//! Environment Variables Antimony needs defined.

use crate::shared::config::CONFIG_FILE;
use anyhow::Result;
use log::{debug, warn};
use nix::{
    libc::getpwuid,
    unistd::{AccessFlags, access},
};
use std::{
    env::{self, temp_dir},
    ffi::CString,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::LazyLock,
};
use user::{USER, as_effective};
use which::which;

/// Antimony's home folder is where configuration/caches are stored
pub static AT_HOME: LazyLock<PathBuf> = LazyLock::new(|| {
    let path =
        PathBuf::from(env::var("AT_HOME").unwrap_or_else(|_| "/usr/share/antimony".to_owned()));
    if !path.starts_with("/usr/") {
        warn!(
            "AT_HOME is not in /usr. If AT_HOME does not exist on the same partition \
            as /usr/lib, Antimony will be forced to create copies of libraries, rather than \
            using hard-links. This will result in considerable performance degradation."
        );
    }

    path
});

/// The Cache Dir is where cache and SOF is stored. It usually defaults to within `AT_HOME`.
#[allow(
    clippy::unwrap_used,
    reason = "We need to switch users to create the cache. If the operation fails, which in practice it never will, it is a fatal error."
)]
pub static CACHE_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    let mut cache_dir = AT_HOME.join("cache");
    if CONFIG_FILE.force_temp()
        || as_effective!(access(&cache_dir, AccessFlags::W_OK).is_err()).unwrap()
    {
        debug!(
            "Cache dir ({}) not-writable. Pivoting to /tmp",
            cache_dir.display()
        );
        cache_dir = temp_dir().join(format!("antimony-{}", USER.effective.as_raw()));
        let result = || -> Result<()> {
            as_effective!({
                if !cache_dir.exists() {
                    fs::create_dir_all(&cache_dir).unwrap();
                }
                fs::set_permissions(&cache_dir, fs::Permissions::from_mode(0o755))?;
                Ok(())
            })?
        }();

        if result.is_err() {
            warn!("Cannot create the cache directory safely! This is a security hole!");
            if !cache_dir.exists() {
                fs::create_dir_all(&cache_dir).unwrap();
                fs::set_permissions(&cache_dir, fs::Permissions::from_mode(0o755))
                    .expect("Failed to set permissions for cache!");
            }
        }
    }
    cache_dir
});

/// Where profiles and features are stored
pub static AT_CONFIG: LazyLock<PathBuf> = LazyLock::new(|| AT_HOME.join("config"));

/// The user's home folder.
pub static HOME: LazyLock<String> = LazyLock::new(|| {
    // This will almost always be defined, it's a bug if it isn't.
    env::var("HOME").unwrap_or_else(|_| format!("/home/{}", USER_NAME.as_str()))
});

/// The present working directory--used to try and resolve profiles/features
pub static PWD: LazyLock<PathBuf> =
    LazyLock::new(|| PathBuf::from(env::var("PWD").unwrap_or_else(|_| HOME.to_owned())));

/// The User's home folder, as a `PathBuf`.
pub static HOME_PATH: LazyLock<PathBuf> = LazyLock::new(|| PathBuf::from(HOME.as_str()));

/// The runtime directory, as a String.
pub static RUNTIME_STR: LazyLock<String> = LazyLock::new(|| {
    let runtime =
        env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| format!("/run/user/{}", user::USER.real));

    if Path::new(&runtime).exists() {
        runtime
    } else {
        format!("/tmp/run/{}", user::USER.real)
    }
});

/// The runtime directory is where portals and docs are located.
pub static RUNTIME_DIR: LazyLock<PathBuf> = LazyLock::new(|| PathBuf::from(RUNTIME_STR.as_str()));

/// The user's name. We don't trust the USER variable, and instead lookup the name from the Real UID.
pub static USER_NAME: LazyLock<String> = LazyLock::new(|| unsafe {
    let passwd = getpwuid(USER.real.as_raw());

    // This happens if we don't have a /etc/passwd (i.e. within Antimony itself)
    if passwd.is_null() || (*passwd).pw_name.is_null() {
        env::var("USER").unwrap_or_else(|_| "unknown".to_owned())
    } else {
        let name = CString::from_raw((*passwd).pw_name);
        name.to_string_lossy().into_owned()
    }
});

/// The user's data directory is where desktop files are stored, and the home folder is located
pub static DATA_HOME: LazyLock<PathBuf> = LazyLock::new(|| {
    env::var("XDG_DATA_HOME").map_or_else(|_| HOME_PATH.join(".local").join("share"), PathBuf::from)
});

/// The user's config directory.
pub static CONFIG_HOME: LazyLock<PathBuf> = LazyLock::new(|| {
    env::var("XDG_CONFIG_HOME").map_or_else(|_| HOME_PATH.join(".config"), PathBuf::from)
});

/// The text editor to use when editing files.
pub static EDITOR: LazyLock<String> = LazyLock::new(|| {
    let editor = {
        #[allow(clippy::option_if_let_else)]
        if let Ok(editor) = env::var("EDITOR") {
            which(&editor).expect("Could not get path for editor!")
        } else if let Ok(vim) = which("vim") {
            vim
        } else if let Ok(vi) = which("vi") {
            vi
        } else if let Ok(nano) = which("nano") {
            nano
        } else {
            which("emacs").expect("Could not find a suitable editor!")
        }
    };

    editor.to_owned()
});
