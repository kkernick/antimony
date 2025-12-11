//! Environment Variables Antimony needs defined.
use anyhow::Result;
use log::{debug, warn};
use once_cell::sync::Lazy;
use spawn::Spawner;
use std::{
    env::{self, temp_dir},
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
};
use which::which;

pub static OVERLAY: Lazy<bool> = Lazy::new(|| {
    let version = || -> Result<String> {
        let out = Spawner::new("/usr/bin/bwrap")
            .arg("--version")?
            .output(true)
            .spawn()?
            .output_all()?;
        Ok(out)
    }();

    match version {
        Ok(version) => version.contains("0.11"),
        Err(_) => false,
    }
});

/// The User's PATH variable, removing ~/.local/bin to prevent
/// Antimony from using itself when a profile has been integrated.
pub static PATH: Lazy<String> = Lazy::new(|| {
    let path = env::var("PATH").unwrap_or("/usr/bin".to_string());
    path.split(':')
        .filter(|e| !e.contains("/.local/bin"))
        .collect::<Vec<_>>()
        .join(":")
});

/// Antimony's home folder is where configuration is stored
pub static AT_HOME: Lazy<PathBuf> = Lazy::new(|| {
    let path = PathBuf::from(env::var("AT_HOME").unwrap_or("/usr/share/antimony".to_string()));
    if !path.starts_with("/usr/") {
        warn!(
            "AT_HOME is not in /usr. If AT_HOME does not exist on the same partition \
            as /usr/lib, Antimony will be forced to create copies of libraries, rather than \
            using hard-links. This will result in considerable performance degradation."
        )
    }

    path
});

/// THe Cache Dir is where cache and SOF is stored. It usually defaults to within AT_HOME.
pub static CACHE_DIR: Lazy<PathBuf> = Lazy::new(|| {
    let mut cache_dir = AT_HOME.join("cache");
    let writeable = if !cache_dir.exists() {
        fs::create_dir(&cache_dir).is_ok()
    } else {
        fs::File::create(cache_dir.join(".test")).is_ok()
    };

    if !writeable {
        debug!("Cache dir not-writable. Pivoting to /tmp");
        cache_dir = temp_dir().join("antimony");
        let save = user::save().expect("Failed to save user!");
        user::set(user::Mode::Effective).expect("Failed to change user!");
        if !cache_dir.exists() {
            fs::create_dir_all(&cache_dir).unwrap();
        }
        fs::set_permissions(&cache_dir, fs::Permissions::from_mode(0o750))
            .expect("Failed to set permissions for cache!");
        user::restore(save).expect("Failed to restore user!");
    }
    cache_dir
});

/// The user's home folder.
pub static HOME: Lazy<String> = Lazy::new(|| {
    // This will almost always be defined, it's a bug if it isn't.
    if let Ok(home) = env::var("HOME") {
        home

    // If that fails, construct it manually
    } else {
        format!("/home/{}", USER_NAME.as_str())
    }
});

/// The present working directory--used to try and resolve profiles/features
pub static PWD: Lazy<PathBuf> =
    Lazy::new(|| PathBuf::from(env::var("PWD").unwrap_or(HOME.to_string())));

/// The User's home folder, as a PathBuf.
pub static HOME_PATH: Lazy<PathBuf> = Lazy::new(|| PathBuf::from(HOME.as_str()));

/// The runtime directory, as a String.
pub static RUNTIME_STR: Lazy<String> = Lazy::new(|| {
    if let Ok(runtime) = env::var("XDG_RUNTIME_DIR") {
        runtime
    } else {
        format!("/run/user/{}", user::USER.real)
    }
});

/// The runtime directory is where portals and docs are located.
pub static RUNTIME_DIR: Lazy<PathBuf> = Lazy::new(|| PathBuf::from(RUNTIME_STR.as_str()));

pub static USER_NAME: Lazy<String> = Lazy::new(|| env::var("USER").expect("USER is not defined"));

/// The user's data directory is where desktop files are stored, and the home folder is located
pub static DATA_HOME: Lazy<PathBuf> = Lazy::new(|| {
    if let Ok(data) = env::var("XDG_DATA_HOME") {
        PathBuf::from(data)
    } else {
        HOME_PATH.join(".local").join("share")
    }
});

/// The user's config directory.
pub static CONFIG_HOME: Lazy<PathBuf> = Lazy::new(|| {
    if let Ok(data) = env::var("XDG_CONFIG_HOME") {
        PathBuf::from(data)
    } else {
        HOME_PATH.join(".config")
    }
});

/// The text editor to use when editing files.
pub static EDITOR: Lazy<String> = Lazy::new(|| {
    let editor = {
        if let Ok(editor) = env::var("EDITOR") {
            which(editor).expect("Could not get path for editor!")
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

    editor.to_string_lossy().into_owned()
});
