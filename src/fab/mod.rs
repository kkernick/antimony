//! Shared functionality between the Fabricators. This contains the brunt of the library fabricator's logic.
#![allow(clippy::missing_errors_doc)]

pub mod bin;
pub mod dev;
pub mod files;
pub mod lib;
pub mod ns;

use crate::{
    fab::lib::ROOTS,
    shared::{
        Set, ThreadMap,
        env::{AT_HOME, CONFIG_HOME, DATA_HOME, HOME},
        package::Package,
        profile::Profile,
        store::{CACHE_STORE, Object},
    },
    timer,
};
use anyhow::Result;
use bilrost::{Message, OwnedMessage};
use common::cache::{self, CacheStatic};
use dashmap::DashMap;
use heck::ToTitleCase;
use log::debug;
use path_clean::clean;
use rayon::prelude::*;
use spawn::{Spawner, StreamMode};
use std::{
    borrow::Cow,
    env,
    fmt::Debug,
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    sync::LazyLock,
};
use temp::Temp;

/// ELF constants.
pub static ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
pub static E_TYPE_OFFSET: usize = 16;

/// Each fabricator is passed this structure.
pub struct FabInfo<'a> {
    pub profile: &'a mut Profile,
    pub handle: &'a Spawner,
    pub name: &'a str,
    pub instance: &'a Temp,
    pub sys_dir: &'a Path,
    pub package: &'a mut Option<(Package, bool)>,
}

/// `bilrost` compatible struct for saving cached data
#[derive(Message, Debug)]
pub struct Cache {
    /// The data
    pub cache: Set<String>,
}

/// The L1 Cache caches the disk cache for repeated look-ups in memory.
static _L1: CacheStatic<Object, ThreadMap<String, Vec<u8>>> = LazyLock::new(DashMap::default);

/// The L1 Cache for get/write cache.
static L1: LazyLock<cache::Cache<Object, ThreadMap<String, Vec<u8>>>> =
    LazyLock::new(|| cache::Cache::new(&_L1));

/// Discover application folders across the filesystem.
#[must_use]
pub fn find_folders(name: &str) -> Set<String> {
    // Each is sent to the library fabricator, in case they contain anything,
    // and are then mounted directly.
    [
        Path::new("/etc"),
        Path::new("/usr/share"),
        Path::new("/opt"),
    ]
    .into_iter()
    .flat_map(|path| {
        [
            Cow::Borrowed(name),
            Cow::Owned(name.to_title_case()),
            Cow::Owned(name.to_lowercase()),
        ]
        .into_iter()
        .filter_map(|name| {
            let path = path.join(name.as_ref());
            if path.exists() {
                Some(path.to_string_lossy().into_owned())
            } else {
                None
            }
        })
    })
    .collect()
}

#[inline]
#[must_use]
pub fn hash(i: &str) -> String {
    format!("{}", ahash::RandomState::with_seeds(0, 0, 0, 0).hash_one(i))
}

/// Get cached definitions.
#[inline]
pub fn get_cache<T: OwnedMessage + Debug>(name: &str, object: Object) -> Result<Option<T>> {
    timer!("::get_cache", {
        if let Some(map) = L1.get(&object)
            && let Some(bytes) = map.get(name)
        {
            let content = T::decode(bytes.as_slice())?;
            Ok(Some(content))
        } else if let Ok(bytes) = CACHE_STORE.borrow().bytes(&hash(name), object) {
            let content = T::decode(bytes.as_slice())?;
            L1.insert(object, ThreadMap::default())
                .insert(name.to_owned(), bytes);
            Ok(Some(content))
        } else {
            Ok(None)
        }
    })
}

