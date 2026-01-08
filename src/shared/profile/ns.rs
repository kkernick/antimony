use clap::ValueEnum;
use serde::{Deserialize, Serialize};

/// Namespaces. By default, none are shared. You will likely not need to use these
/// directly, as they are included in relevant features.
#[derive(Eq, Hash, PartialEq, Deserialize, Serialize, ValueEnum, Clone, Copy, Debug)]
#[serde(deny_unknown_fields)]
pub enum Namespace {
    /// Enable all namespaces
    All,

    /// The user namespace is needed to create additional sandboxes (Such as chromium)
    User,

    /// Allow the sandbox to communicate to other processes outside the sandbox.
    /// This is not required for the Proxy.
    Ipc,

    /// Share the Pid namespace, so the process can see all running processes within
    /// the /proc directory.
    Pid,

    /// Use the network feature instead.
    Net,

    /// Enable the UTS namespace.
    Uts,

    /// Allow the sandbox to be managed/manage the system C-Groups.
    CGroup,
}
impl std::fmt::Display for Namespace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Namespace::All => write!(f, "All"),
            Namespace::User => write!(f, "User"),
            Namespace::Ipc => write!(f, "Ipc"),
            Namespace::Pid => write!(f, "Pid"),
            Namespace::Net => write!(f, "Net"),
            Namespace::Uts => write!(f, "Uts"),
            Namespace::CGroup => write!(f, "Cgroup"),
        }
    }
}
