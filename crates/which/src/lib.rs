use common::cache::{self, CacheStatic};
use dashmap::DashMap;
use rayon::prelude::*;
use std::{
    borrow::Cow,
    env,
    path::{Path, PathBuf},
    sync::LazyLock,
};

#[derive(Debug)]
pub enum Error {
    NotFound(String),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(e) => write!(f, "Could not find {e} in path"),
        }
    }
}
impl std::error::Error for Error {}

/// The User's PATH variable, removing ~/.local/bin to prevent
/// Antimony from using itself when a profile has been integrated.
pub static PATH: LazyLock<Vec<PathBuf>> = LazyLock::new(|| {
    let path = env::var("PATH").unwrap_or("/usr/bin".to_string());
    path.split(':')
        .filter(|e| !e.contains("/.local/bin"))
        .map(PathBuf::from)
        .filter(|root| root.exists())
        .collect::<Vec<_>>()
});

static CACHE: CacheStatic<String, Cow<'static, str>> = LazyLock::new(DashMap::default);

pub static WHICH: LazyLock<cache::Cache<String, Cow<'static, str>>> =
    LazyLock::new(|| cache::Cache::new(&CACHE));

pub fn which(path: &str) -> Result<&'static str, Error> {
    match WHICH.get(path) {
        Some(resolved) => Ok(resolved),
        None => {
            let resolved = if Path::new(path).exists() {
                path.to_string()
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
                    .ok_or(Error::NotFound(path.to_string()))?
            };
            Ok(WHICH.insert(path.to_string(), Cow::Owned(resolved)))
        }
    }
}
