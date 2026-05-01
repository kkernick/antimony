//! A wrapper on `SCMP_FLTATR`.

use super::raw::scmp_filter_attr::{
    self, SCMP_FLTATR_ACT_BADARCH, SCMP_FLTATR_API_SYSRAWRC, SCMP_FLTATR_API_TSKIP,
    SCMP_FLTATR_CTL_LOG, SCMP_FLTATR_CTL_NNP, SCMP_FLTATR_CTL_OPTIMIZE, SCMP_FLTATR_CTL_SSB,
    SCMP_FLTATR_CTL_TSYNC,
};
use crate::action::Action;
use std::fmt;

/// How to organize the filter rules.
pub enum OptimizeStrategy {
    /// Uses priority and rule complexity for ordering.
    PriorityAndComplexity,

    /// Uses a simple Binary Search Tree for ordering.
    BinaryTree,
}

/// Attributes.
pub enum Attribute {
    /// The action for when an invalid architecture is detected.
    BadArchAction(Action),

    /// Deny new privileges on load
    NoNewPrivileges(bool),

    /// Sync all threads in the process to make sure the filter applies.
    ThreadSync(bool),

    /// Allow negative Syscalls.
    NegativeSyscalls(bool),

    /// Log syscalls to Audit.
    Log(bool),

    /// Disable SSB Mitigation.
    DisableSSB(bool),

    /// How the rules are ordered.
    Optimize(OptimizeStrategy),

    /// Return system return codes.
    ReturnSystemReturnCodes(bool),
}
impl Attribute {
    /// Get the raw name of the attribute.
    #[must_use]
    pub const fn name(&self) -> scmp_filter_attr {
        match self {
            Self::BadArchAction(_) => SCMP_FLTATR_ACT_BADARCH,
            Self::NoNewPrivileges(_) => SCMP_FLTATR_CTL_NNP,
            Self::ThreadSync(_) => SCMP_FLTATR_CTL_TSYNC,
            Self::NegativeSyscalls(_) => SCMP_FLTATR_API_TSKIP,
            Self::Log(_) => SCMP_FLTATR_CTL_LOG,
            Self::DisableSSB(_) => SCMP_FLTATR_CTL_SSB,
            Self::Optimize(_) => SCMP_FLTATR_CTL_OPTIMIZE,
            Self::ReturnSystemReturnCodes(_) => SCMP_FLTATR_API_SYSRAWRC,
        }
    }

    /// Get the current value of the attribute.
    #[must_use]
    pub fn value(&self) -> u32 {
        match self {
            Self::BadArchAction(action) => (*action).into(),
            Self::NoNewPrivileges(set)
            | Self::ThreadSync(set)
            | Self::NegativeSyscalls(set)
            | Self::Log(set)
            | Self::DisableSSB(set)
            | Self::ReturnSystemReturnCodes(set) => u32::from(*set),
            Self::Optimize(strategy) => match strategy {
                OptimizeStrategy::PriorityAndComplexity => 1,
                OptimizeStrategy::BinaryTree => 2,
            },
        }
    }

    /// Get a string value for the attribute
    #[must_use]
    pub const fn str(&self) -> &'static str {
        match self {
            Self::BadArchAction(_) => "Bad Arch Action",
            Self::NoNewPrivileges(_) => "No New Privileges",
            Self::ThreadSync(_) => "Thread Sync",
            Self::NegativeSyscalls(_) => "Negative Syscalls",
            Self::Log(_) => "Log",
            Self::DisableSSB(_) => "Disable SSB",
            Self::Optimize(_) => "Optimize",
            Self::ReturnSystemReturnCodes(_) => "Return System Return Codes",
        }
    }
}
impl fmt::Display for Attribute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.str())
    }
}
impl fmt::Debug for Attribute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.str())
    }
}
