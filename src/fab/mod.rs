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
    env::{AT_HOME, CONFIG_HOME, DATA_HOME, HOME},
    profile::Profile,
    store::{CACHE_STORE, Object},
};
use anyhow::Result;
use log::debug;
use parking_lot::Mutex;
use rayon::prelude::*;
use serde::{Serialize, de::DeserializeOwned};
use spawn::{Spawner, StreamMode};
use std::{
    borrow::Cow,
    collections::HashSet,
    env,
    fs::{self, File},
    io::Read,
    path::Path,
    sync::LazyLock,
};
use temp::Temp;
use user::as_real;

type Cache = HashSet<String, ahash::RandomState>;

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

/// Each fabricator is passed this structure.
pub struct FabInfo<'a> {
    pub profile: &'a Mutex<Profile>,
    pub handle: &'a Spawner,
    pub name: &'a str,
    pub instance: &'a Temp,
    pub sys_dir: &'a Path,
}

/// Get cached definitions.
#[inline]
fn get_cache<T: DeserializeOwned>(name: &str, object: Object) -> Result<Option<T>> {
    if let Ok(bytes) = CACHE_STORE.with_borrow(|s| s.bytes(&name.replace("/", "-"), object)) {
        Ok(Some(postcard::from_bytes(&bytes)?))
    } else {
        Ok(None)
    }
}

