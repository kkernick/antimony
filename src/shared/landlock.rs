//! Landlock policy format

use crate::shared::{Map, Set};
use landlock::{
    rule,
    ruleset::{self, Filesystem, Ruleset},
};
use serde::{Deserialize, Serialize};
use std::{io::ErrorKind, path::PathBuf};

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
    pub flags: Option<Set<ruleset::Flag>>,

    /// Top level attribute flags
    pub scopes: Option<Set<ruleset::Scope>>,

    /// FS attributes the policy should regulate. Note you only need to provide
    /// this field for attributes not mentioned in `paths`--as those are added
    /// automatically.
    pub fs_domain: Set<ruleset::Filesystem>,

    /// Network attributes the policy should regulate. Note you only need to provide
    /// this field for attributes not mentioned in `ports`--as those are added
    /// automatically.
    pub net_domain: Set<ruleset::Network>,

    /// Paths
    pub paths: Option<Map<String, Set<ruleset::Filesystem>>>,

    /// Ports
    pub ports: Option<Map<u64, Set<ruleset::Network>>>,
}
impl LandlockPolicy {
    pub fn construct_ruleset(self) -> Result<Ruleset, landlock::Error> {
        let mut ruleset = Ruleset::new()?;

        if let Some(flags) = self.flags {
            ruleset.add_flags(flags)?;
        }

        if let Some(scopes) = self.scopes {
            ruleset.batch_scope(scopes)?;
        }

        ruleset.batch_fs(self.fs_domain)?;
        ruleset.batch_net(self.net_domain)?;

        if let Some(paths) = self.paths {
            for (path, attrs) in paths {
                if !path.is_empty() {
                    match rule::Path::new_with(path, attrs) {
                        Ok(r) => ruleset.add_rule(r),
                        Err(landlock::Error::Io(e)) if e.kind() == ErrorKind::NotFound => {}
                        Err(e) => return Err(e),
                    }
                }
            }
        }

        if let Some(ports) = self.ports {
            for (net, attrs) in ports {
                ruleset.add_rule(rule::Net::new_with(net, attrs)?);
            }
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

    if let Some(parent) = dest {
        let dir = parent.to_string_lossy().into_owned();
        let paths = policy.paths.get_or_insert_default();
        if let Some(existing) = paths.get_mut(&dir) {
            existing.extend(permissions);
        } else {
            paths.insert(dir.clone(), permissions.into_iter().collect());
        }
    }
}
