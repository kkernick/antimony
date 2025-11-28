//!  Features are miniature profiles used by the latter for common functionality.

use super::profile::{Ipc, Namespace};
use crate::aux::{
    edit,
    env::{AT_HOME, PWD},
    profile::Files,
};
use console::style;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    error, fmt, fs, io,
    path::{Path, PathBuf},
};

/// Errors reading feature files
#[derive(Debug)]
pub enum Error {
    /// An error reading/writing/opening the file.
    Io(io::Error),

    /// An error if a feature does not exist.
    NotFound(String),

    /// An error if the TOML is malformed.
    Malformed(toml::de::Error),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "Failed to read feature: {e}"),
            Self::NotFound(name) => write!(f, "Feature not found: {name}"),
            Self::Malformed(e) => write!(f, "Malformed feature file: {e}"),
        }
    }
}
impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}
impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Error::Io(value)
    }
}
impl From<toml::de::Error> for Error {
    fn from(value: toml::de::Error) -> Self {
        Error::Malformed(value)
    }
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
    pub requires: Option<BTreeSet<String>>,

    /// A list of other features this feature conflicts with.
    pub conflicts: Option<BTreeSet<String>>,

    /// Any IPC busses needed.
    pub ipc: Option<Ipc>,

    /// Namespaces required.
    pub namespaces: Option<BTreeSet<Namespace>>,

    /// Required files
    pub files: Option<Files>,

    /// Required binaries
    pub binaries: Option<BTreeSet<String>>,

    /// Required libraries
    pub libraries: Option<BTreeSet<String>>,

    /// Required devices.
    pub devices: Option<BTreeSet<String>>,

    /// Environment variables to be set. Variables are resolved using standard bash $ENV syntax.
    pub environment: Option<BTreeMap<String, String>>,

    /// Arguments to pass to Bubblewrap directly before the program. This could be actual bubblewrap arguments,
    /// or a wrapper for the sandbox.
    pub sandbox_args: Option<Vec<String>>,
}
impl Feature {
    /// Get the path to a feature.
    pub fn path(name: &str) -> Result<PathBuf, Error> {
        if name.ends_with(".toml") {
            return Ok(PathBuf::from(name));
        }

        let system = AT_HOME.join("features").join(name).with_extension("toml");
        if system.exists() {
            return Ok(system);
        }

        let local = PWD.join("config").join("features").join(name);
        if local.exists() {
            return Ok(local);
        }

        Err(Error::NotFound(name.to_string()))
    }

    /// Get a feature from its name.
    pub fn new(name: &str) -> Result<Feature, Error> {
        Ok(toml::from_str(&fs::read_to_string(&Self::path(name)?)?)?)
    }

    /// Print info about the feature.
    pub fn info(&self, verbose: u8) {
        println!("{}: {}", style(&self.name).bold(), self.description);
        if let Some(caveat) = &self.caveat {
            println!("\t- Caveat: {}", style(caveat).red());
        }

        if verbose > 0 {
            if let Some(requires) = &self.requires {
                println!("\t- Required Features: {requires:?}");
            }

            if let Some(conflicts) = &self.conflicts {
                println!("\t- Conflicting Features: {conflicts:?}");
            }

            if let Some(ipc) = &self.ipc {
                ipc.info();
            }

            if let Some(namespaces) = &self.namespaces {
                println!(
                    "\t- Namespaces: {}",
                    namespaces
                        .iter()
                        .map(|e| format!("{e:?}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                );
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
