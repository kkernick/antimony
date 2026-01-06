//! The Binary Fabricator determines all executables that are used by the program by analyzing
//! it underneath a specialized SECCOMP Notifier. It then passes these binaries to the Library
//! Fabricator to analyze under LDD.

use crate::{
    fab::{
        ELF_MAGIC, FabInfo, get_cache, get_dir, get_wildcards, lib::in_lib, localize_path,
        write_cache,
    },
    shared::{
        Set,
        db::Table,
        direct_path, format_iter,
        profile::{Profile, files::FileMode},
        utility,
    },
    timer,
};
use anyhow::{Context, Result, anyhow};
use dashmap::{DashMap, DashSet};
use log::{trace, warn};
use parking_lot::Mutex;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use spawn::{Spawner, StreamMode};
use std::{
    borrow::Cow,
    fs::{self, File},
    io::{self, BufRead, BufReader, Read, Seek},
    path::{Path, PathBuf},
    sync::Arc,
};
use user::{as_real, run_as};
use which::which;

/// The kind of thing we just analyzed.
pub enum Type {
    Elf,
    File,
    Script,
    Link,
    Directory,
    Done,
    None,
}

/// Information returned from parse.
#[derive(Default, Serialize, Deserialize)]
pub struct ParseReturn {
    /// ELF files, to be passed to the library fabricator.
    pub elf: DashSet<String>,

    /// Regular files, which act as heuristics for library folders.
    pub files: DashSet<String>,

    /// Script files, which need no further parsing, but must be mounted.
    pub scripts: DashSet<String>,

    /// Symlinks
    pub symlinks: DashMap<String, String>,

    /// Localized values
    pub localized: DashMap<String, String>,

    /// Library directories.
    pub directories: DashSet<String>,
}
impl ParseReturn {
    fn new() -> Self {
        Self::default()
    }

    fn merge(global: &Self, cache: Self) {
        for elf in cache.elf {
            global.elf.insert(elf);
        }
        for script in cache.scripts {
            global.scripts.insert(script);
        }
        for file in cache.files {
            global.files.insert(file);
        }
        for dir in cache.directories {
            global.directories.insert(dir);
        }
        for (k, v) in cache.symlinks {
            global.symlinks.insert(k, v);
        }
    }
}

/// Resolve the path of a binary, canonicalized to /usr/bin.
fn resolve_bin(path: &str) -> Result<Cow<'_, str>> {
    let resolved: Cow<'_, str> = if path.contains("..") {
        let path = run_as!(user::Mode::Real, Result<String>, {
            Ok(Path::new(path)
                .canonicalize()?
                .to_string_lossy()
                .into_owned())
        })??;
        Cow::Owned(path)
    } else if path.starts_with('/') {
        Cow::Borrowed(path)
    } else {
        Cow::Borrowed(which(path).with_context(|| path.to_string())?)
    };

    if resolved.starts_with("/bin") {
        Ok(Cow::Owned(format!("/usr{resolved}")))
    } else {
        Ok(resolved)
    }
}

