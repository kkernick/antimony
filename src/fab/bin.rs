//! The Binary Fabricator determines all executables that are used by the program by analyzing
//! it underneath a specialized SECCOMP Notifier. It then passes these binaries to the Library
//! Fabricator to analyze under LDD.

use crate::{
    fab::{
        ELF_MAGIC, FabInfo, elf_filter, get_cache, get_wildcards, in_lib,
        lib::{self, WildcardFilter},
        localize_path, write_cache,
    },
    shared::{
        Map, Set, ThreadSet, direct_path,
        profile::{Profile, files::FileMode},
        store::Object,
        utility,
    },
    timer,
};
use anyhow::{Context, Result};
use bilrost::{Enumeration, Message};
use log::{debug, warn};
use parking_lot::Mutex;
use rayon::prelude::*;
use spawn::{Spawner, StreamMode};
use std::{
    borrow::Cow,
    fs::{self, File},
    io::{self, BufRead, BufReader, Read, Seek},
    path::{Path, PathBuf},
    sync::Arc,
};
use temp::Temp;
use user::as_real;
use which::which;

#[derive(Message, Debug)]
struct Cache {
    t: Type,
    parse: ParseReturn,
}

/// The kind of thing we just analyzed.
#[derive(Debug, Enumeration, Eq, PartialEq)]
pub enum Type {
    Elf = 0,
    File = 1,
    Script = 2,
    Link = 3,
    Directory = 4,
    Done = 5,
    None = 6,
}

/// Information returned from parse.
#[derive(Default, Message, Debug)]
pub struct ParseReturn {
    /// ELF files, to be passed to the library fabricator.
    pub elf: Set<String>,

    /// Regular files, which act as heuristics for library folders.
    pub files: Set<String>,

    /// Script files, which need no further parsing, but must be mounted.
    pub scripts: Set<String>,

    /// Symlinks
    pub symlinks: Set<(String, String)>,

    /// Localized values
    pub localized: Map<String, String>,

    /// Library directories.
    pub directories: Set<String>,
}
impl ParseReturn {
    fn new() -> Self {
        Self::default()
    }

    fn merge(global: &mut Self, cache: Self) {
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
            global.symlinks.insert((k, v));
        }
    }
}

