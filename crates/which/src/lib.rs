use dashmap::DashMap;
use rayon::prelude::*;
use std::{
    borrow::Cow,
    env,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
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

static CACHE: LazyLock<DashMap<String, Arc<str>, ahash::RandomState>> =
    LazyLock::new(DashMap::default);

pub fn which<'a>(path: impl Into<Cow<'a, str>>) -> Result<&'static str, Error> {
    let path = path.into();

    // Insert if missing
    if !CACHE.contains_key(path.as_ref()) {
        let resolved = if Path::new(path.as_ref()).exists() {
            path.to_string()
        } else {
            PATH.par_iter()
                .find_map_any(|root: &PathBuf| {
                    let candidate = root.join(path.as_ref());
                    if candidate.exists() {
                        Some(candidate.to_string_lossy().into_owned())
                    } else {
                        None
                    }
                })
                .ok_or(Error::NotFound(path.clone().into_owned()))?
        };
        CACHE.insert(path.to_string(), Arc::from(resolved));
    }

    // Get the arc
    let arc = CACHE.get(path.as_ref()).unwrap().value().clone();

    // Consume it to get the data.
    let raw_ptr: *const str = Arc::into_raw(arc);

    // Cast it to static--the content will stay alive safely.
    let static_ref: &'static str = unsafe { &*raw_ptr };
    Ok(static_ref)
}
