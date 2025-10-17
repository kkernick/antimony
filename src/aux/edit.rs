use dialoguer::Confirm;
use log::error;
use serde::Serialize;
use serde::de::DeserializeOwned;
use spawn::Spawner;
use std::io::Write;
use std::{fs::File, path::Path};

use crate::aux::env::EDITOR;

/// An error for issues around Profiles.
#[derive(Debug)]
pub enum Error {
    /// When the profile cannot be Deserialized.
    Deserialize(toml::de::Error),

    /// When the profile cannot be Serialized.
    Serialize(toml::ser::Error),

    /// Misc IO errors.
    Io(&'static str, std::io::Error),

    /// Misc Errno errors.
    Errno(&'static str, nix::errno::Errno),

    /// Errors resolving/creating paths.
    Path(which::Error),

    /// Errors running the profile
    Spawn(spawn::SpawnError),

    /// Errors managing a spawned profile
    Handle(spawn::HandleError),

    /// Dialog errors
    Dialog(dialoguer::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::Deserialize(e) => write!(f, "Failed to read profile: {e}"),
            Self::Serialize(e) => write!(f, "Failed to write profile: {e}"),
            Self::Io(what, e) => write!(f, "Failed to {what}: {e}"),
            Self::Errno(what, e) => write!(f, "{what} failed: {e}"),
            Self::Path(e) => write!(f, "Path error: {e}"),
            Self::Spawn(e) => write!(f, "Failed to run command: {e}"),
            Self::Handle(e) => write!(f, "Failed to manage subprocess: {e}"),
            Self::Dialog(e) => write!(f, "Failed to prompt user: {e}"),
        }
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Deserialize(e) => Some(e),
            Self::Serialize(e) => Some(e),
            Self::Io(_, e) => Some(e),
            Self::Errno(_, e) => Some(e),
            Self::Path(e) => Some(e),
            Self::Dialog(e) => Some(e),
            _ => None,
        }
    }
}
impl From<spawn::SpawnError> for Error {
    fn from(val: spawn::SpawnError) -> Self {
        Error::Spawn(val)
    }
}
impl From<spawn::HandleError> for Error {
    fn from(val: spawn::HandleError) -> Self {
        Error::Handle(val)
    }
}
impl From<which::Error> for Error {
    fn from(val: which::Error) -> Self {
        Error::Path(val)
    }
}
impl From<toml::de::Error> for Error {
    fn from(val: toml::de::Error) -> Self {
        Error::Deserialize(val)
    }
}
impl From<toml::ser::Error> for Error {
    fn from(val: toml::ser::Error) -> Self {
        Error::Serialize(val)
    }
}
impl From<dialoguer::Error> for Error {
    fn from(val: dialoguer::Error) -> Self {
        Error::Dialog(val)
    }
}

pub fn edit<T: DeserializeOwned + Serialize>(path: &Path) -> Result<Option<()>, Error> {
    let saved = user::save().map_err(|e| Error::Errno("Get User", e))?;

    // Pivot to real mode to edit the temporary.
    // Editors, like vim, can run arbitrary commands, and we don't want
    // to extend privilege.
    user::set(user::Mode::Real).map_err(|e| Error::Errno("Set User", e))?;
    let temp = tempfile::Builder::new()
        .suffix(".toml")
        .tempfile()
        .map_err(|e| Error::Io("open temporary file", e))?;

    std::fs::copy(path, &temp).map_err(|e| Error::Io("write temporary file", e))?;
    let original =
        std::fs::read_to_string(path).map_err(|e| Error::Io("read original profile", e))?;

    // Loop until the user either:
    //  1. Provides a valid edit.
    //  2. Bails
    let buffer = loop {
        // Launch the editor.
        Spawner::new(EDITOR.as_str())
            .preserve_env(true)
            .arg(temp.path().to_string_lossy())?
            .mode(user::Mode::Real)
            .spawn()?
            .wait()?;

        // Read the contents.
        match std::fs::read_to_string(&temp) {
            Ok(string) => match toml::from_str::<T>(string.as_ref()) {
                // If they didn't make any changes, we want to tell edit
                // so that they don't create a redundant user profile.
                Ok(profile) => {
                    if string == original {
                        println!("No modification made.");
                        return Ok(None);
                    } else {
                        break profile;
                    }
                }

                // If there's an error, make the user correct, or bail entirely.
                Err(e) => {
                    let retry = Confirm::new()
                        .with_prompt(format!("Syntax error: {e}\nTry again?"))
                        .interact()?;

                    if !retry {
                        user::restore(saved).map_err(|e| Error::Errno("Set user", e))?;
                        return Ok(Some(()));
                    }
                }
            },
            Err(e) => {
                error!("Failed to read temporary profile: {e}");
                user::restore(saved).map_err(|e| Error::Errno("Set user", e))?;
                return Ok(None);
            }
        }
    };
    user::set(user::Mode::Effective).map_err(|e| Error::Errno("Set user", e))?;
    write!(
        File::create(path).map_err(|e| Error::Io("write", e))?,
        "{}",
        toml::to_string(&buffer)?
    )
    .map_err(|e| Error::Io("write", e))?;
    user::restore(saved).map_err(|e| Error::Errno("Set user", e))?;
    Ok(Some(()))
}
