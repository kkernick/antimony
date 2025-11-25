//! A wrapper for `libseccomp`.

use std::{error, fmt};
pub mod action;
pub mod attribute;
pub mod filter;
pub mod notify;
pub mod raw;
pub mod syscall;

/// Get the current architecture.
pub fn get_architecture() -> u32 {
    unsafe { raw::seccomp_arch_native() }
}

/// An error for all aspects of the SECCOMP crate.
#[derive(Debug)]
pub enum Error {
    /// Filter errors.
    Filter(filter::Error),

    /// Syscall errors.
    Syscall(syscall::Error),

    /// Notify errors
    #[cfg(feature = "notify")]
    Notify(notify::Error),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Filter(e) => write!(f, "Filter Error: {e}"),
            Self::Syscall(e) => write!(f, "Syscall Error: {e}"),

            #[cfg(feature = "notify")]
            Self::Notify(e) => write!(f, "Notify Error: {e}"),
        }
    }
}
impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Error::Filter(e) => Some(e),
            Error::Syscall(e) => Some(e),

            #[cfg(feature = "notify")]
            Error::Notify(e) => Some(e),
        }
    }
}
impl From<filter::Error> for Error {
    fn from(e: filter::Error) -> Self {
        Self::Filter(e)
    }
}
impl From<syscall::Error> for Error {
    fn from(e: syscall::Error) -> Self {
        Self::Syscall(e)
    }
}

#[cfg(feature = "notify")]
impl From<notify::Error> for Error {
    fn from(e: notify::Error) -> Self {
        Self::Notify(e)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        action::Action,
        attribute::{Attribute, OptimizeStrategy},
        filter::Filter,
        syscall::Syscall,
    };

    #[test]
    fn init_and_release() {
        Filter::new(Action::Allow).expect("Allow Default Failed");
        Filter::new(Action::KillProcess).expect("KillProcess Default Failed");
        Filter::new(Action::KillThread).expect("KillThread Default Failed");
        Filter::new(Action::Log).expect("Log Default Failed");
        Filter::new(Action::Trap).expect("Trap Default Failed");
    }

    #[test]
    fn attributes() {
        let mut filter = Filter::new(Action::Log).expect("Log Default Failed");

        filter
            .set_attribute(Attribute::BadArchAction(Action::KillProcess))
            .expect("Failed to set Default Action");

        filter
            .set_attribute(Attribute::DisableSSB(true))
            .expect("Failed to disable SSB");

        filter
            .set_attribute(Attribute::Log(true))
            .expect("Failed to set Log");

        filter
            .set_attribute(Attribute::NoNewPrivileges(true))
            .expect("Failed to set NoNewPrivileges");

        filter
            .set_attribute(Attribute::Optimize(OptimizeStrategy::BinaryTree))
            .expect("Failed to set Optimization Type to BST");

        filter
            .set_attribute(Attribute::Optimize(OptimizeStrategy::PriorityAndComplexity))
            .expect("Failed to set Optimization Type to default");

        filter
            .set_attribute(Attribute::ReturnSystemReturnCodes(true))
            .expect("Failed to set SysRawRC");

        filter
            .set_attribute(Attribute::NegativeSyscalls(true))
            .expect("Failed to set TSkip");

        filter
            .set_attribute(Attribute::ThreadSync(true))
            .expect("Failed to set ThreadSync");
    }

    #[test]
    fn add_rule() {
        let mut filter = Filter::new(Action::Allow).expect("Failed to create filter");
        filter
            .add_rule(
                Action::Allow,
                Syscall::from_name("read").expect("Failed to get read syscall"),
            )
            .expect_err("libseccomp should not allow a rule that is the default");
        filter
            .add_rule(
                Action::KillProcess,
                Syscall::from_name("execve").expect("Failed to get ptrace syscall"),
            )
            .expect("Failed to kill execve");
        filter
            .add_rule(Action::KillThread, Syscall::from_number(1))
            .expect("Failed to kill syscall 1");
        filter
            .add_rule(Action::Log, Syscall::from_number(2))
            .expect("Failed to log syscall 2");
        filter
            .add_rule(Action::Trap, Syscall::from_number(3))
            .expect("Failed to log syscall 3");
    }
}
