use crate::{
    aux::{
        env::{AT_HOME, DATA_HOME},
        path::direct_path,
        profile::{FileMode, Profile},
    },
    fab::{lib::get_dir, localize_path},
};
use anyhow::{Context, Result, anyhow};
use dashmap::{DashMap, DashSet};
use log::{debug, trace};
use once_cell::sync::Lazy;
use rayon::prelude::*;
use spawn::Spawner;
use std::{
    borrow::Cow,
    collections::{BTreeSet, HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader, Read, Seek, Write},
    path::{Path, PathBuf},
    sync::Arc,
};
use which::which;

/// Characters used for splitting.
static CHARS: Lazy<HashSet<char>> = Lazy::new(|| {
    ['"', '\'', ';', '=', '$', '(', ')', '{', '}']
        .into_iter()
        .collect()
});

/// Reserved keywords in bash.
static COMPGEN: Lazy<HashSet<String>> = Lazy::new(|| {
    let mut compgen: HashSet<String> = Spawner::new("/usr/bin/bash")
        .args(["-c", "compgen -k"])
        .unwrap()
        .output(true)
        .mode(user::Mode::Real)
        .spawn()
        .unwrap()
        .output_all()
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();

    // These ones are usually false positives.
    compgen.insert("true".to_string());
    compgen.insert("false".to_string());

    compgen
});

/// The magic for an ELF file.
pub static ELF_MAGIC: [u8; 5] = [0x7F, b'E', b'L', b'F', 2];

/// The location to store cache files.
static CACHE_DIR: Lazy<PathBuf> =
    Lazy::new(|| PathBuf::from(AT_HOME.as_path()).join("cache").join(".bin"));

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
struct ParseReturn {
    /// ELF files, to be passed to the library fabricator.
    pub elf: DashSet<String>,

    /// Regular files, which act as heuristics for library folders.
    pub files: DashSet<String>,

    /// Script files, which need no further parsing, but must be mounted.
    pub scripts: DashSet<String>,

    /// Symlinks
    pub symlinks: DashMap<String, String>,

    /// Library directories.
    pub directories: DashSet<String>,

    /// Binaries already processed.
    pub done: DashSet<String>,
}
impl ParseReturn {
    /// Get cached definitions if they exist.
    fn cache(name: &str) -> Result<Option<Self>> {
        let cache_file = CACHE_DIR.join(name.replace("/", ".").replace("*", "."));
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
            ret.symlinks.par_extend(
                next()?
                    .into_iter()
                    .filter_map(|e| {
                        if let Some((key, value)) = e.split_once("=") {
                            Some((key.to_string(), value.to_string()))
                        } else {
                            None
                        }
                    })
                    .collect::<DashMap<_, _>>(),
            );
            Ok(Some(ret))
        } else {
            Ok(None)
        }
    }

    /// Write a cache file.
    fn write(&self, name: &str) -> Result<()> {
        let cache_file = CACHE_DIR.join(name.replace("/", ".").replace("*", "."));
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
    }

    /// Merge two Parse Returns together.
    fn merge(&self, rh: ParseReturn) {
        rh.elf.into_par_iter().for_each(|elf| {
            self.elf.insert(elf);
        });
        rh.files.into_par_iter().for_each(|file| {
            self.files.insert(file);
        });
        rh.scripts.into_par_iter().for_each(|script| {
            self.scripts.insert(script);
        });
        rh.symlinks.into_par_iter().for_each(|symlink| {
            self.symlinks.insert(symlink.0, symlink.1);
        });
        rh.directories.into_par_iter().for_each(|dir| {
            self.directories.insert(dir);
        });
    }
}

/// Tokenize a string
fn tokenize(line: String) -> HashSet<String> {
    let mut ret = HashSet::new();
    for token in line.split_whitespace() {
        let token: String = token.chars().filter(|e| !CHARS.contains(e)).collect();
        if COMPGEN.contains(&token) {
            continue;
        }
        ret.insert(token);
    }
    ret
}

