//! Environment Variables Antimony needs defined.
use log::warn;
use once_cell::sync::Lazy;
use std::path::PathBuf;
use which::which;

/// The User's PATH variable, removing ~/.local/bin to prevent
/// Antimony from using itself when a profile has been integrated.
pub static PATH: Lazy<String> = Lazy::new(|| {
    let path = std::env::var("PATH").unwrap_or("/usr/bin".to_string());
    path.split(':')
        .filter(|e| !e.contains("/.local/bin"))
        .collect::<Vec<_>>()
        .join(":")
});

/// Antimony's home folder is where configuration and caches are stored.
pub static AT_HOME: Lazy<PathBuf> = Lazy::new(|| {
    let path = PathBuf::from(std::env::var("AT_HOME").unwrap_or("/usr/share/antimony".to_string()));
    if !path.starts_with("/usr/") {
        warn!(
            "AT_HOME is not in /usr. If AT_HOME does not exist on the same partition \
            as /usr/lib, Antimony will be forced to create copies of libraries, rather than \
            using hard-links. This will result in considerable performance degradation."
        )
    }

    path
});

/// The user's home folder.
pub static HOME: Lazy<String> = Lazy::new(|| {
    // This will almost always be defined, it's a bug if it isn't.
    if let Ok(home) = std::env::var("HOME") {
        home

    // If that fails, construct it manually
    } else {
        format!("/home/{}", USER_NAME.as_str())
    }
});

/// The present working directory--used to try and resolve profiles/features
pub static PWD: Lazy<PathBuf> =
    Lazy::new(|| PathBuf::from(std::env::var("PWD").unwrap_or(HOME.to_string())));

/// The User's home folder, as a PathBuf.
pub static HOME_PATH: Lazy<PathBuf> = Lazy::new(|| PathBuf::from(HOME.as_str()));

/// The runtime directory, as a String.
pub static RUNTIME_STR: Lazy<String> = Lazy::new(|| {
    if let Ok(runtime) = std::env::var("XDG_RUNTIME_DIR") {
        runtime
    } else {
        format!("/run/user/{}", user::USER.real)
    }
});

/// The runtime directory is where portals and docs are located.
pub static RUNTIME_DIR: Lazy<PathBuf> = Lazy::new(|| PathBuf::from(RUNTIME_STR.as_str()));

pub static USER_NAME: Lazy<String> =
    Lazy::new(|| std::env::var("USER").expect("USER is not defined"));

/// The user's data directory is where desktop files are stored, and the home folder is located
pub static DATA_HOME: Lazy<PathBuf> = Lazy::new(|| {
    if let Ok(data) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(data)
    } else {
        HOME_PATH.join(".local").join("share")
    }
});

/// The text editor to use when editing files.
pub static EDITOR: Lazy<String> = Lazy::new(|| {
    let editor = {
        if let Ok(editor) = std::env::var("EDITOR") {
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
