//!  Features are miniature profiles used by the latter for common functionality.

use super::profile::{Ipc, Namespace};
use crate::shared::{
    IMap, ISet,
    config::CONFIG_FILE,
    db::{self, Database, Table},
    edit, format_iter,
    profile::{Files, Hooks},
};
use console::style;
use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};
use thiserror::Error;

/// Errors reading feature files
#[derive(Debug, Error)]
pub enum Error {
    /// An error reading/writing/opening the file.
    #[error("Failed to read feature: {0}")]
    Io(#[from] io::Error),

    /// An error if a feature does not exist.
    #[error("No such feature: {0}")]
    NotFound(String),

    /// An error if the TOML is malformed.
    #[error("Malform feature file: {0}")]
    Malformed(#[from] toml::de::Error),

    /// Database error
    #[error("Database error: {0}")]
    Database(#[from] db::Error),
}

/// A Feature
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Feature {
    /// The name of the feature, such as wayland or pipewire.
    pub name: String,

    /// A description of what the feature is for.
    pub description: String,

    /// An optional shell-script that must return 0 for
    /// the feature to be included. If it fails, the feature
    /// is skipped. Useful to ensure a required resource
    /// actually exists
    pub conditional: Option<String>,

    /// If the feature introduces a significant change to the sandbox, warn users.
    pub caveat: Option<String>,

    /// A list of other features this feature depends on.
    pub requires: Option<ISet<String>>,

    /// A list of other features this feature conflicts with.
    pub conflicts: Option<ISet<String>>,

    /// Any IPC busses needed.
    pub ipc: Option<Ipc>,

    /// Namespaces required.
    pub namespaces: Option<ISet<Namespace>>,

    /// Required files
    pub files: Option<Files>,

    /// Required binaries
    pub binaries: Option<ISet<String>>,

    /// Required libraries
    pub libraries: Option<ISet<String>>,

    /// Required devices.
    pub devices: Option<ISet<String>>,

    /// Environment variables to be set. Variables are resolved using standard bash $ENV syntax.
    pub environment: Option<IMap<String, String>>,

    /// Arguments to pass to Bubblewrap directly before the program. This could be actual bubblewrap arguments,
    /// or a wrapper for the sandbox.
    pub sandbox_args: Option<Vec<String>>,

    /// Hooks for this feature. Keep in mind that Hooks have no guarantees on order outside
    /// of the profile/feature they are defined. They'll run within the order defined in
    /// here, but when they run in relation to other features and profiles you cannot
    /// count on.
    pub hooks: Option<Hooks>,
}
impl Feature {
    /// Get a feature from its name.
    pub fn new(name: &str) -> Result<Feature, Error> {
        if name.ends_with(".toml") {
            return Ok(toml::from_str(&fs::read_to_string(name)?)?);
        }

        if !CONFIG_FILE.system_mode()
            && let Some(feature) = db::get::<Self>(name, Database::User, Table::Features)?
        {
            return Ok(feature);
        }

        if let Some(feature) = db::get::<Self>(name, Database::System, Table::Features)? {
            return Ok(feature);
        }

        Err(Error::NotFound(name.to_string()))
    }

    /// Print info about the feature.
    pub fn info(&self, verbose: u8) {
        println!("{}: {}", style(&self.name).bold(), self.description);
        if let Some(caveat) = &self.caveat {
            println!("\t- Caveat: {}", style(caveat).red());
        }

        if verbose > 0 {
            if let Some(requires) = &self.requires {
                println!("\t- Required Features: {}", format_iter(requires.iter()));
            }

            if let Some(conflicts) = &self.conflicts {
                println!(
                    "\t- Conflicting Features: {}",
                    format_iter(conflicts.iter())
                );
            }

            if let Some(ipc) = &self.ipc {
                ipc.info();
            }

            if let Some(namespaces) = &self.namespaces {
                println!("\t- Namespaces: {}", format_iter(namespaces.iter()));
            }

            if let Some(files) = &self.files {
                files.info()
            }

            if let Some(binaries) = &self.binaries {
                println!("\t- Binaries:");
                for binary in binaries {
                    println!("\t\t- {}", style(binary).italic());
                }
            }

            if let Some(libraries) = &self.libraries {
                super::profile::library_info(libraries, verbose);
            }

            if let Some(devices) = &self.devices {
                println!("\t- Devices:");
                for device in devices {
                    println!("\t\t- {}", style(device).italic());
                }
            }

            if let Some(envs) = &self.environment {
                println!("\t- Environment Variables:");
                for (key, value) in envs {
                    println!("\t\t - {key} = {value}");
                }
            }
        }
    }

    /// Edit a feature.
    pub fn edit(path: &Path) -> Result<Option<()>, edit::Error> {
        edit::edit::<Self>(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::env::AT_HOME;

    #[test]
    fn validate_features() {
        let features = Path::new(AT_HOME.as_path()).join("features");
        if features.exists() {
            for path in fs::read_dir(features)
                .expect("No features to test")
                .filter_map(|e| e.ok())
            {
                toml::from_str::<Feature>(
                    &fs::read_to_string(path.path()).expect("Failed to read feature"),
                )
                .expect("Failed to parse feature");
            }
        }
    }
}
