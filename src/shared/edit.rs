//! Edit a file.

use dialoguer::Confirm;
use log::error;
use serde::Serialize;
use serde::de::DeserializeOwned;
use spawn::Spawner;
use std::{
    fs::{self, File},
    io::{self, Write},
    path::Path,
};
use thiserror::Error;
use user::{Mode, as_effective, as_real};

use crate::shared::env::EDITOR;

/// An error for issues around Profiles.
#[derive(Debug, Error)]
pub enum Error {
    /// Misc IO errors.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// Serialization errors.
    #[error("Failed to serialize file: {0}")]
    Serialize(#[from] toml::ser::Error),

    /// Misc Errno errors.
    #[error("System error: {0}")]
    Errno(#[from] nix::errno::Errno),

    /// Errors running the profile
    #[error("Failed to spawn editor: {0}")]
    Spawn(#[from] spawn::SpawnError),

    /// Errors managing a spawned profile
    #[error("Failed to communicate with editor: {0}")]
    Handle(#[from] spawn::HandleError),

    /// Dialog errors
    #[error("Dialog errors: {0}")]
    Dialog(#[from] dialoguer::Error),
}

/// Edit a file via a temporary, committing the changes back into the file.
pub fn edit<T: DeserializeOwned + Serialize>(path: &Path) -> Result<Option<()>, Error> {
    // Pivot to real mode to edit the temporary.
    // Editors, like vim, can run arbitrary commands, and we don't want
    // to extend privilege.
    let temp = as_real!(Result<_, Error>, {
        let temp = temp::Builder::new()
            .owner(Mode::Real)
            .extension("toml")
            .create::<temp::File>()?;

        fs::copy(path, temp.full())?;
        Ok(temp)
    })??;

    let original = fs::read_to_string(path)?;

    // Loop until the user either:
    //  1. Provides a valid edit.
    //  2. Bails
    let buffer = loop {
        // Launch the editor.
        Spawner::new(EDITOR.as_str())?
            .preserve_env(true)
            .arg(temp.full().to_string_lossy())?
            .mode(user::Mode::Real)
            .spawn()?
            .wait()?;

        // Read the contents.
        match fs::read_to_string(temp.full()) {
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
                        return Ok(Some(()));
                    }
                }
            },
            Err(e) => {
                error!("Failed to read temporary profile: {e}");
                return Ok(None);
            }
        }
    };

    as_effective!(Result<(), Error>, {
        write!(
            File::create(path)?,
            "{}",
            toml::to_string(&buffer)?
        )?;
        Ok(())
    })??;

    Ok(Some(()))
}