/// Write the cache file.
#[inline]
fn write_cache<T: Serialize>(name: &str, content: &T, object: Object) -> Result<()> {
    let bytes = postcard::to_stdvec(content)?;
    CACHE_STORE.with_borrow(|s| s.dump(&name.replace("/", "-"), object, &bytes))?;
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
///
/// ## Examples
///
/// ```rust
/// antimony::fab::get_dir("/usr/lib").expect("Failed to search lib");
/// ```
pub fn get_dir(dir: &str) -> Result<Cache> {
    if let Ok(Some(libraries)) = get_cache(dir, Object::Directories) {
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

    write_cache(dir, &libraries, Object::Directories)?;
    Ok(libraries)
}

/// Find all matches in a directory. We only match the top level for performance considerations.
///
/// ## Examples
///
/// ```rust
/// antimony::fab::get_wildcards("glib*", true).expect("Failed to find Glib");
/// ```
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

    if let Some(libraries) = get_cache(pattern, Object::Wildcards)? {
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

    write_cache(pattern, &libraries, Object::Wildcards)?;
    Ok(libraries)
}

/// LDD a path.
///
/// ## Examples
///
/// ```rust
/// use std::borrow::Cow;
/// use antimony::fab::get_libraries;
///
/// get_libraries(Cow::Borrowed("/proc/self/exe")).expect("Failed to LDD self");
/// ```
pub fn get_libraries(path: Cow<'_, str>) -> Result<Cache> {
    let libraries = if let Ok(Some(libraries)) = get_cache(&path, Object::Libraries) {
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

        write_cache(&path, &libraries, Object::Libraries)?;
        libraries
    };

    Ok(libraries)
}

/// Resolve environment variables within paths.
///
/// ## Examples
///
/// ```rust
/// use antimony::fab::resolve;
/// use std::borrow::Cow;
///
/// unsafe {
///     std::env::set_var("HOME", "/home/test");
///     std::env::set_var("UID", "1000");
///     std::env::set_var("PID", "1");
///     std::env::set_var("FD", "2");
/// }
///
/// assert!(resolve(Cow::Borrowed("$HOME/test")) == "/home/test/test");
/// assert!(resolve(Cow::Borrowed("/run/$UID/test")) == "/run/1000/test");
/// assert!(resolve(Cow::Borrowed("/proc/$PID/fd/$FD")) == "/proc/1/fd/2");
/// assert!(resolve(Cow::Borrowed("$HOME$UID$PID$FD")) == "/home/test100012");
/// assert!(resolve(Cow::Borrowed("$NOT_A_VAR")) == "$NOT_A_VAR");
/// assert!(resolve(Cow::Borrowed("$$$")) == "$$$");
/// ```
pub fn resolve(mut string: Cow<'_, str>) -> Cow<'_, str> {
    if string.starts_with('~') {
        string = Cow::Owned(string.replace("~", "/home/antimony"));
    }

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
                        "AT_HOME" => AT_HOME.display().to_string(),
                        "XDG_CONFIG_HOME" => CONFIG_HOME.display().to_string(),
                        "XDG_DATA_HOME" => DATA_HOME.display().to_string(),
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

/// Localize a home path to /home/antimony
///
/// ## Examples
///
/// ```rust
/// unsafe {std::env::set_var("HOME", "/home/test")}
/// assert!(antimony::fab::localize_home("/home/test/file") == "/home/antimony/file");
/// ```
#[inline]
pub fn localize_home<'a>(path: &'a str) -> Cow<'a, str> {
    if path.starts_with("/home") {
        Cow::Owned(path.replace(HOME.as_str(), "/home/antimony"))
    } else {
        Cow::Borrowed(path)
    }
}

/// Localize a path, returning the resolved source, and where it should be mounted in the sandbox.
/// This is a rather complicated function because it handles a lot of different cases. Specifically:
///
/// 1. Resolves environment variables ($AT_HOME => /usr/share/antimony)
/// 2. Resolves home markers => (~/test => $HOME/test)
/// 3. The destination is mapped to Antimony's home (/home/test/file => /home/antimony/file)
/// 4. When the home flag is set, paths can be relative to home (file => $HOME/file)
/// 5. An explicit mapping can be provided with an equal sign. ($AT_HOME/binary=/usr/bin/binary => /usr/bin/binary)
///
/// The return value is also rather complicated, and not entirely intuitive. The src aspect is optional,
/// but the destination is always returned. That's because the source is the important aspect (Its the
/// location on the host), so if it doesn't exist, the callers can just check src and drop the entire
/// thing otherwise.
///
/// ## Examples
///
/// ```rust
/// use antimony::fab::localize_path;
/// use std::borrow::Cow;
///
/// unsafe {
///     std::env::set_var("HOME", "/home/test");
///     std::env::set_var("UID", "1000");
///     std::env::set_var("PID", "1");
///     std::env::set_var("FD", "2");
/// }
///
/// let temp = temp::Builder::new().create::<temp::File>().unwrap();
/// let path = temp.full();
/// let str = path.to_string_lossy();
///
/// assert!(localize_path(str.clone().as_ref(), false).unwrap() == (Some(str.clone()), str.into_owned()));
///
/// assert!(localize_path("/etc/file", false).unwrap() == (None, "/etc/file".to_string()));
/// assert!(localize_path("/etc/file=/file", false).unwrap() == (None, "/file".to_string()));
///
/// assert!(localize_path("file", true).unwrap() == (None, "/home/antimony/file".to_string()));
/// assert!(localize_path("~/file", true).unwrap() == (None, "/home/antimony/file".to_string()));
/// assert!(localize_path("$HOME/file", true).unwrap() == (None, "/home/antimony/file".to_string()));
///
/// assert!(localize_path("/run/$UID/file", false).unwrap() == (None, "/run/1000/file".to_string()));
/// assert!(localize_path("/proc/$PID/fd/$FD=/init-err", false).unwrap() == (None, "/init-err".to_string()));
/// ```
pub fn localize_path(file: &str, home: bool) -> Result<(Option<Cow<'_, str>>, String), nix::Error> {
    let (source, dest) = if let Some((source, dest)) = file.split_once('=') {
        (resolve(Cow::Borrowed(source)), Cow::Borrowed(dest))
    } else {
        let mut resolved = resolve(Cow::Borrowed(file));
        if home
            && !resolved.starts_with("/home")
            && (!resolved.starts_with('/') || !as_real!({ Path::new(resolved.as_ref()).exists() })?)
        {
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
