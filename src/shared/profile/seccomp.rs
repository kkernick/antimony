use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// The SECCOMP Policy for the Profile
#[derive(Hash, Debug, Deserialize, Serialize, PartialEq, Eq, Copy, Clone, ValueEnum, Default)]
#[serde(deny_unknown_fields)]
pub enum SeccompPolicy {
    /// Disable SECCOMP
    #[default]
    Disabled,

    /// Syscalls are logged to construct a policy for the profile.
    Permissive,

    /// The policy is enforced: unrecognized syscalls return with EPERM.
    Enforcing,

    /// The policy is enforced: unrecognized syscalls are presented to the user for decision.
    Notifying,
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
