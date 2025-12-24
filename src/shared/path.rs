//! Tools and definitions related to paths.
use crate::shared::env::{CACHE_DIR, PATH, PWD, RUNTIME_DIR};
use std::path::PathBuf;
use which::which_in;

/// Lookup a binary, excluding the path Antimony installs itself.
#[inline]
pub fn which_exclude(name: &str) -> Result<String, which::Error> {
    let path = which_in(name, Some(PATH.as_str()), PWD.as_path())?;
    Ok(path.to_string_lossy().into_owned())
}

/// The user dir is where the instance information is stored.
#[inline]
pub fn user_dir(instance: &str) -> PathBuf {
    PathBuf::from(RUNTIME_DIR.as_path())
        .join("antimony")
        .join(instance)
}

/// Get where direct files should be placed.
#[inline]
pub fn direct_path(file: &str) -> PathBuf {
    CACHE_DIR.join(".direct").join(&file[1..])
}