/// Write the cache file.
#[inline]
pub fn write_cache<T: Message + Debug>(name: &str, content: T, object: Object) -> Result<T> {
    timer!("::write_cache", {
        let bytes = content.encode_to_bytes();
        CACHE_STORE.borrow().dump(&hash(name), object, &bytes)?;
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
#[allow(
    clippy::arithmetic_side_effects,
    reason = "The index is static, and will always fall into the allocated buffer"
)]
fn elf_filter(path: &str) -> io::Result<bool> {
    let mut file = File::open(path)?;
    let mut buf = [0u8; 18];
    file.read_exact(&mut buf)?;

    // Check magic number
    if !buf.starts_with(&ELF_MAGIC) {
        return Ok(false);
    }

    // Read e_type at offset 16 (little-endian ELF standard position)
    let e_type = u16::from_le_bytes([buf[E_TYPE_OFFSET], buf[E_TYPE_OFFSET + 1]]);
    Ok(e_type == 2 || e_type == 3)
}

/// LDD an executable to get its library dependencies.
///
/// ```rust
/// antimony::fab::ldd("/usr/bin/bash").expect("Failed to LDD bash");
/// ```
///
#[inline]
pub fn ldd(path: &str) -> Result<Set<String>> {
    timer!(
        "::ldd",
        if fs::read_link(path).is_ok() {
            Ok(Set::default())
        } else {
            let depends = if elf_filter(path)? {
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
                            && let Some(name) = path.file_name()
                        {
                            let mut path = clean(parent).join(name);
                            if !ROOTS.par_iter().any(|root| path.starts_with(root.as_ref())) {
                                let usr_path = format!("/usr{}", path.to_string_lossy());
                                if Path::new(&usr_path).exists() && in_lib(&usr_path) {
                                    path = PathBuf::from(usr_path);
                                } else {
                                    path = path.canonicalize().unwrap_or(path);
                                }
                            }
                            path.to_string_lossy().into_owned()
                        } else {
                            e
                        }
                    })
                    .collect()
            } else {
                Set::default()
            };
            Ok(depends)
        }
    )
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
pub fn get_libraries(path: &str) -> Result<Set<String>> {
    timer!("::get_libraries", {
        let libraries = if let Ok(Some(libraries)) = get_cache::<Cache>(path, Object::Libraries) {
            libraries.cache
        } else {
            write_cache(path, Cache { cache: ldd(path)? }, Object::Libraries)?.cache
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
/// std::fs::File::create("test.txt");
/// let current_dir = std::env::current_dir().unwrap();
///
/// assert!(resolve(Cow::Borrowed("$HOME/test")) == "/home/test/test");
/// assert!(resolve(Cow::Borrowed("/run/$UID/test")) == "/run/1000/test");
/// assert!(resolve(Cow::Borrowed("/proc/$PID/fd/$FD")) == "/proc/1/fd/2");
/// assert!(resolve(Cow::Borrowed("$HOME$UID$PID$FD")) == "/home/test100012");
/// assert!(resolve(Cow::Borrowed("$NOT_A_VAR")) == "$NOT_A_VAR");
/// assert!(resolve(Cow::Borrowed("$$$")) == "$$$");
/// assert!(resolve(Cow::Borrowed("test.txt")) == current_dir.join("test.txt").to_string_lossy());
///
/// std::fs::remove_file("test.txt");
/// ```
pub fn resolve(mut string: Cow<'_, str>) -> Cow<'_, str> {
    timer!("::resolve", {
        if string.starts_with('~') {
            string = Cow::Owned(string.replacen('~', "/home/antimony", 1));
        }

        let mut resolved = if string.contains('$') {
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
        };

        let path = Path::new(resolved.as_ref());
        if !path.is_absolute() || !path.exists() {
            let mut path = clean(path);
            if (!path.is_absolute() || !path.exists())
                && let Ok(canon) = fs::canonicalize(&path)
            {
                path = canon;
            }

            resolved = Cow::Owned(path.to_string_lossy().into_owned());
        }

        resolved
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
            Cow::Owned(path.replacen(HOME.as_str(), "/home/antimony", 1))
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
                && (!resolved.starts_with('/') || !Path::new(resolved.as_ref()).exists())
            {
                resolved = Cow::Owned(format!("{}/{resolved}", HOME.as_str()));
            }
            (resolved.clone(), resolved)
        };
        let dest = localize_home(&dest);

        Ok(if Path::new(source.as_ref()).exists() {
            debug!("{source} => {dest}");
            (Some(source), dest.into_owned())
        } else {
            debug!("{source} (does not exist) => {dest}");
            (None, dest.into_owned())
        })
    })
}
