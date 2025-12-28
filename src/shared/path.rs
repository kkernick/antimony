//! Tools and definitions related to paths.
use crate::shared::env::{CACHE_DIR, RUNTIME_DIR};
use std::path::PathBuf;

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
