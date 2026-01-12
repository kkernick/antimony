//! Shared functionality between the Fabricators. This contains the brunt of the library fabricator's logic.

pub mod bin;
pub mod dev;
pub mod etc;
pub mod features;
pub mod files;
pub mod lib;
pub mod ns;

use crate::shared::{
    Set,
    env::{AT_HOME, CACHE_DIR, HOME},
    profile::Profile,
};
use anyhow::Result;
use log::{debug, trace};
use parking_lot::Mutex;
use rayon::prelude::*;
use serde::{Serialize, de::DeserializeOwned};
use spawn::{Spawner, StreamMode};
use std::{
    borrow::Cow,
    env,
    fs::{self, File},
    io::Read,
    path::Path,
    sync::LazyLock,
};
use temp::Temp;
use user::{as_effective, as_real};

/// What the Cache type is. Through benchmarking, a regular Vec outperforms
/// both Sets and SmallVec.
type Cache = Vec<String>;

/// Get the library roots.
pub static LIB_ROOTS: LazyLock<Set<String>> = LazyLock::new(|| {
    let mut roots = Set::default();
    for known_root in ["/usr/lib", "/usr/lib64", "/usr/lib/x86_64-linux-gnu"] {
        if Path::new(known_root).exists() && fs::read_link(known_root).is_err() {
            roots.insert(known_root.to_string());
        }
    }
    roots
});

/// The magic for an ELF file.
pub static ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];

/// Each fabrciator is passed this structure.
pub struct FabInfo<'a> {
    pub profile: &'a Mutex<Profile>,
    pub handle: &'a Spawner,
    pub name: &'a str,
    pub instance: &'a Temp,
    pub sys_dir: &'a Path,
}

/// Get cached definitions.
#[inline]
fn get_cache<T: DeserializeOwned>(name: &str, cache: &str) -> Result<Option<T>> {
    if let Ok(bytes) = fs::read(CACHE_DIR.join(cache).join(name.replace("/", "-"))) {
        Ok(Some(postcard::from_bytes(&bytes)?))
    } else {
        Ok(None)
    }
}

/// Write the cache file.
#[inline]
fn write_cache<T: Serialize>(name: &str, content: &T, cache: &str) -> Result<()> {
    let bytes = postcard::to_stdvec(content)?;
    let cache = CACHE_DIR.join(cache);
    if !cache.exists() {
        as_effective!(fs::create_dir_all(&cache))??;
    }
    let cache = cache.join(name.replace("/", "-"));
    trace!("Writing {}", cache.display());
    as_effective!(fs::write(cache, &bytes))??;
    Ok(())
}

/// Filter non-elf files.
#[inline]
fn elf_filter(path: &str) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).ok()?;
    if magic != ELF_MAGIC {
        return None;
    }
    Some(path.to_string())
}

/// Get all executable files in a directory. This is very expensive.
pub fn get_dir(dir: &str) -> Result<Cache> {
    if let Ok(Some(libraries)) = get_cache(dir, "directories") {
        return Ok(libraries);
    }
    let libraries: Cache = Spawner::abs("/usr/bin/find")
        .args([dir, "-executable", "-type", "f"])?
        .output(StreamMode::Pipe)
        .mode(user::Mode::Real)
        .spawn()?
        .output_all()?
        .lines()
        .par_bridge()
        .filter_map(elf_filter)
        .collect();
    write_cache(dir, &libraries, "directories")?;
    Ok(libraries)
}

/// Find all matches in a directory. We only match the top level for performance considerations.
pub fn get_wildcards(pattern: &str, lib: bool) -> Result<Cache> {
    let run = |dir: &str, base: &str| -> Result<Cache> {
        Ok(Spawner::abs("/usr/bin/find")
            .args([dir, "-maxdepth", "1", "-mindepth", "1", "-name", base])?
            .output(StreamMode::Pipe)
            .mode(user::Mode::Real)
            .spawn()?
            .output_all()?
            .lines()
            .map(|e| e.to_string())
            .collect())
    };

    if let Some(libraries) = get_cache(pattern, "wildcards")? {
        return Ok(libraries);
    }

    // If we have a direct path, call `find /path/to/parent -name file*`
    let libraries = if pattern.starts_with("/") {
        let i = pattern.rfind('/').unwrap();
        run(&pattern[..i], &pattern[i + 1..])?

    // If we're looking for libraries, check each library root.
    } else if lib {
        let mut libraries = Cache::default();
        LIB_ROOTS.iter().for_each(|root| {
            if let Ok(lib) = run(root, pattern) {
                libraries.extend(lib);
            }
        });

        libraries
    // Otherwise, we assume we're looking for binaries.
    } else {
        run("/usr/bin", pattern)?
    };

    write_cache(pattern, &libraries, "wildcards")?;
    Ok(libraries)
}

