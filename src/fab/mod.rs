#![allow(clippy::missing_errors_doc)]
//! Shared functionality between the Fabricators. This contains the brunt of the library fabricator's logic.

pub mod bin;
pub mod dev;
pub mod features;
pub mod files;
pub mod lib;
pub mod ns;

use crate::{
    fab::lib::{ROOTS, WildcardFilter},
    shared::{
        Set,
        env::{AT_HOME, CONFIG_HOME, DATA_HOME, HOME},
        profile::Profile,
        store::{CACHE_STORE, Object},
    },
    timer,
};
use anyhow::Result;
use bilrost::{Message, OwnedMessage};
use common::cache::{self, CacheStatic};
use dashmap::DashMap;
use log::debug;
use rayon::prelude::*;
use spawn::{Spawner, StreamMode};
use std::{
    borrow::Cow,
    env,
    fmt::Debug,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::LazyLock,
};
use temp::Temp;
use user::as_real;

/// The magic for an ELF file.
pub static ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];

/// Each fabricator is passed this structure.
pub struct FabInfo<'a> {
    pub profile: &'a mut Profile,
    pub handle: &'a Spawner,
    pub name: &'a str,
    pub instance: &'a Temp,
    pub sys_dir: &'a Path,
}

/// `bilrost` compatible struct for saving cached data
#[derive(Message, Debug)]
struct Cache {
    /// The data
    cache: Set<Cow<'static, str>>,
}

/// Get cached definitions.
#[inline]
fn get_cache<T: OwnedMessage + Debug>(name: &str, object: Object) -> Result<Option<T>> {
    timer!("::get_cache", {
        if let Ok(bytes) = CACHE_STORE.borrow().bytes(&name.replace('/', "-"), object) {
            let content = T::decode(bytes.as_slice())?;
            Ok(Some(content))
        } else {
            Ok(None)
        }
    })
}

/// Write the cache file.
#[inline]
fn write_cache<T: Message + Debug>(name: &str, content: T, object: Object) -> Result<T> {
    timer!("::write_cache", {
        let bytes = content.encode_to_bytes();
        CACHE_STORE
            .borrow()
            .dump(&name.replace('/', "-"), object, &bytes)?;
        Ok(content)
    })
}

#[inline]
pub fn in_lib(path: &str) -> bool {
    timer!("::in_lib", {
        ROOTS.par_iter().any(|r| path.starts_with(r.as_ref()))
    })
}

/// Filter non-elf files.
#[inline]
fn elf_filter(path: &str) -> bool {
    timer!("::elf_filter", {
        if let Ok(mut file) = File::open(path) {
            let mut magic = [0u8; 4];
            if file.read_exact(&mut magic).is_ok() && magic == ELF_MAGIC {
                return true;
            }
        }
        false
    })
}

/// Cache LDD calls ephemerally.
static CACHE: CacheStatic<String, Set<String>> = LazyLock::new(DashMap::default);

/// The underlying cache, storing path -> resolved path lookups.
static LDD: LazyLock<cache::Cache<String, Set<String>>> =
    LazyLock::new(|| cache::Cache::new(&CACHE));

#[inline]
/// LDD an executable to get its library dependencies.
///
/// ```rust
/// antimony::fab::ldd("/usr/bin/bash").expect("Failed to LDD bash");
/// ```
///
pub fn ldd(path: &str) -> Result<&'static Set<String>> {
    if let Some(depends) = LDD.get(path) {
        Ok(depends)
    } else {
        let depends = if elf_filter(path) {
            Spawner::abs("/usr/bin/ldd")
                .arg(path)
                .output(StreamMode::Pipe)
                .error(StreamMode::Discard)
                .mode(user::Mode::Real)
                .spawn()?
                .output_all()?
                .lines()
                .filter_map(|e| {
                    if let Some(start) = e.find('/')
                        && let Some(end) = e[start..].find(' ')
                    {
                        Some(String::from(
                            &e[start..start.checked_add(end).unwrap_or(end)],
                        ))
                    } else {
                        None
                    }
                })
                .map(|e| {
                    let path = Path::new(&e);
                    if let Some(parent) = path.parent()
                        && let Ok(canon) = fs::canonicalize(parent)
                        && let Some(name) = path.file_name()
                    {
                        canon.join(name).to_string_lossy().into_owned()
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
                .collect()
        } else {
            Set::default()
        };
        Ok(LDD.insert(path.to_owned(), depends))
    }
}

/// Get all executable files in a directory. This is very expensive.
///
/// ## Examples
///
/// ```rust
/// antimony::fab::get_dir("/usr/lib").expect("Failed to search lib");
/// ```
pub fn get_dir(dir: &str) -> Result<Set<Cow<'static, str>>> {
    timer!("::get_dir", {
        if let Ok(Some(libraries)) = get_cache::<Cache>(dir, Object::Directories) {
            return Ok(libraries.cache.into_iter().collect());
        }

        let libraries: Set<Cow<'static, str>> = Spawner::abs("/usr/bin/find")
            .args([dir, "-executable", "-type", "f"])
            .output(StreamMode::Pipe)
            .mode(user::Mode::Real)
            .spawn()?
            .output_all()?
            .lines()
            .par_bridge()
            .filter_map(|lib| ldd(lib).ok())
            .flatten()
            .map(|s| Cow::Borrowed(s.as_str()))
            .collect();

        Ok(write_cache(dir, Cache { cache: libraries }, Object::Directories)?.cache)
    })
}

/// Find all matches in a directory. We only match the top level for performance considerations.
///
/// ## Examples
///
/// ```rust
/// use antimony::fab::{get_wildcards,lib::WildcardFilter};
/// get_wildcards("glib*", true, WildcardFilter::Files).expect("Failed to find Glib");
/// ```
#[allow(
    clippy::unwrap_used,
    clippy::missing_panics_doc,
    reason = "Both unwraps are done with explicit knowledge that they cannot fail."
)]
pub fn get_wildcards(
    pattern: &str,
    lib: bool,
    filter: WildcardFilter,
) -> Result<Set<Cow<'static, str>>> {
    timer!("::get_wildcards", {
        let run = |dir: &str, base: &str| -> Result<Set<Cow<'static, str>>> {
            let handle = Spawner::abs("/usr/bin/find").arg(dir);

            if base.contains('/') {
                let whole = PathBuf::from(format!("{dir}/{base}"));
                if !whole.parent().unwrap().exists() {
                    return Ok(Set::default());
                }
                let d = format!("{}", whole.components().count().saturating_sub(1));
                handle.args_i(["-maxdepth", &d, "-wholename", &whole.display().to_string()]);
            } else {
                handle.args_i(["-maxdepth", "1", "-mindepth", "1", "-name", base]);
            }

            Ok(handle
                .args(["-type", filter.find_filter()])
                .output(StreamMode::Pipe)
                .mode(user::Mode::Real)
                .spawn()?
                .output_all()?
                .lines()
                .map(|e| Cow::Owned(e.to_owned()))
                .collect())
        };

        if let Some(libraries) = get_cache::<Cache>(pattern, Object::Wildcards)? {
            return Ok(libraries.cache);
        }

        // If we have a direct path, call `find /path/to/parent -name file*`
        let libraries = if pattern.starts_with('/') {
            let i = pattern.rfind('/').unwrap();
            run(&pattern[..i], &pattern[i.checked_add(1).unwrap_or(i)..])?

        // If we're looking for libraries, check each library root.
        } else if lib {
            let mut libraries = Set::default();
            for root in ROOTS.iter() {
                if let Ok(lib) = run(&root, pattern) {
                    libraries.extend(lib);
                }
            }

            libraries
        // Otherwise, we assume we're looking for binaries.
        } else {
            run("/usr/bin", pattern)?
        };

        Ok(write_cache(pattern, Cache { cache: libraries }, Object::Wildcards)?.cache)
    })
}