/// Resolve the path of a binary, canonicalized to /usr/bin.
fn resolve_bin(path: &str) -> Result<Cow<'_, str>> {
    let resolved = if path.starts_with('/') {
        Cow::Borrowed(path)
    } else {
        Cow::Owned(
            which(path)
                .with_context(|| path.to_string())?
                .to_string_lossy()
                .into_owned(),
        )
    };

    if resolved.starts_with("/bin") {
        Ok(Cow::Owned(format!("/usr{resolved}")))
    } else {
        Ok(resolved)
    }
}

/// Parses binaries, specifically for shell scripts.
fn parse(path: &str, ret: Arc<ParseReturn>, include_self: bool) -> Result<Type> {
    // Avoid duplicate work
    if !ret.done.insert(path.to_string()) {
        return Ok(Type::Done);
    }

    trace!("Parsing {path}");

    let resolved = match resolve_bin(path) {
        Ok(path) => path,
        Err(_) => return Ok(Type::None),
    };

    // Ensure it's a valid binary.
    if let Ok(dest) = std::fs::read_link(resolved.as_ref()) {
        let dest = dest.to_string_lossy();
        if include_self {
            match resolve_bin(dest.as_ref()) {
                Ok(dest) => {
                    ret.symlinks
                        .insert(resolved.into_owned(), dest.into_owned());
                }
                Err(_) => return Ok(Type::None),
            };
        }

        parse(&dest, ret.clone(), true)?;
        return Ok(Type::Link);
    }

    if path.starts_with("/usr/lib") {
        if let Some(parent) = resolve_dir(path)? {
            debug!("Directory => {parent}");
            ret.directories.insert(parent);
        }
    }

    // Open it.
    let mut file = match File::open(resolved.as_ref()) {
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
        Ok(Type::Elf)
    }
    // Shell scripts are parsed, but they aren't added to the return since
    // LDD can't deal with them. Programs used in the script, however,
    // will be added if the themselves are ELF binaries.
    else if magic[0] == b'#' {
        if include_self {
            ret.scripts.insert(resolved.to_string());
        }

        if let Some(cache) = ParseReturn::cache(&resolved)? {
            ret.merge(cache);
        } else {
            // Store environment assignment for later evaluation
            let mut environment = HashMap::<String, String>::new();

            // Rewind.
            file.seek(std::io::SeekFrom::Start(0))?;
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

            tokenize(header[2..].to_string())
                .par_iter()
                .try_for_each(|token| -> Result<()> {
                    parse(token, ret.clone(), true)?;
                    Ok(())
                })?;

            for line in iter {
                let mut line = line?.trim().to_string();
                if line.starts_with("#") || line.is_empty() {
                    continue;
                }

                // Substitute variables.
                for (key, value) in &environment {
                    if line.contains(key) {
                        let syntax = format!("${key}");
                        line = line.replace(&syntax, value);

                        let syntax = format!("${{{key}}}");
                        line = line.replace(&syntax, value);

                        let syntax = format!("$({key})");
                        line = line.replace(&syntax, value);
                    }
                }

                if let Some((key, val)) = line.split_once('=') {
                    if !line.starts_with("-") {
                        let mut result = Spawner::new("/usr/bin/bash")
                            .args(["-c", &format!("{line}; echo ${key}")])?
                            .output(true)
                            .error(true)
                            .mode(user::Mode::Real)
                            .spawn()?;

                        let code = result.wait()?;
                        if code == 0 {
                            let result = result.output_all()?;
                            environment.insert(key.to_string(), result);
                            line = val.to_string();
                        }
                    }
                }

                tokenize(line)
                    .par_iter()
                    .try_for_each(|token| -> Result<()> {
                        parse(token, ret.clone(), true)?;
                        Ok(())
                    })?;
            }
            ret.write(path)?;
        }
        Ok(Type::Script)
    } else {
        if include_self {
            ret.files.insert(resolved.to_string());
        }
        Ok(Type::File)
    }
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

pub fn fabricate(profile: &mut Profile, name: &str, handle: &Spawner) -> Result<()> {
    if COMPGEN.is_empty() {
        return Err(anyhow!("Could not calculate bash builtins"));
    }
    std::fs::create_dir_all(CACHE_DIR.as_path())?;

    let mut elf_binaries = BTreeSet::<String>::new();
    let path = profile.app_path(name).to_string();
    let binaries = profile.binaries.get_or_insert_default();

    binaries.insert(path);

    let parsed = Arc::new(ParseReturn::default());

    let handle_localize = |file: &str, home: bool| -> Result<()> {
        if let (Some(src), dst) = localize_path(file, home) {
            if src == dst {
                parse(file, parsed.clone(), true)?;
            } else {
                match parse(&src, parsed.clone(), false)? {
                    Type::Script | Type::File | Type::Elf => {
                        handle.args_i(["--ro-bind", &src, &dst])?
                    }
                    _ => {}
                }
            }
            Ok(())
        } else {
            parse(file, parsed.clone(), true)?;
            Ok(())
        }
    };

    // Read direct files so we can determine dependencies.
    if let Some(files) = &mut profile.files {
        if let Some(user) = &mut files.user {
            if let Some(x) = user.remove(&FileMode::Executable) {
                for file in x {
                    handle_localize(&file, true)?;
                }
            }
        }
        if let Some(sys) = &mut files.system {
            if let Some(x) = sys.remove(&FileMode::Executable) {
                for file in x {
                    handle_localize(&file, false)?;
                }
            }
        }
        if let Some(direct) = &mut files.direct {
            if let Some(x) = direct.remove(&FileMode::Executable) {
                x.iter().try_for_each(|(file, _)| {
                    let path = direct_path(file);
                    handle_localize(&path.to_string_lossy(), false)
                })?;
            }
        }
    }

    // Parse the binaries
    binaries
        .iter()
        .try_for_each(|binary| handle_localize(binary, false))?;

    let parsed = Arc::try_unwrap(parsed).unwrap();

    // ELF files need to be processed by the library fabricator,
    // to use LDD on depends.
    // However, if the ELF is contained in  /usr/lib, we
    // want its parent directory, such as /usr/lib/chromium.
    parsed.elf.into_iter().try_for_each(|elf| -> Result<()> {
        if !elf.contains("/lib/") && !elf.contains("/lib64/") {
            handle.args_i(["--ro-bind", &elf, &elf])?;
        }
        elf_binaries.insert(elf.to_string());
        Ok(())
    })?;

    // Scripts are consumed here, and are only bound to the sandbox.
    parsed
        .scripts
        .into_par_iter()
        .try_for_each(|script| handle.args_i(["--ro-bind", &script, &script]))?;

    // Files are treated similarly to ELF file in terms of being
    // heuristics for library folders, but are not LDD searched
    // because they are not ELF binaries.
    parsed
        .files
        .into_par_iter()
        .try_for_each(|file| handle.args_i(["--ro-bind", &file, &file]))?;

    parsed
        .symlinks
        .into_par_iter()
        .try_for_each(|(link, dest)| {
            if !link.contains("/lib") {
                handle.args_i(["--symlink", &dest, &link])
            } else {
                Ok(())
            }
        })?;

    profile
        .libraries
        .get_or_insert_default()
        .extend(parsed.directories);

    if profile.home.is_some() {
        user::set(user::Mode::Real)?;
        let home_dir = Path::new(DATA_HOME.as_path()).join("antimony").join(name);
        std::fs::create_dir_all(&home_dir)?;
        user::revert()?;

        debug!("Finding home binaries");
        let home_str = home_dir.to_string_lossy();
        elf_binaries.extend(get_dir(&home_str)?);
    }

    #[rustfmt::skip]
    handle.args_i([
        "--symlink", "/usr/bin", "/bin",
        "--symlink", "/usr/sbin", "/sbin"

    ])?;
    profile.binaries = Some(elf_binaries);
    Ok(())
}
