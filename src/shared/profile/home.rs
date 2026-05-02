use crate::{cli, shared::env::DATA_HOME};
use bilrost::{Enumeration, Message};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Sandboxes can define home folders in the user's home at ~/.local/share/antimony
/// for persistent configurations and caches.
#[derive(Deserialize, Serialize, Default, Debug, PartialEq, Eq, Message)]
pub struct Home {
    /// The name of the home folder in ~/.local/share/antimony
    pub name: Option<String>,

    /// How to mount the home
    pub policy: Option<HomePolicy>,

    /// Where to mount the home within the sandbox. Defaults to ~/antimony
    /// Changing this feature requires overlays.
    pub path: Option<String>,

    /// Whether to lock the home to a single instance
    pub lock: Option<bool>,
}
impl Home {
    pub fn merge(&mut self, home: Self) {
        if self.name.is_none() {
            self.name = home.name;
        }
        if self.policy.is_none() {
            self.policy = home.policy;
        }
        if self.path.is_none() {
            self.path = home.path;
        }
        if self.lock.is_none() {
            self.lock = home.lock;
        }
    }

    pub const fn from_args(args: &mut cli::run::Args) -> Self {
        Self {
            name: args.home_name.take(),
            policy: args.home_policy.take(),
            path: args.home_path.take(),
            lock: args.home_lock.take(),
        }
    }

    pub fn path(&self, name: &str) -> PathBuf {
        DATA_HOME
            .join("antimony")
            .join(self.name.as_ref().map_or(name, |name| name))
    }
}

/// The Home Policy being set creates a persistent home folder for the profile.
#[derive(
    Deserialize, Serialize, PartialEq, Eq, Clone, Copy, ValueEnum, Default, Debug, Enumeration,
)]
#[serde(deny_unknown_fields)]
pub enum HomePolicy {
    /// Do not use a home profile.
    #[default]
    None = 0,

    /// The Home Folder is passed read/write. Applications that only permit a single
    /// instance, such as Chromium, will get upset if you launch multiple instances of
    /// the sandbox.
    Enabled = 1,

    /// Mount the Home Folder as a Read-Only overlay.
    ReadOnly = 2,

    /// Once an application has been configured, Overlay effectively freezes it in place by
    /// mounting it as a temporary overlay. Changes made in the sandbox are discarded, and
    /// it can be shared by multiple instances, even if that application doesn't typically
    /// support multiple instances (Zed, Chromium, etc).
    Overlay = 3,
}
