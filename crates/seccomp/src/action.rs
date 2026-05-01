//! Wrapper for `SCMP_ACT`.

use super::raw;
use std::fmt;

/// An Action.
#[derive(Clone, Copy, Debug)]
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
    #[allow(clippy::cast_sign_loss)]
    fn from(action: Action) -> Self {
        match action {
            Action::KillProcess => raw::SCMP_ACT_KILL_PROCESS,
            Action::KillThread => raw::SCMP_ACT_KILL_THREAD,
            Action::Trap => raw::SCMP_ACT_TRAP,
            Action::Log => raw::SCMP_ACT_LOG,
            Action::Allow => raw::SCMP_ACT_ALLOW,
            Action::Errno(e) => 0x0005_0000 | (e as Self & 0x0000_ffff),
            Action::Notify => raw::SCMP_ACT_NOTIFY,
        }
    }
}
impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KillProcess => write!(f, "Kill Process"),
            Self::KillThread => write!(f, "Kill Thread"),
            Self::Trap => write!(f, "Trap"),
            Self::Log => write!(f, "Log"),
            Self::Allow => write!(f, "Allow"),
            Self::Notify => write!(f, "Notify"),
            Self::Errno(errno) => write!(f, "{errno}"),
        }
    }
}
