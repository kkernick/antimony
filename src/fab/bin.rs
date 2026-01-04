use crate::{
    fab::{
        FabInfo, get_dir, get_wildcards,
        lib::{add_sof, in_lib},
        localize_path,
    },
    shared::{
        Set, format_iter,
        path::direct_path,
        profile::{FileMode, Profile},
        utility,
    },
    timer,
};
use anyhow::{Context, Result, anyhow};
use dashmap::{DashMap, DashSet};
use log::{debug, error, trace, warn};
use parking_lot::Mutex;
use rayon::prelude::*;
use spawn::{Spawner, StreamMode};
use user::{as_effective, as_real, run_as};

use std::{
    borrow::Cow,
    fs::{self, File},
    io::{self, BufRead, BufReader, Read, Seek, Write},
    path::{Path, PathBuf},
    sync::Arc,
};
use which::which;

/// The magic for an ELF file.
pub static ELF_MAGIC: [u8; 5] = [0x7F, b'E', b'L', b'F', 2];

#[derive(Debug)]
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
#[derive(Debug, Default)]
pub struct ParseReturn {
    /// The location of the cache
    cache: PathBuf,

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
    fn new(cache: &Path) -> Self {
        Self {
            cache: cache.to_path_buf(),
            ..Default::default()
        }
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

    /// Get cached definitions if they exist.
    fn cache(name: &str, cache: &Path) -> Result<Option<Self>> {
        let cache_file = cache.join(name.replace("/", ".").replace("*", "."));
        if cache_file.exists() {
            let mut ret = Self::default();
            let file = File::open(&cache_file)?;
            let reader = BufReader::new(file);
            let mut lines = reader.lines();

            let mut next = || -> Result<DashSet<_>> {
                Ok(lines
                    .next()
                    .ok_or(0)
                    .map_err(|_| anyhow!("Corrupt cache!"))??
                    .split(" ")
                    .map(|e| e.to_string())
                    .filter(|e| !e.is_empty())
                    .collect())
            };

            ret.elf.extend(next()?);
            ret.scripts.par_extend(next()?);
            ret.files.par_extend(next()?);
            ret.directories.par_extend(next()?);
            ret.symlinks
                .par_extend(next()?.into_par_iter().filter_map(|e| {
                    if let Some((key, value)) = e.split_once("=") {
                        Some((key.to_string(), value.to_string()))
                    } else {
                        None
                    }
                }));
            Ok(Some(ret))
        } else {
            Ok(None)
        }
    }

    /// Write a cache file.
    fn write(&self, name: &str) -> Result<()> {
        let cache_file = self.cache.join(name.replace("/", ".").replace("*", "."));

        as_effective!(Result<()>, {
            let mut file = File::create(&cache_file)?;

            let mut write = |dash: &DashSet<String>| -> Result<()> {
                dash.iter()
                    .try_for_each(|elf| write!(file, "{} ", elf.as_str()))?;
                writeln!(file)?;
                Ok(())
            };

            write(&self.elf)?;
            write(&self.scripts)?;
            write(&self.files)?;
            write(&self.directories)?;
            write(
                &self
                    .symlinks
                    .iter()
                    .map(|pair| format!("{}={}", pair.key(), pair.value()))
                    .collect(),
            )?;
            Ok(())
        })?
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
    cache: &Path,
    done: Arc<DashSet<String>>,
    mut include_self: bool,
) -> Result<Type> {
    // Avoid duplicate work
    if !done.insert(path.to_string()) {
        return Ok(Type::Done);
    }

    if let Some(cache) = ParseReturn::cache(path, cache)? {
        ParseReturn::merge(global, cache);
        return Ok(Type::None);
    }

    let resolved = match resolve_bin(path) {
        Ok(path) => path,
        Err(_) => return Ok(Type::None),
    };

    let ret = ParseReturn::new(cache);

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
            parse(&dest, instance, global, cache, done.clone(), true)?;
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
                        .args(["run", "--path", &resolved, "--instance", instance])?
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
                    parse(&bin, instance, global, cache, done.clone(), true)?;
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

    ret.write(path)?;
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

fn handle_localize(
    file: &str,
    instance: &str,
    home: bool,
    include_self: bool,
    parsed: &ParseReturn,
    done: Arc<DashSet<String>>,
    cache: &Path,
) -> Result<()> {
    let file = which::which(file).unwrap_or(file);
    if let (Some(src), dst) = localize_path(file, home)? {
        if src == dst {
            parse(file, instance, parsed, cache, done.clone(), include_self)?;
        } else {
            match parse(&src, instance, parsed, cache, done.clone(), false)? {
                Type::Script | Type::File | Type::Elf => {
                    parsed.localized.insert(src.into_owned(), dst);
                }

                Type::Link => {
                    let link = fs::read_link(src.as_ref())?;
                    let (_, ldst) = localize_path(&link.to_string_lossy(), home)?;
                    handle_localize(&ldst, instance, home, false, parsed, done.clone(), cache)?;
                    parsed.symlinks.insert(dst, ldst);
                }
                _ => warn!("Excluding localization for {file}"),
            }
        }
        Ok(())
    } else {
        parse(file, instance, parsed, cache, done.clone(), true)?;
        Ok(())
    }
}

pub fn collect(
    profile: &Mutex<Profile>,
    name: &str,
    instance: &str,
    parsed: &ParseReturn,
    cache: &Path,
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
                if let Ok(cards) = get_wildcards(w, false, Some(cache)) {
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
                    handle_localize(file, instance, true, true, parsed, done.clone(), cache)
                })?;
            }
            if let Some(x) = files.resources.get(&FileMode::Executable) {
                x.iter().try_for_each(|file| {
                    handle_localize(file, instance, false, true, parsed, done.clone(), cache)
                })?;
            }
            if let Some(x) = files.platform.get(&FileMode::Executable) {
                x.iter().try_for_each(|file| {
                    handle_localize(file, instance, false, true, parsed, done.clone(), cache)
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
                        cache,
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
                handle_localize(
                    binary.as_str(),
                    instance,
                    false,
                    true,
                    parsed,
                    done.clone(),
                    cache,
                )
            })?;
    });
    Ok(())
}

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

    let cache = crate::shared::env::CACHE_DIR.join(".bin");
    if !cache.exists() {
        as_effective!({ std::fs::create_dir_all(&cache).expect("Failed to create binary cache") })?;
    }

    let parsed = match ParseReturn::cache("bin.cache", info.sys_dir)? {
        Some(parsed) => parsed,
        None => {
            let parsed = ParseReturn::new(info.sys_dir);
            timer!(
                "::collect",
                collect(
                    info.profile,
                    info.name,
                    info.instance.name(),
                    &parsed,
                    &cache,
                )
            )?;

            parsed.write("bin.cache")?;
            parsed
        }
    };

    let elf_binaries = Arc::new(DashSet::<String>::new());

    debug!("Creating Binary Folder");
    let bin = info.sys_dir.join("bin");
    if !bin.exists() {
        fs::create_dir(&bin)?;
    }

    // ELF files need to be processed by the library fabricator,
    // to use LDD on depends.
    timer!("::elf", {
        parsed.elf.into_par_iter().for_each(|elf| {
            if let Err(e) = add_sof(&bin, Cow::Borrowed(&elf), &cache, "/usr/bin") {
                error!("Failed to add {elf} to Bin: {e}")
            }
            elf_binaries.insert(elf.to_string());
        })
    });

    // Scripts are consumed here, and are only bound to the sandbox.
    timer!("::scripts", {
        parsed.scripts.into_par_iter().for_each(|script| {
            if let Err(e) = add_sof(&bin, Cow::Borrowed(&script), &cache, "/usr/bin") {
                error!("Failed to add {script} to Bin: {e}")
            }
        })
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
                    if dest.starts_with("/usr/bin") {
                        if let Err(e) = add_sof(&bin, Cow::Borrowed(&link), &cache, "/usr/bin") {
                            error!("Failed to add {link} to SOF: {e}")
                        }
                    } else {
                        info.handle.args_i([
                            "--ro-bind",
                            &dest,
                            &dest,
                            "--symlink",
                            &dest,
                            &link,
                        ])?;
                    }
                }
                Ok(())
            })
    })?;

    if let Some(home) = &info.profile.lock().home {
        timer!("::home_binaries", {
            let home_dir = home.path(info.name);
            if home_dir.exists() {
                let home_str = home_dir.to_string_lossy();
                let home_binaries = get_dir(&home_str, Some(&cache))?;
                for binary in home_binaries {
                    elf_binaries.insert(binary);
                }
            }
        })
    }

    let bin_str = bin.to_string_lossy();

    #[rustfmt::skip]
    info.handle.args_i([
        "--ro-bind-try", &bin_str, "/usr/bin",
        "--symlink", "/usr/bin", "/bin",
        "--symlink", "/usr/sbin", "/sbin"

    ])?;

    info.profile.lock().binaries = Arc::into_inner(elf_binaries)
        .expect("Failed to get elf binaries")
        .into_iter()
        .collect();
    Ok(())
}