/// Resolve the path of a binary, canonicalized to /usr/bin.
fn resolve_bin(path: &str) -> Result<Cow<'_, str>> {
    let resolved: Cow<'_, str> = if path.contains("..") {
        let path = as_real!(Result<String>, {
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
    instance: &Temp,
    global: &Mutex<ParseReturn>,
    done: Arc<ThreadSet<String>>,
    mut include_self: bool,
) -> Result<Type> {
    // Avoid duplicate work
    if !done.insert(path.to_string()) {
        return Ok(Type::Done);
    }

    if let Ok(Some(cache)) = get_cache::<Cache>(path, Object::Binaries) {
        debug!("Using cache");
        let mut lock = global.lock();
        ParseReturn::merge(&mut lock, cache.parse);
        return Ok(cache.t);
    }

    let resolved = match resolve_bin(path) {
        Ok(path) => path,
        Err(_) => return Ok(Type::None),
    };

    let mut ret = ParseReturn::new();

    let dest = as_real!(Result<String>, {
        let mut dest = fs::read_link(resolved.as_ref())?;
        if !dest.is_absolute()
            && let Some(parent) = Path::new(resolved.as_ref()).parent()
        {
            dest = parent.join(dest)
        }

        let dest = dest.canonicalize()?;
        Ok(resolve_bin(&dest.to_string_lossy())?.into_owned())
    })?;

    // Ensure it's a valid binary.
    let t = {
        if let Ok(dest) = dest {
            parse(&dest, instance, global, done.clone(), true)?;
            if include_self {
                if !dest.contains("bin")
                    && let Some(i) = dest.rfind("/")
                {
                    ret.directories.insert(dest[..i].to_string());
                }

                ret.symlinks.insert((resolved.into_owned(), dest));
            }
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
            let mut magic = [0u8; 4];
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

                let mut iter = as_real!(Result<_>, {
                    file.seek(io::SeekFrom::Start(0))?;
                    let reader = BufReader::new(file);
                    Ok(reader.lines())
                })??;

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
                            &instance.full().to_string_lossy(),
                            "--filter",
                            "execve",
                        ])
                        .output(StreamMode::Pipe)
                        .error(StreamMode::Discard)
                        .preserve_env(true)
                        .new_privileges(true)
                        .mode(user::Mode::Real)
                        .spawn()?
                        .output_all()?;

                    let out = out.lines().map(String::from);
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

    let cache = write_cache(path, Cache { t, parse: ret }, Object::Binaries)?;
    {
        let mut lock = global.lock();
        ParseReturn::merge(&mut lock, cache.parse);
    }
    Ok(cache.t)
}

/// Get the immediate parent within /usr/lib.
#[inline]
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
    instance: &Temp,
    home: bool,
    include_self: bool,
    parsed: &Mutex<ParseReturn>,
    done: Arc<ThreadSet<String>>,
) -> Result<()> {
    let file = which::which(file).unwrap_or(file);
    if let (Some(src), dst) = localize_path(file, home)? {
        if src == dst {
            parse(file, instance, parsed, done.clone(), include_self)?;
        } else {
            match parse(&src, instance, parsed, done.clone(), false)? {
                Type::Script | Type::File | Type::Elf => {
                    parsed.lock().localized.insert(src.into_owned(), dst);
                }

                Type::Link => {
                    let link = fs::read_link(src.as_ref())?;
                    let (_, ldst) = localize_path(&link.to_string_lossy(), home)?;
                    handle_localize(&ldst, instance, home, false, parsed, done.clone())?;
                    parsed.lock().symlinks.insert((dst, ldst));
                }
                _ => {}
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
/// in the files header.
pub fn collect(
    profile: &Profile,
    name: &str,
    instance: &Temp,
    parsed: &Mutex<ParseReturn>,
) -> Result<()> {
    let resolved = Arc::new(ThreadSet::default());
    resolved.insert(profile.app_path(name).to_string());

    // Scope so the lock falls out of scope.
    {
        let binaries = &profile.binaries;
        timer!("::collect::wildcard", {
            // Separate the wildcards from the files/dirs.
            let (wildcards, flat): (Set<_>, Set<_>) =
                binaries.into_par_iter().partition(|e| e.contains('*'));

            flat.into_par_iter().for_each(|f| {
                resolved.insert(f.clone());
            });

            wildcards.into_par_iter().for_each(|w| {
                match get_wildcards(w, false, WildcardFilter::Files) {
                    Ok(cards) => {
                        for card in cards {
                            resolved.insert(card);
                        }
                    }
                    Err(e) => warn!("Failed to get wildcards for {w}: {e}"),
                }
            })
        });
    }

    let done = Arc::new(ThreadSet::default());

    // Read direct files so we can determine dependencies.
    timer!("::collect::files", {
        if let Some(files) = &profile.files {
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
    timer!("::collect::localization", {
        Arc::into_inner(resolved)
            .unwrap()
            .into_par_iter()
            .for_each(|binary| {
                if let Err(e) =
                    handle_localize(binary.as_str(), instance, false, true, parsed, done.clone())
                {
                    warn!("Failed to localize {binary}: {e}");
                }
            });
    });
    Ok(())
}

/// Fabricate the binaries.
pub fn fabricate(info: &mut FabInfo) -> Result<()> {
    {
        let binaries = &info.profile.binaries;
        if binaries.contains("/usr/bin") {
            #[rustfmt::skip]
            info.handle.args_i([
                "--overlay-src", "/usr/bin",
                "--tmp-overlay", "/usr/bin",

                "--overlay-src", "/usr/sbin",
                "--tmp-overlay", "/usr/sbin",

                "--symlink", "/usr/bin", "/bin",
                "--symlink", "/usr/sbin", "/sbin",
            ]);
            return Ok(());
        }
    }

    info.handle.args_i(["--dir", "/usr/bin"]);
    let bin_cache = format!("{}-bin", info.instance.name());
    let parsed = match get_cache(&bin_cache, Object::Binaries) {
        Ok(Some(parsed)) => parsed,
        _ => {
            let parsed = Mutex::new(ParseReturn::new());
            timer!(
                "::collect",
                collect(info.profile, info.name, info.instance, &parsed,)
            )?;

            let parsed = parsed.into_inner();
            write_cache(&bin_cache, parsed, Object::Binaries)?
        }
    };

    let elf_binaries = Arc::new(ThreadSet::default());

    // ELF files need to be processed by the library fabricator,
    // to use LDD on depends.
    timer!("::elf", {
        parsed.elf.into_par_iter().try_for_each(|elf| {
            info.handle.args_i(["--ro-bind", &elf, &elf]);
            elf_binaries.insert(elf.to_string());
            anyhow::Ok(())
        })?;
    });

    // Scripts are consumed here, and are only bound to the sandbox.
    timer!("::scripts", {
        parsed
            .scripts
            .into_par_iter()
            .for_each(|script| info.handle.args_i(["--ro-bind", &script, &script]));
    });

    timer!("::files", {
        parsed
            .files
            .into_par_iter()
            .for_each(|file| info.handle.args_i(["--ro-bind", &file, &file]))
    });

    timer!("::localized", {
        parsed.localized.into_par_iter().try_for_each(
            |(src, dst): (String, String)| -> anyhow::Result<()> {
                info.handle.args_i(["--ro-bind", &src, &dst]);
                if elf_filter(&src) {
                    elf_binaries.insert(src);
                }
                Ok(())
            },
        )
    })?;

    if !parsed.directories.is_empty() {
        let libraries = info.profile.libraries.get_or_insert_default();
        timer!("::libraries", {
            parsed.directories.into_iter().for_each(|dir| {
                let _ = libraries.directories.insert(dir);
            });
        });
    }

    timer!("::symlinks", {
        parsed
            .symlinks
            .into_par_iter()
            .try_for_each(|(link, dest)| -> anyhow::Result<()> {
                if !elf_binaries.contains(&dest) {
                    info.handle.args_i(["--ro-bind", &dest, &dest]);
                    elf_binaries.insert(dest.clone());
                }
                if !in_lib(&link) {
                    info.handle.args_i(["--symlink", &dest, &link]);
                }
                Ok(())
            })
    })?;

    if let Some(home) = &info.profile.home {
        timer!("::home_binaries", {
            let home_dir = home.path(info.name);
            if home_dir.exists() {
                let home_str = home_dir.to_string_lossy();
                lib::DIRS.insert(home_str.into_owned());
            }
        })
    }

    info.handle.args_i(["--symlink", "/usr/bin", "/bin"]);

    if fs::read_link("/usr/sbin").is_ok() {
        info.handle.args_i([
            "--symlink",
            "/usr/bin",
            "/usr/sbin",
            "--symlink",
            "/usr/bin",
            "/sbin",
        ]);
    }

    info.profile.binaries = Arc::into_inner(elf_binaries).unwrap().into_iter().collect();
    Ok(())
}
