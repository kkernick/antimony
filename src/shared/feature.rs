//!  Features are miniature profiles used by the latter for common functionality.

use super::profile::{ipc::Ipc, ns::Namespace};
use crate::shared::{
    Map, Set, edit,
    profile::{files::Files, hooks::Hooks},
    store::{self, Object},
};
use serde::{Deserialize, Serialize};
use std::io;
use thiserror::Error;

/// Errors reading feature files
#[derive(Debug, Error)]
pub enum Error {
    /// An error reading/writing/opening the file.
    #[error("Failed to read feature: {0}")]
    Io(#[from] io::Error),

    /// An error if the TOML is malformed.
    #[error("Malformed feature file: {0}")]
    Malformed(#[from] toml::de::Error),

    /// Store errors
    #[error("Failed to access feature store: {0}")]
    Store(#[from] store::Error),
}

/// A Feature
#[derive(Deserialize, Serialize, PartialEq)]
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
    pub requires: Option<Set<String>>,

    /// A list of other features this feature conflicts with.
    pub conflicts: Option<Set<String>>,

    /// Any IPC busses needed.
    pub ipc: Option<Ipc>,

    /// Namespaces required.
    pub namespaces: Option<Set<Namespace>>,

    /// Required files
    pub files: Option<Files>,

    /// Required binaries
    pub binaries: Option<Set<String>>,

    /// Required libraries
    pub libraries: Option<Set<String>>,

    /// Required devices.
    pub devices: Option<Set<String>>,

    /// Environment variables to be set. Variables are resolved using standard bash $ENV syntax.
    pub environment: Option<Map<String, String>>,

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
    pub fn new(name: &str) -> Result<Self, Error> {
        store::load::<Self, Error>(name, Object::Feature, false)
    }

    /// Edit a feature.
    pub fn edit(feat: &str) -> Result<Option<String>, edit::Error> {
        edit::edit::<Self>(feat)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::store::SYSTEM_STORE;

    #[test]
    fn validate_features() {
        for feature in SYSTEM_STORE
            .with_borrow(|s| s.get(Object::Feature))
            .expect("Failed to get features")
        {
            SYSTEM_STORE
                .with_borrow(|s| s.fetch(&feature, Object::Feature))
                .expect("Failed to read feature");
        }
    }
}
