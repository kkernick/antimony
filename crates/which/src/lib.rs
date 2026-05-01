#![doc = include_str!("../README.md")]

use common::cache::{self, CacheStatic};
use core::fmt;
use dashmap::DashMap;
use rayon::prelude::*;
use std::{
    borrow::Cow,
    env, error,
    path::{Path, PathBuf},
    sync::LazyLock,
};

/// Errors when trying to resolve a path.
#[derive(Debug)]
pub enum Error {
    /// If path couldn't be found.
    NotFound(String),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(e) => write!(f, "Could not find {e} in path"),
        }
    }
}
impl error::Error for Error {}

/// The User's PATH variable, removing ~/.local/bin to prevent
/// Antimony from using itself when a profile has been integrated.
pub static PATH: LazyLock<Vec<PathBuf>> = LazyLock::new(|| {
    let path = env::var("PATH").unwrap_or_else(|_| "/usr/bin".to_owned());
    path.split(':')
        .filter(|e| !e.contains("/.local/bin"))
        .map(PathBuf::from)
        .filter(|root| root.exists())
        .collect::<Vec<_>>()
});

/// The cache store.
static CACHE: CacheStatic<String, Cow<'static, str>> = LazyLock::new(DashMap::default);

/// The underlying cache, storing path -> resolved path lookups.
static WHICH: LazyLock<cache::Cache<String, Cow<'static, str>>> =
    LazyLock::new(|| cache::Cache::new(&CACHE));

/// Resolve the provided path in the environment's PATH variable.
///
/// Note that this implementation will return a path as-is if it exists,
/// which means that if binary exists in the current folder, it will
/// be resolved to that. It will also just return absolute paths as-is,
/// even if they aren't executable.
///
/// ## Errors
/// `Error::NotFound`: If the path could not be found.
pub fn which(path: &str) -> Result<&'static str, Error> {
    if let Some(resolved) = WHICH.get(path) {
        Ok(resolved)
    } else {
        let resolved = if Path::new(path).exists() {
            path.to_owned()
        } else {
            PATH.par_iter()
                .find_map_any(|root: &PathBuf| {
                    let candidate = root.join(path);
                    if candidate.exists() {
                        Some(candidate.to_string_lossy().into_owned())
                    } else {
                        None
                    }
                })
                .ok_or_else(|| Error::NotFound(path.to_owned()))?
        };
        Ok(WHICH.insert(path.to_owned(), Cow::Owned(resolved)))
    }
}
