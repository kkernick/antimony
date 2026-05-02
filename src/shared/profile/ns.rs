use bilrost::Enumeration;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// Namespaces. By default, none are shared. You will likely not need to use these
/// directly, as they are included in relevant features.
#[derive(
    Eq, Hash, PartialEq, Deserialize, Serialize, ValueEnum, Clone, Copy, Debug, Enumeration,
)]
#[serde(deny_unknown_fields)]
pub enum Namespace {
    /// Enable all namespaces
    All = 0,

    /// The user namespace is needed to create additional sandboxes (Such as chromium)
    User = 1,

    /// Allow the sandbox to communicate to other processes outside the sandbox.
    /// This is not required for the Proxy.
    Ipc = 2,

    /// Share the Pid namespace, so the process can see all running processes within
    /// the /proc directory.
    Pid = 3,

    /// Use the network feature instead.
    Net = 4,

    /// Enable the UTS namespace.
    Uts = 5,

    /// Allow the sandbox to be managed/manage the system C-Groups.
    CGroup = 6,
}
impl std::fmt::Display for Namespace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::All => write!(f, "All"),
            Self::User => write!(f, "User"),
            Self::Ipc => write!(f, "Ipc"),
            Self::Pid => write!(f, "Pid"),
            Self::Net => write!(f, "Net"),
            Self::Uts => write!(f, "Uts"),
            Self::CGroup => write!(f, "Cgroup"),
        }
    }
}