/// Parses binaries, specifically for shell scripts.
fn parse(
    path: &str,
    instance: &str,
    global: &ParseReturn,
    done: Arc<DashSet<String>>,
    mut include_self: bool,
) -> Result<Type> {
    // Avoid duplicate work
    if !done.insert(path.to_string()) {
        return Ok(Type::Done);
    }

    if let Some(cache) = get_cache(path, Table::Binaries)? {
        ParseReturn::merge(global, cache);
        return Ok(Type::None);
    }

    let resolved = match resolve_bin(path) {
        Ok(path) => path,
        Err(_) => return Ok(Type::None),
    };

    let ret = ParseReturn::new();

    let dest = run_as!(user::Mode::Real, Result<String>, {
        let dest = fs::read_link(resolved.as_ref())?;
        let canon = Path::new(resolved.as_ref())
            .canonicalize()?
            .parent()
            .ok_or(anyhow!("Binary does not have parent!"))?
            .join(&dest);
        Ok(canon.to_string_lossy().into_owned())
    })?;

    // Ensure it's a valid binary.
    let t = {
        if let Ok(dest) = dest {
            if include_self {
                match resolve_bin(dest.as_ref()) {
                    Ok(dest) => ret
                        .symlinks
                        .insert(resolved.into_owned(), dest.into_owned()),

                    Err(e) => {
                        warn!("Could not resolve symlink destination {dest}: {e}");
                        return Ok(Type::None);
                    }
                };
            }
            parse(&dest, instance, global, done.clone(), true)?;
            Type::Link
        } else {
            if in_lib(path) {
                if let Some(parent) = resolve_dir(path)?
                    && PathBuf::from(&parent).is_dir()
                {
                    ret.directories.insert(parent);
                }
                include_self = false;
            }

            // Open it.
            let mut file = match as_real!({ File::open(resolved.as_ref()) })? {
                Ok(file) => file,
                Err(_) => return Ok(Type::None),
            };

            // Get the magic.
            let mut magic = [0u8; 5];
            if file.read_exact(&mut magic).is_err() {
                return Ok(Type::None);
            }

            // ELF binaries are returned, as they are LDD'd by the library fabricator.
            if magic == ELF_MAGIC {
                if include_self {
                    ret.elf.insert(resolved.to_string());
                }
                Type::Elf
            }
            // Shell scripts are parsed, but they aren't added to the return since
            // LDD can't deal with them. Programs used in the script, however,
            // will be added if the themselves are ELF binaries.
            else if magic[0] == b'#' {
                if include_self {
                    ret.scripts.insert(resolved.to_string());
                }

                let mut binaries = Vec::new();
                // Rewind.
                file.seek(io::SeekFrom::Start(0))?;
                let reader = BufReader::new(file);
                let mut iter = reader.lines();

                // Grab the shebang
                let header = match iter.next() {
                    Some(line) => match line {
                        Ok(line) => line,
                        Err(_) => return Ok(Type::None),
                    },
                    None => return Ok(Type::None),
                };

                binaries.extend(
                    header
                        .split(' ')
                        .map(|token| token.strip_prefix("#!").unwrap_or(token).to_string()),
                );

                if ["dash", "bash", "sh", "zsh"]
                    .into_iter()
                    .any(|shell| header.contains(shell))
                {
                    let out = Spawner::abs(utility("dumper"))
                        .args([
                            "run",
                            "--path",
                            &resolved,
                            "--instance",
                            instance,
                            "--filter",
                            "execve",
                        ])?
                        .output(StreamMode::Pipe)
                        .preserve_env(true)
                        .new_privileges(true)
                        .mode(user::Mode::Real)
                        .spawn()?
                        .output_all()?;

                    let out = out.lines().map(String::from);
                    trace!("{resolved} => {}", format_iter(binaries.iter()));
                    binaries.extend(out);
                }

                for bin in binaries {
                    parse(&bin, instance, global, done.clone(), true)?;
                }

                Type::Script
            } else {
                if include_self {
                    ret.files.insert(resolved.to_string());
                }
                Type::File
            }
        }
    };

    write_cache(path, &ret, Table::Binaries)?;
    ParseReturn::merge(global, ret);
    Ok(t)
}

/// Get the immediate parent within /usr/lib.
fn resolve_dir(path: &str) -> Result<Option<String>> {
    let lib_root = Path::new("/usr/lib");
    let mut path = Path::new(&path);
    while let Some(parent) = path.parent() {
        if parent == lib_root {
            return Ok(Some(path.to_string_lossy().into_owned()));
        }
        path = parent;
    }
    Ok(None)
}

/// Localization means pointing things in $HOME to /home/antimony, ensuring
/// that environment variables in the filename are properly resolved, and
/// that symlinks are properly managed. It also parses all intermediary files
/// (IE the destination to a symlink).
fn handle_localize(
    file: &str,
    instance: &str,
    home: bool,
    include_self: bool,
    parsed: &ParseReturn,
    done: Arc<DashSet<String>>,
) -> Result<()> {
    let file = which::which(file).unwrap_or(file);
    if let (Some(src), dst) = localize_path(file, home)? {
        if src == dst {
            parse(file, instance, parsed, done.clone(), include_self)?;
        } else {
            match parse(&src, instance, parsed, done.clone(), false)? {
                Type::Script | Type::File | Type::Elf => {
                    parsed.localized.insert(src.into_owned(), dst);
                }

                Type::Link => {
                    let link = fs::read_link(src.as_ref())?;
                    let (_, ldst) = localize_path(&link.to_string_lossy(), home)?;
                    handle_localize(&ldst, instance, home, false, parsed, done.clone())?;
                    parsed.symlinks.insert(dst, ldst);
                }
                _ => warn!("Excluding localization for {file}"),
            }
        }
        Ok(())
    } else {
        parse(file, instance, parsed, done.clone(), true)?;
        Ok(())
    }
}

