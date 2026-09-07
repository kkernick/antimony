//! Landlock policy format

use std::{io::ErrorKind, path::PathBuf};

use crate::shared::{Map, Set};
use landlock::{
    rule,
    ruleset::{self, Filesystem, Ruleset},
};
use log::debug;
use serde::{Deserialize, Serialize};

pub static RW: [Filesystem; 10] = [
    Filesystem::ReadFile,
    Filesystem::ReadDir,
    Filesystem::WriteFile,
    Filesystem::MakeDir,
    Filesystem::Truncate,
    Filesystem::Refer,
    Filesystem::RemoveDir,
    Filesystem::RemoveFile,
    Filesystem::MakeReg,
    Filesystem::MakeSym,
];

pub static RO: [Filesystem; 2] = [Filesystem::ReadFile, Filesystem::ReadDir];

/// A serialized policy document for how the Landlock Ruleset should be constructed
#[derive(Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct LandlockPolicy {
    /// Top level attribute flags
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub flags: Set<ruleset::Flag>,

    /// Top level attribute flags
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub scopes: Set<ruleset::Scope>,

    /// FS attributes the policy should regulate. Note you only need to provide
    /// this field for attributes not mentioned in `paths`--as those are added
    /// automatically.
    pub fs_domain: Set<ruleset::Filesystem>,

    /// Network attributes the policy should regulate. Note you only need to provide
    /// this field for attributes not mentioned in `ports`--as those are added
    /// automatically.
    pub net_domain: Set<ruleset::Network>,

    /// Paths
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub paths: Map<String, Set<ruleset::Filesystem>>,

    /// Ports
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub ports: Map<u64, Set<ruleset::Network>>,
}
impl LandlockPolicy {
    pub fn construct_ruleset(self) -> Result<Ruleset, landlock::Error> {
        let mut ruleset = Ruleset::new()?;
        ruleset.add_flags(self.flags)?;
        ruleset.batch_scope(self.scopes)?;
        ruleset.batch_fs(self.fs_domain)?;
        ruleset.batch_net(self.net_domain)?;

        for (path, attrs) in self.paths {
            match rule::Path::new_with(path, attrs) {
                Ok(r) => ruleset.add_rule(r),
                Err(landlock::Error::Io(e)) if e.kind() == ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        for (net, attrs) in self.ports {
            ruleset.add_rule(rule::Net::new_with(net, attrs)?);
        }

        Ok(ruleset)
    }
}

pub fn update_policy(
    path: impl Into<PathBuf>,
    policy: &mut LandlockPolicy,
    permissions: impl IntoIterator<Item = Filesystem>,
) {
    let path = path.into();
    let dest = if path.is_file() {
        path.parent()
    } else {
        Some(path.as_ref())
    };

    debug!(
        "Updating policy with {} (Localized to {dest:?}",
        path.display()
    );

    if let Some(parent) = dest {
        let dir = parent.to_string_lossy().into_owned();
        if let Some(existing) = policy.paths.get_mut(&dir) {
            debug!("Extending permissions for {dir:?}: {existing:?}");
            existing.extend(permissions);
        } else {
            policy
                .paths
                .insert(dir.clone(), permissions.into_iter().collect());
        }
        debug!(
            "Extending permissions for {dir:?}: {:?}",
            policy.paths.get(&dir)
        );
    }
}
