//! Helper functions wrapper the seccomp crate.
use super::raw;
use crate::get_architecture;
use nix::libc::free;
use std::{
    error,
    ffi::{CStr, CString, c_int, c_void},
    fmt,
};

/// An error trying to resolve a syscall, either from string to number, or number to string.
#[derive(Debug)]
pub enum Error {
    Name(String),
    Code(c_int),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name(name) => write!(f, "Failed to resolve syscall name: {name}"),
            Self::Code(code) => write!(f, "Failed to resolve syscall name: {code}"),
        }
    }
}
impl error::Error for Error {}

/// A Syscall, which can be constructed from either the number, or from the name.
#[derive(Debug, Copy, Clone)]
pub struct Syscall {
    /// The architecture specific code.
    code: c_int,
}
impl Syscall {
    /// Construct a Syscall from the number. No validation is performed.
    #[must_use]
    pub const fn from_number(code: c_int) -> Self {
        Self { code }
    }

    /// Construct a Syscall from a name, returning Error if libseccomp could not resolve the name.
    ///
    /// ## Errors
    /// `Error::Name`: If `seccomp_syscall_resolve_name` fails, or the provided string cannot be converted
    ///                to a `CString`.
    pub fn from_name(name: &str) -> Result<Self, Error> {
        CString::new(name).map_or_else(
            |_| Err(Error::Name(name.to_owned())),
            |c_name| match unsafe { raw::seccomp_syscall_resolve_name(c_name.as_ptr()) } {
                -1 => Err(Error::Name(name.to_owned())),
                code => Ok(Self { code }),
            },
        )
    }

    /// Resolve a name to a syscall number for a specific architecture.
    /// Fails if libseccomp could not resolve the name.
    ///
    /// ## Errors
    /// `Error::Name`: If `seccomp_syscall_resolve_name` fails, or the provided string cannot be converted
    ///                to a `CString`.
    pub fn with_arch(name: &str, arch: u32) -> Result<Self, Error> {
        CString::new(name).map_or_else(
            |_| Err(Error::Name(name.to_owned())),
            |c_name| match unsafe { raw::seccomp_syscall_resolve_name_arch(arch, c_name.as_ptr()) }
            {
                -1 => Err(Error::Name(name.to_owned())),
                code => Ok(Self { code }),
            },
        )
    }

    /// Get the name for a syscall on the native architecture.
    ///
    /// ## Errors
    /// `Error::Code`: If `seccomp_syscall_resolve_num_arch` fails, or the syscall
    ///                cannot be converted to Unicode.
    pub fn get_name(num: c_int) -> Result<String, Error> {
        Self::get_name_arch(num, get_architecture())
    }

    /// Get the name for a syscall on the provided architecture.
    ///
    /// ## Errors
    /// `Error::Code`: If `seccomp_syscall_resolve_num_arch` fails, or the syscall
    ///                cannot be converted to Unicode.
    pub fn get_name_arch(num: c_int, arch: u32) -> Result<String, Error> {
        let name = unsafe { raw::seccomp_syscall_resolve_num_arch(arch, num) };

        if name.is_null() {
            Err(Error::Code(num))
        } else {
            let syscall_name = unsafe {
                let c_str = CStr::from_ptr(name);
                if let Ok(result) = c_str.to_str() {
                    let result = result.to_owned();
                    free(name.cast::<c_void>());
                    result
                } else {
                    return Err(Error::Code(num));
                }
            };
            Ok(syscall_name)
        }
    }

    /// Get the numerical value of the syscall.
    #[must_use]
    pub const fn get_number(&self) -> i32 {
        self.code
    }
}
impl From<Syscall> for c_int {
    fn from(syscall: Syscall) -> Self {
        syscall.code
    }
}
impl fmt::Display for Syscall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code)
    }
}
