pub mod bin;
pub mod dev;
pub mod etc;
pub mod features;
pub mod files;
pub mod lib;
pub mod ns;

use crate::{
    fab::bin::ELF_MAGIC,
    shared::{
        Set,
        db::{self, Database, Table},
        env::{AT_HOME, HOME},
        profile::Profile,
    },
    timer,
};
use anyhow::Result;
use log::{debug, trace, warn};
use parking_lot::Mutex;
use rayon::prelude::*;
use serde::{Serialize, de::DeserializeOwned};
use spawn::{Spawner, StreamMode};
use std::{
    borrow::Cow,
    env,
    fs::File,
    io::Read,
    path::Path,
    sync::{LazyLock, OnceLock},
};
use temp::Temp;
use user::as_real;

type Cache = Vec<String>;

/// A lock for initializing LIB_ROOTS
static LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

// Get the library roots.
pub static LIB_ROOTS: OnceLock<Set<String>> = OnceLock::new();

/// Whether we have a split-root.
pub static SINGLE_LIB: LazyLock<bool> = LazyLock::new(|| {
    let single = std::fs::read_link("/usr/lib64").is_ok();
    debug!("Single Library Folder: {single}");
    single
});

pub struct FabInfo<'a> {
    pub profile: &'a Mutex<Profile>,
    pub handle: &'a Spawner,
    pub name: &'a str,
    pub instance: &'a Temp,
    pub sys_dir: &'a Path,
}

/// Get cached definitions.
#[inline]
fn get_cache<T: DeserializeOwned>(name: &str, tb: Table) -> Result<Option<T>> {
    if let Some(bytes) = db::dump::<Vec<u8>>(name, Database::Cache, tb)? {
        trace!("Read {} from cache ({name} from {tb})", bytes.len());
        Ok(Some(postcard::from_bytes(&bytes)?))
    } else {
        trace!("No cache for {name}");
        Ok(None)
    }
}

/// Write the cache file.
#[inline]
fn write_cache<T: Serialize>(name: &str, content: &T, tb: Table) -> Result<()> {
    let bytes = postcard::to_stdvec(content)?;
    trace!("Writing {} bytes to cache ({name} to {tb})", bytes.len());
    if let Err(e) = db::store_bytes(name, &bytes, Database::Cache, tb) {
        warn!("Couldn't write to system cache: {e}");
    }
    Ok(())
}

/// Filter non-elf files.
fn elf_filter(path: &str) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut magic = [0u8; 5];
    file.read_exact(&mut magic).ok()?;
    if magic != ELF_MAGIC {
        return None;
    }
    Some(path.to_string())
}

/// Get all executable files in a directory.
pub fn get_dir(dir: &str) -> Result<Cache> {
    if let Some(libraries) = get_cache(dir, Table::Directories)? {
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

    write_cache(dir, &libraries, Table::Directories)?;

    Ok(libraries)
}

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

    if let Some(libraries) = get_cache(pattern, Table::Wildcards)? {
        return Ok(libraries);
    }

    let libraries = if pattern.starts_with("/") {
        let i = pattern.rfind('/').unwrap();
        run(&pattern[..i], &pattern[i + 1..])?
    } else if lib {
        let mut libraries = Cache::default();
        for root in LIB_ROOTS.get().unwrap() {
            debug!("Checking {root}");
            libraries.extend(run(root, pattern)?);
        }

        libraries
    } else {
        run("/usr/bin", pattern)?
    };

    write_cache(pattern, &libraries, Table::Wildcards)?;
    Ok(libraries)
}

/// LDD a path.
pub fn get_libraries(path: Cow<'_, str>) -> Result<Cache> {
    let libraries = if let Some(libraries) = get_cache(&path, Table::Libraries)? {
        libraries
    } else {
        let libraries: Cache = Spawner::abs("/usr/bin/ldd")
            .arg(path.as_ref())?
            .output(StreamMode::Pipe)
            .spawn()?
            .output_all()?
            .lines()
            .par_bridge()
            .filter_map(|e| {
                if let Some(start) = e.find("=> /")
                    && let Some(end) = e.rfind(' ')
                {
                    Some(String::from(&e[start + 3..end]))
                } else if let Some(start) = e.find("/")
                    && let Some(end) = e.rfind(' ')
                {
                    let path = String::from(&e[start..end]);
                    if path.contains(" ") { None } else { Some(path) }
                } else {
                    None
                }
            })
            .map(|e| {
                if e.contains("..")
                    && let Ok(path) = std::fs::canonicalize(&e)
                {
                    path.to_string_lossy().into_owned()
                } else if !e.starts_with("/usr") {
                    format!("/usr{e}")
                } else {
                    e
                }
            })
            .collect();

        write_cache(&path, &libraries, Table::Libraries)?;
        libraries
    };

    if LIB_ROOTS.get().is_none() {
        let _lock = LOCK.lock();
        timer!("::lib_roots", {
            let mut roots: Set<String> = libraries
                .iter()
                .filter_map(|lib| Path::new(&lib).parent().map(|p| p.to_owned()))
                .map(|path| path.to_string_lossy().into_owned())
                .filter(|e| {
                    if *SINGLE_LIB {
                        !e.contains("lib64")
                    } else {
                        true
                    }
                })
                .collect();
            roots.insert(String::from("/usr/lib"));
            if !*SINGLE_LIB {
                roots.insert(String::from("/usr/lib64"));
            }
            let _ = LIB_ROOTS.set(roots);
        })
    }
    Ok(libraries)
}

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
