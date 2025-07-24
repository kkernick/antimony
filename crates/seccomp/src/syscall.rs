//! Helper functions wrapper the seccomp crate.
use super::raw;
use crate::get_architecture;
use nix::libc::free;
use std::ffi::{CStr, CString, c_int, c_void};

/// An error trying to resolve a syscall, either from string to number, or number to string.
#[derive(Debug)]
pub enum Error {
    Name(String),
    Code(c_int),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Name(name) => write!(f, "Failed to resolve syscall name: {name}"),
            Error::Code(code) => write!(f, "Failed to resolve syscall name: {code}"),
        }
    }
}
impl std::error::Error for Error {}

/// A Syscall, which can be constructed from either the number, or from the name.
#[derive(Debug, Copy, Clone)]
pub struct Syscall {
    /// The architecture specific code.
    code: c_int,
}
impl Syscall {
    /// Construct a Syscall from the number. No validation is performed.
    pub fn from_number(code: c_int) -> Self {
        Self { code }
    }

    /// Construct a Syscall from a name, returning Error if libseccomp could not resolve the name.
    pub fn from_name(name: &str) -> Result<Self, Error> {
        if let Ok(c_name) = CString::new(name) {
            match unsafe { raw::seccomp_syscall_resolve_name(c_name.as_ptr()) } {
                -1 => Err(Error::Name(name.to_string())),
                code => Ok(Self { code }),
            }
        } else {
            Err(Error::Name(name.to_string()))
        }
    }

    /// Resolve a name to a syscall number for a specific architecture.
    /// Fails if libseccomp could not resolve the name.
    pub fn with_arch(name: &str, arch: u32) -> Result<Self, Error> {
        if let Ok(c_name) = CString::new(name) {
            match unsafe { raw::seccomp_syscall_resolve_name_arch(arch, c_name.as_ptr()) } {
                -1 => Err(Error::Name(name.to_string())),
                code => Ok(Self { code }),
            }
        } else {
            Err(Error::Name(name.to_string()))
        }
    }

    /// Get the name for a syscall on the native architecture.
    pub fn get_name(num: c_int) -> Result<String, Error> {
        Self::get_name_arch(num, get_architecture())
    }

    /// Get the name for a syscall on the provided architecture.
    pub fn get_name_arch(num: c_int, arch: u32) -> Result<String, Error> {
        let name = unsafe { raw::seccomp_syscall_resolve_num_arch(arch, num) };

        if name.is_null() {
            Err(Error::Code(num))
        } else {
            let syscall_name = unsafe {
                let c_str = CStr::from_ptr(name);
                if let Ok(result) = c_str.to_str() {
                    let result = result.to_owned();
                    free(name as *mut c_void);
                    result
                } else {
                    return Err(Error::Code(num));
                }
            };
            Ok(syscall_name)
        }
    }

    /// Get the numerical value of the syscall.
    pub fn get_number(&self) -> i32 {
        self.code
    }
}
impl From<Syscall> for c_int {
    fn from(syscall: Syscall) -> c_int {
        syscall.code
    }
}
impl std::fmt::Display for Syscall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code)
    }
}
