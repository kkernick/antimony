//! Wrapper for SCMP_ACT.
use super::raw;

/// An Action.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    /// Kill the entire process.
    KillProcess,

    /// Kill the offending thread.
    KillThread,

    /// Trap the process.
    Trap,

    /// Log to the audit framework.
    Log,

    /// Allow the call.
    Allow,

    /// Notify the user space monitor
    Notify,

    /// An ERRNO code.
    Errno(i32),
}
impl From<Action> for u32 {
    fn from(action: Action) -> u32 {
        match action {
            Action::KillProcess => raw::SCMP_ACT_KILL_PROCESS,
            Action::KillThread => raw::SCMP_ACT_KILL_THREAD,
            Action::Trap => raw::SCMP_ACT_TRAP,
            Action::Log => raw::SCMP_ACT_LOG,
            Action::Allow => raw::SCMP_ACT_ALLOW,
            Action::Errno(e) => 0x00050000 | (e as u32 & 0x0000ffff),
            Action::Notify => raw::SCMP_ACT_NOTIFY,
        }
    }
}
impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Action::KillProcess => write!(f, "Kill Process"),
            Action::KillThread => write!(f, "Kill Thread"),
            Action::Trap => write!(f, "Trap"),
            Action::Log => write!(f, "Log"),
            Action::Allow => write!(f, "Allow"),
            Action::Notify => write!(f, "Notify"),
            Action::Errno(errno) => write!(f, "{errno}"),
        }
    }
}