/// Collection takes all the binaries defined in the profiles, and parses them.
/// This includes resolving wildcards, and parsing files tagged as Executable
/// in the [files] header.
pub fn collect(
    profile: &Mutex<Profile>,
    name: &str,
    instance: &str,
    parsed: &ParseReturn,
) -> Result<()> {
    let resolved = Arc::new(DashSet::new());
    resolved.insert(profile.lock().app_path(name).to_string());

    // Scope so the lock falls out of scope.
    {
        let binaries = &profile.lock().binaries;
        timer!("::collect::wildcard", {
            // Separate the wildcards from the files/dirs.
            let (wildcards, flat): (Set<_>, Set<_>) =
                binaries.into_par_iter().partition(|e| e.contains('*'));

            flat.into_par_iter().for_each(|f| {
                resolved.insert(f.clone());
            });

            wildcards.into_par_iter().for_each(|w| {
                if let Ok(cards) = get_wildcards(w, false) {
                    for card in cards {
                        resolved.insert(card);
                    }
                }
            })
        });
    }

    let done = Arc::new(DashSet::new());

    // Read direct files so we can determine dependencies.
    timer!("::collect::files", {
        if let Some(files) = &profile.lock().files {
            if let Some(x) = files.user.get(&FileMode::Executable) {
                x.iter().try_for_each(|file| {
                    handle_localize(file, instance, true, true, parsed, done.clone())
                })?;
            }
            if let Some(x) = files.resources.get(&FileMode::Executable) {
                x.iter().try_for_each(|file| {
                    handle_localize(file, instance, false, true, parsed, done.clone())
                })?;
            }
            if let Some(x) = files.platform.get(&FileMode::Executable) {
                x.iter().try_for_each(|file| {
                    handle_localize(file, instance, false, true, parsed, done.clone())
                })?;
            }
            if let Some(x) = files.direct.get(&FileMode::Executable) {
                x.iter().try_for_each(|(file, _)| {
                    let path = direct_path(file);
                    handle_localize(
                        &path.to_string_lossy(),
                        instance,
                        false,
                        false,
                        parsed,
                        done.clone(),
                    )
                })?;
            }
        }
    });

    // Parse the binaries
    // Parallelizing this causes deadlocks. Perhaps handle_localize calls itself with other parallelization too aggressively.
    timer!("::collect::localization", {
        Arc::into_inner(resolved)
            .unwrap()
            .into_iter()
            .try_for_each(|binary| {
                handle_localize(binary.as_str(), instance, false, true, parsed, done.clone())
            })?;
    });
    Ok(())
}

/// Fabricate the binaries.
pub fn fabricate(info: &FabInfo) -> Result<()> {
    {
        let binaries = &info.profile.lock().binaries;
        if binaries.contains("/usr/bin") {
            #[rustfmt::skip]
            info.handle.args_i([
                "--ro-bind", "/usr/bin", "/usr/bin",
                "--ro-bind", "/usr/sbin", "/usr/sbin",
                "--symlink", "/usr/bin", "/bin",
                "--symlink", "/usr/sbin", "/sbin",
            ])?;
            return Ok(());
        }
    }

    info.handle.args_i(["--dir", "/usr/bin"])?;
    let bin_cache = format!("{}-bin", info.instance.name());
    let parsed = match get_cache(&bin_cache, Table::Binaries)? {
        Some(parsed) => parsed,
        None => {
            let parsed = ParseReturn::new();
            timer!(
                "::collect",
                collect(info.profile, info.name, info.instance.name(), &parsed,)
            )?;

            write_cache(&bin_cache, &parsed, Table::Binaries)?;
            parsed
        }
    };

    let elf_binaries = Arc::new(DashSet::<String>::new());

    // ELF files need to be processed by the library fabricator,
    // to use LDD on depends.
    timer!("::elf", {
        parsed.elf.into_par_iter().try_for_each(|elf| {
            info.handle.args_i(["--ro-bind", &elf, &elf])?;
            elf_binaries.insert(elf.to_string());
            anyhow::Ok(())
        })?;
    });

    // Scripts are consumed here, and are only bound to the sandbox.
    timer!("::scripts", {
        parsed
            .scripts
            .into_par_iter()
            .try_for_each(|script| info.handle.args_i(["--ro-bind", &script, &script]))?;
    });

    timer!("::files", {
        parsed
            .files
            .into_par_iter()
            .try_for_each(|file| info.handle.args_i(["--ro-bind", &file, &file]))
    })?;

    timer!("::localized", {
        parsed
            .localized
            .into_iter()
            .try_for_each(|(src, dst)| -> anyhow::Result<()> {
                info.handle.args_i(["--ro-bind", &src, &dst])?;
                elf_binaries.insert(src);
                Ok(())
            })
    })?;

    timer!("::libraries", {
        info.profile.lock().libraries.extend(parsed.directories)
    });

    timer!("::symlinks", {
        parsed
            .symlinks
            .into_par_iter()
            .try_for_each(|(link, dest)| -> anyhow::Result<()> {
                if !in_lib(&link) {
                    info.handle.args_i(["--ro-bind", &dest, &link])?;
                }
                Ok(())
            })
    })?;

    if let Some(home) = &info.profile.lock().home {
        timer!("::home_binaries", {
            let home_dir = home.path(info.name);
            if home_dir.exists() {
                let home_str = home_dir.to_string_lossy();
                let home_binaries = get_dir(&home_str)?;
                for binary in home_binaries {
                    elf_binaries.insert(binary);
                }
            }
        })
    }

    #[rustfmt::skip]
    info.handle.args_i([
        "--symlink", "/usr/bin", "/bin",
        "--symlink", "/usr/sbin", "/sbin"

    ])?;

    info.profile.lock().binaries = Arc::into_inner(elf_binaries)
        .expect("Failed to get elf binaries")
        .into_iter()
        .collect();
    Ok(())
}
