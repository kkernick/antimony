//! A wrapper on SCMP_FLTATR.
use super::raw::scmp_filter_attr::{self, *};
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
    pub fn name(&self) -> scmp_filter_attr {
        match self {
            Attribute::BadArchAction(_) => SCMP_FLTATR_ACT_BADARCH,
            Attribute::NoNewPrivileges(_) => SCMP_FLTATR_CTL_NNP,
            Attribute::ThreadSync(_) => SCMP_FLTATR_CTL_TSYNC,
            Attribute::NegativeSyscalls(_) => SCMP_FLTATR_API_TSKIP,
            Attribute::Log(_) => SCMP_FLTATR_CTL_LOG,
            Attribute::DisableSSB(_) => SCMP_FLTATR_CTL_SSB,
            Attribute::Optimize(_) => SCMP_FLTATR_CTL_OPTIMIZE,
            Attribute::ReturnSystemReturnCodes(_) => SCMP_FLTATR_API_SYSRAWRC,
        }
    }

    /// Get the current value of the attribute.
    pub fn value(&self) -> u32 {
        match self {
            Attribute::BadArchAction(action) => (*action).into(),
            Attribute::NoNewPrivileges(set) => *set as u32,
            Attribute::ThreadSync(set) => *set as u32,
            Attribute::NegativeSyscalls(set) => *set as u32,
            Attribute::Log(set) => *set as u32,
            Attribute::DisableSSB(set) => *set as u32,
            Attribute::Optimize(strategy) => match strategy {
                OptimizeStrategy::PriorityAndComplexity => 1,
                OptimizeStrategy::BinaryTree => 2,
            },
            Attribute::ReturnSystemReturnCodes(set) => *set as u32,
        }
    }

    /// Get a string value for the attribute
    pub fn str(&self) -> &'static str {
        match self {
            Attribute::BadArchAction(_) => "Bad Arch Action",
            Attribute::NoNewPrivileges(_) => "No New Privileges",
            Attribute::ThreadSync(_) => "Thread Sync",
            Attribute::NegativeSyscalls(_) => "Negative Syscalls",
            Attribute::Log(_) => "Log",
            Attribute::DisableSSB(_) => "Disable SSB",
            Attribute::Optimize(_) => "Optimize",
            Attribute::ReturnSystemReturnCodes(_) => "Return System Return Codes",
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
