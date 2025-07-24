//!  Features are miniature profiles used by the latter for common functionality.
use super::profile::{Ipc, Namespace};
use crate::aux::env::{AT_HOME, PWD};
use crate::aux::profile::Files;
use console::style;
use serde::Deserialize;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

/// Errors reading feature files
#[derive(Debug)]
pub enum Error {
    /// An error reading/writing/opening the file.
    Io(std::io::Error),

    /// An error if a feature does not exist.
    NotFound(String),

    /// An error if the TOML is malformed.
    Malformed(toml::de::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "Failed to read feature: {e}"),
            Self::NotFound(name) => write!(f, "Feature not found: {name}"),
            Self::Malformed(e) => write!(f, "Malformed feature file: {e}"),
        }
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}
impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Error::Io(value)
    }
}
impl From<toml::de::Error> for Error {
    fn from(value: toml::de::Error) -> Self {
        Error::Malformed(value)
    }
}

/// A Feature
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Feature {
    /// The name of the feature, such as wayland or pipewire.
    pub name: String,

    /// A description of what the feature is for.
    pub description: String,

    /// If the feature introduces a significant change to the sandbox, warn users.
    pub caveat: Option<String>,

    /// A list of other features this feature depends on.
    pub requires: Option<BTreeSet<String>>,

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
    pub environment: Option<HashMap<String, String>>,
}
impl Feature {
    /// Get a feature from its name.
    pub fn new(name: &str) -> Result<Feature, Error> {
        // Features can either be sourced via an absolute path, or AT_HOME.
        let feature: String = {
            if name.ends_with(".toml") {
                std::fs::read_to_string(Path::new(name))?
            } else if let Ok(path) =
                std::fs::read_to_string(AT_HOME.join("features").join(name).with_extension("toml"))
            {
                path
            } else if let Ok(path) =
                std::fs::read_to_string(PWD.join("config").join("features").join(name))
            {
                path
            } else {
                return Err(Error::NotFound(name.to_string()));
            }
        };

        Ok(toml::from_str(feature.as_str())?)
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_features() {
        let features = Path::new(AT_HOME.as_path()).join("features");
        if features.exists() {
            for path in std::fs::read_dir(features)
                .expect("No features to test")
                .filter_map(|e| e.ok())
            {
                toml::from_str::<Feature>(
                    &std::fs::read_to_string(path.path()).expect("Failed to read feature"),
                )
                .expect("Failed to parse feature");
            }
        }
    }
}