/// LDD a path.
///
/// ## Examples
///
/// ```rust
/// use std::borrow::Cow;
/// use antimony::fab::get_libraries;
///
/// get_libraries("/proc/self/exe").expect("Failed to LDD self");
/// ```
pub fn get_libraries(path: &str) -> Result<Set<Cow<'static, str>>> {
    timer!("::get_libraries", {
        let libraries = if let Ok(Some(libraries)) = get_cache::<Cache>(path, Object::Libraries) {
            libraries.cache
        } else {
            let libraries: Set<Cow<'static, str>> = ldd(path)?
                .iter()
                .map(|s| Cow::Borrowed(s.as_str()))
                .collect();
            write_cache(path, Cache { cache: libraries }, Object::Libraries)?.cache
        };
        Ok(libraries)
    })
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
    timer!("::resolve", {
        if string.starts_with('~') {
            string = Cow::Owned(string.replace('~', "/home/antimony"));
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
                    if var_name.is_empty() {
                        resolved.push('$');
                    } else {
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
                    }
                } else {
                    resolved.push(ch);
                }
            }
            Cow::Owned(resolved)
        } else {
            string
        }
    })
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
pub fn localize_home(path: &str) -> Cow<'_, str> {
    timer!("::localize_home", {
        if path.starts_with("/home") {
            Cow::Owned(path.replace(HOME.as_str(), "/home/antimony"))
        } else {
            Cow::Borrowed(path)
        }
    })
}

/// Localize a path, returning the resolved source, and where it should be mounted in the sandbox.
/// This is a rather complicated function because it handles a lot of different cases. Specifically:
///
/// 1. Resolves environment variables ($`AT_HOME` => /usr/share/antimony)
/// 2. Resolves home markers => (~/test => $HOME/test)
/// 3. The destination is mapped to Antimony's home (/home/test/file => /home/antimony/file)
/// 4. When the home flag is set, paths can be relative to home (file => $HOME/file)
/// 5. An explicit mapping can be provided with an equal sign. ($`AT_HOME/binary=/usr/bin/binary` => /usr/bin/binary)
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
/// assert!(localize_path("/etc/file", false).unwrap() == (None, "/etc/file".to_owned()));
/// assert!(localize_path("/etc/file=/file", false).unwrap() == (None, "/file".to_owned()));
///
/// assert!(localize_path("file", true).unwrap() == (None, "/home/antimony/file".to_owned()));
/// assert!(localize_path("~/file", true).unwrap() == (None, "/home/antimony/file".to_owned()));
/// assert!(localize_path("$HOME/file", true).unwrap() == (None, "/home/antimony/file".to_owned()));
///
/// assert!(localize_path("/run/$UID/file", false).unwrap() == (None, "/run/1000/file".to_owned()));
/// assert!(localize_path("/proc/$PID/fd/$FD=/init-err", false).unwrap() == (None, "/init-err".to_owned()));
/// ```
pub fn localize_path(
    file: &str,
    home: bool,
) -> Result<(Option<Cow<'_, str>>, String), user::Error> {
    timer!("::localize_path", {
        let (source, dest) = if let Some((source, dest)) = file.split_once('=') {
            (resolve(Cow::Borrowed(source)), Cow::Borrowed(dest))
        } else {
            let mut resolved = resolve(Cow::Borrowed(file));
            if home
                && !resolved.starts_with("/home")
                && (!resolved.starts_with('/')
                    || !as_real!({ Path::new(resolved.as_ref()).exists() })?)
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
    })
}