/// LDD a path.
pub fn get_libraries(path: Cow<'_, str>) -> Result<Cache> {
    let libraries = if let Ok(Some(libraries)) = get_cache(&path, "libraries") {
        libraries
    } else {
        let libraries: Cache = Spawner::abs("/usr/bin/ldd")
            .arg(path.as_ref())?
            .output(StreamMode::Pipe)
            .mode(user::Mode::Real)
            .spawn()?
            .output_all()?
            .lines()
            .par_bridge()
            .filter_map(|e| {
                if let Some(start) = e.find("/")
                    && let Some(end) = e[start..].find(' ')
                {
                    Some(String::from(&e[start..start + end]))
                } else {
                    None
                }
            })
            .map(|e| {
                let path = Path::new(&e);
                if let Some(parent) = path.parent()
                    && let Ok(canon) = fs::canonicalize(parent)
                {
                    let canon = canon.join(path.file_name().unwrap());
                    canon.to_string_lossy().into_owned()
                } else {
                    e
                }
            })
            .map(|e| {
                let formatted = format!("/usr{e}");
                if Path::new(&formatted).exists() {
                    formatted
                } else {
                    e
                }
            })
            .collect();
        write_cache(&path, &libraries, "libraries")?;
        libraries
    };

    trace!("{path} => {libraries:?}");
    Ok(libraries)
}

/// Resolve environment variables within names.
pub fn resolve_env(string: Cow<'_, str>) -> Cow<'_, str> {
    if string.contains('$') {
        let mut resolved = String::new();
        let mut chars = string.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '$' {
                let mut var_name = String::new();
                while let Some(&next) = chars.peek() {
                    if next.is_ascii_uppercase() || next.is_ascii_digit() || next == '_' {
                        var_name.push(next);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if !var_name.is_empty() {
                    // These values may not be defined in the environment, but have explicit
                    // values Antimony can fill.
                    let val = match var_name.as_str() {
                        "UID" => format!("{}", user::USER.real),
                        "AT_HOME" => format!("{}", AT_HOME.display()),
                        name => env::var(name).unwrap_or_else(|_| format!("${name}")),
                    };
                    resolved.push_str(&val);
                } else {
                    resolved.push('$');
                }
            } else {
                resolved.push(ch)
            }
        }
        Cow::Owned(resolved)
    } else {
        string
    }
}

/// Resolve environment variables in paths.
#[inline]
pub fn resolve(mut path: Cow<'_, str>) -> Cow<'_, str> {
    if path.starts_with('~') {
        path = Cow::Owned(path.replace("~", "/home/antimony"));
    }
    resolve_env(path)
}

/// Localize a home path to /home/antimony
#[inline]
pub fn localize_home<'a>(path: &'a str) -> Cow<'a, str> {
    if path.starts_with("/home") {
        Cow::Owned(path.replace(HOME.as_str(), "/home/antimony"))
    } else {
        Cow::Borrowed(path)
    }
}

/// Ensure ~ points to /home/antimony
pub fn localize_path(file: &str, home: bool) -> Result<(Option<Cow<'_, str>>, String), nix::Error> {
    let (source, dest) = if let Some((source, dest)) = file.split_once('=') {
        (resolve(Cow::Borrowed(source)), Cow::Borrowed(dest))
    } else {
        let mut resolved = resolve(Cow::Borrowed(file));
        if home && !resolved.starts_with("/home") {
            resolved = Cow::Owned(format!("{}/{resolved}", HOME.as_str()));
        }
        (resolved.clone(), resolved)
    };
    let dest = localize_home(&dest);

    Ok(if as_real!({ Path::new(source.as_ref()).exists() })? {
        debug!("{source} => {dest}");
        (Some(source), dest.into_owned())
    } else {
        debug!("{source} (does not exist) => {dest}");
        (None, dest.into_owned())
    })
}
