//! Helper utilities for switching modes in SetUID applications.

use nix::{
    errno::Errno,
    unistd::{ResGid, ResUid, getresgid, getresuid, setresgid, setresuid},
};
use once_cell::sync::Lazy;
use std::{error, fmt};

pub mod sync;

/// The Real, Effective, and Saved UID of the application.
pub static USER: Lazy<ResUid> = Lazy::new(|| getresuid().expect("Failed to get UID!"));

/// The Real, Effective, and Saved GID of the application.
pub static GROUP: Lazy<ResGid> = Lazy::new(|| getresgid().expect("Failed to get GID!"));

/// Whether the system is actually running under SetUid. If false, all functions here
/// are no-ops.
pub static SETUID: Lazy<bool> = Lazy::new(|| USER.effective != USER.real);

/// An error when trying to change UID/GID.
#[derive(Debug)]
pub struct Error {
    /// The UID we were trying to change to
    uid: ResUid,

    /// The error we got from the syscall.
    errno: Errno,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let current = getresgid();
        if let Ok(uid) = current {
            write!(
                f,
                "Failed to change UID from: ({}, {}, {}) to ({}, {}, {})",
                uid.real,
                uid.effective,
                uid.saved,
                self.uid.real,
                self.uid.effective,
                self.uid.saved
            )
        } else {
            write!(
                f,
                "Failed to change UID to ({}, {}, {})",
                self.uid.real, self.uid.effective, self.uid.saved
            )
        }
    }
}
impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        Some(&self.errno as &dyn error::Error)
    }
}

/// A SetUID mode.
#[derive(Debug, Copy, Clone)]
pub enum Mode {
    /// Transition to the Real user, setting both Real and Effective
    /// to `USER.real`, while saving Effective to Saved.
    Real,

    /// Transition to the Effective user, setting both Real and Effective
    /// to `USER.effective`, while saving Real to Saved.
    Effective,

    /// Revert to the original UID/GID, with Real, Effective, and Saved
    /// returning to their initial values.
    Existing,
}

/// Set the Mode.
/// This function can never be misused to lock out the process
/// from returning to the original, or any other mode.
/// This function returns an error if the mode could not be
/// changed. Otherwise, it returns the old mode for use in
/// `restore()`
///
/// ## Examples
/// ```rust
/// user::set(user::Mode::Real).unwrap();
/// ```
pub fn set(mode: Mode) -> Result<(ResUid, ResGid), Errno> {
    if !*SETUID {
        return Ok((*USER, *GROUP));
    }

    let uid = getresuid()?;
    let gid = getresgid()?;

    match mode {
        Mode::Real => {
            setresuid(USER.real, USER.real, USER.effective)?;
            setresgid(GROUP.real, GROUP.real, GROUP.effective)?;
        }
        Mode::Effective => {
            setresuid(USER.effective, USER.effective, USER.real)?;
            setresgid(GROUP.effective, GROUP.effective, GROUP.real)?;
        }
        Mode::Existing => revert()?,
    }

    Ok((uid, gid))
}

/// Revert the Mode to the original.
/// This function returns to the values of `USER` and `GROUP`.
/// This function can fail if the underlying syscall does.
pub fn revert() -> Result<(), Errno> {
    if !*SETUID {
        return Ok(());
    }

    setresuid(USER.real, USER.effective, USER.saved)?;
    setresgid(GROUP.real, GROUP.effective, GROUP.saved)
}

/// Destructively change mode, preventing the process from returning.
/// This function will set Real, Effective, and Saved values to the
/// desired Mode. This prevents the process from changing their mode
/// ## Examples
/// ```rust
/// user::drop(user::Mode::Real).unwrap();
/// // This will only fail if we were SetUID.
/// if user::USER.real != user::USER.effective {
///     user::set(user::Mode::Effective).expect_err("Cannot return!");
/// }
/// ```
pub fn drop(mode: Mode) -> Result<(), Errno> {
    if !*SETUID {
        return Ok(());
    }

    match mode {
        Mode::Real => {
            setresuid(USER.real, USER.real, USER.real)?;
            setresgid(GROUP.real, GROUP.real, GROUP.real)
        }
        Mode::Effective => {
            setresuid(USER.effective, USER.effective, USER.effective)?;
            setresgid(GROUP.effective, GROUP.effective, GROUP.effective)
        }

        Mode::Existing => revert(),
    }
}

/// Restore a saved Uid/Gid combination
/// This function fails if the syscall does.
/// ## Examples
/// ```rust
/// let saved = user::set(user::Mode::Real).unwrap();
/// // Do work.
/// user::restore(saved).unwrap()
/// ```
pub fn restore((uid, gid): (ResUid, ResGid)) -> Result<(), Errno> {
    if !*SETUID {
        return Ok(());
    }

    setresuid(uid.real, uid.effective, uid.saved)?;
    setresgid(gid.real, gid.effective, gid.saved)
}

/// Run a particular function/lamdba under a user.
/// Accepts a Mode to switch to, then saves the current mode,
/// reverting at the end of execution. This function, therefore,
/// does not change the existing mode after returning.
///
/// This macro can only be used in the context of a function returning
/// a Result, as errors in saving/setting/restoring are propagated. If
/// you can't return a Result, use `run_as`
#[macro_export]
macro_rules! try_run_as {
    ($mode:path, $ret:ty, $body:block) => {{
        let __saved = user::set($mode)?;
        let __result: $ret = (|| -> $ret { $body })();
        user::restore(__saved)?;
        __result
    }};

    ($mode:path, $body:block) => {{
        let __saved = user::set($mode)?;
        let __result = (|| $body)();
        user::restore(__saved)?;
        __result
    }};

    ($mode:path, $expr:expr) => {{
        let __saved = user::set($mode)?;
        let __result = $expr;
        user::restore(__saved)?;
        __result
    }};
}

/// This macro is functionally equivalent to `try_run_as`, but will panic
/// the program with `expect()` rather than returning an Err on failure
/// to save/set/restore the user mode.
///
/// Note that due to how these functions work (Relying on the original
/// Real/Effective/Saved values), there is no situation in which
/// these modes will actually fail save the kernel being unable
/// to allocate (Or, in other words, never).
#[macro_export]
macro_rules! run_as {
    ($mode:path, $ret:ty, $body:block) => {{
        let __saved = user::set($mode).expect("Failed to set user mode");
        let __result: $ret = (|| -> $ret { $body })();
        user::restore(__saved).expect("Failed to restore user mode");
        __result
    }};

    ($mode:path, $body:block) => {{
        let __saved = user::set($mode).expect("Failed to set user mode");
        let __result = (|| $body)();
        user::restore(__saved).expect("Failed to restore user mode");
        __result
    }};

    ($mode:path, $expr:expr) => {{
        let __saved = user::set($mode).expect("Failed to set user mode");
        let __result = $expr;
        user::restore(__saved).expect("Failed to restore user mode");
        __result
    }};
}
