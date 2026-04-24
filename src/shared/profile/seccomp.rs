use bilrost::Enumeration;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// The SECCOMP Policy for the Profile
#[derive(
    Hash, Debug, Deserialize, Serialize, PartialEq, Eq, Copy, Clone, ValueEnum, Default, Enumeration,
)]
#[serde(deny_unknown_fields)]
pub enum SeccompPolicy {
    /// Disable SECCOMP
    #[default]
    Disabled = 0,

    /// Syscalls are logged to construct a policy for the profile.
    Permissive = 1,

    /// The policy is enforced: unrecognized syscalls return with EPERM.
    Enforcing = 2,

    /// The policy is enforced: unrecognized syscalls are presented to the user for decision.
    Notifying = 3,
}
impl std::fmt::Display for SeccompPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disabled => write!(f, "Disabled"),
            Self::Permissive => write!(f, "Permissive"),
            Self::Enforcing => write!(f, "Enforcing"),
            Self::Notifying => write!(f, "Notifying"),
        }
    }
}
