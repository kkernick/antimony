#![doc = include_str!("../README.md")]

use common::singleton::Singleton;
use nix::{
    errno::Errno,
    unistd::{ResGid, ResUid, getresgid, getresuid, setresgid, setresuid},
};
use std::{error, fmt, sync::LazyLock};

/// The Real, Effective, and Saved UID of the application.
pub static USER: LazyLock<ResUid> = LazyLock::new(|| getresuid().expect("Failed to get UID!"));

/// The Real, Effective, and Saved GID of the application.
pub static GROUP: LazyLock<ResGid> = LazyLock::new(|| getresgid().expect("Failed to get GID!"));

/// Whether the system is actually running under SetUid. If false, all functions here
/// are no-ops.
pub static SETUID: LazyLock<bool> = LazyLock::new(|| USER.effective != USER.real);

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

    /// The current operating mode. This is functionally a no-op except for
    /// in drop, where it drops whatever the current mode happens to be.
    Existing,

    /// Revert to the program's original operating mode. For set, this
    /// mode is functionally identical to using revert(). For drop, it
    /// acts as user::revert().
    Original,
}

/// Set the Mode.
/// This function can never be misused to lock out the process
/// from returning to the original, or any other mode.
/// This function returns an error if the mode could not be
/// changed. Otherwise, it returns the old mode for use in
/// `restore()`.
///
/// ## Examples
/// ```rust
/// user::set(user::Mode::Real).unwrap();
/// ```
///
/// ## Notes
///
/// * user::set(Mode::Original) is functionally identical to user::revert()
/// * user::set(Mode::Existing) is a no-op.
///
/// ## Thread Safety
///
/// This function is not thread safe. Multiple threads can change the state
/// using this function. If you need to ensure that everything executed between
/// a `set()` and `restore()` block is run under the desired user, use `run_as` or
/// its mode-specific variants.
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
        Mode::Original => revert()?,
        Mode::Existing => {}
    }

    Ok((uid, gid))
}

/// Get the current user mode
/// Note that this is not thread-safe, and your program can suffer
/// from TOC-TOU problems if you assume this value will remain the same
/// when you actually need to perform a privileged operation in a multi-threaded
/// environment.
pub fn current() -> Result<Mode, Errno> {
    let uid = getresuid()?.real;
    if uid == USER.real {
        Ok(Mode::Real)
    } else if uid == USER.effective {
        Ok(Mode::Effective)
    } else {
        Err(Errno::EINVAL)
    }
}

/// Revert the Mode to the original.
/// This function returns to the values of `USER` and `GROUP`.
/// This function can fail if the underlying syscall does.
pub fn revert() -> Result<(), Errno> {
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
    match mode {
        Mode::Real => {
            setresuid(USER.real, USER.real, USER.real)?;
            setresgid(GROUP.real, GROUP.real, GROUP.real)
        }
        Mode::Effective => {
            setresuid(USER.effective, USER.effective, USER.effective)?;
            setresgid(GROUP.effective, GROUP.effective, GROUP.effective)
        }
        Mode::Original => revert(),
        Mode::Existing => {
            let (user, group) = (getresuid()?, getresgid()?);
            setresuid(user.real, user.real, user.real)?;
            setresgid(group.real, group.real, group.real)
        }
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

pub fn obtain_lock() -> Option<Singleton> {
    if *crate::SETUID {
        Singleton::new()
    } else {
        None
    }
}

/// This is a thread-safe wrapper that sets the mode, runs the closure/expression,
/// then returns to the mode before the call. You can use this in multi-threaded
/// environments, and it is guaranteed the content of the closure/expression will
/// be run under the requested Mode.
#[macro_export]
macro_rules! run_as {
    ($mode:path, $ret:ty, $body:block) => {{
        {
            let lock = user::obtain_lock();
            match user::set($mode) {
                Ok(__saved) => {
                    let __result = (|| -> $ret { $body })();
                    user::restore(__saved).map(|e| __result)
                }
                Err(e) => Err(e),
            }
        }
    }};

    ($mode:path, $body:block) => {{
        {
            let lock = user::obtain_lock();
            match user::set($mode) {
                Ok(__saved) => {
                    let __result = (|| $body)();
                    user::restore(__saved).map(|e| __result)
                }
                Err(e) => Err(e),
            }
        }
    }};

    ($mode:path, $expr:expr) => {{
        {
            let lock = user::obtain_lock();
            match user::set($mode) {
                Ok(__saved) => {
                    let __result = $expr;
                    user::restore(__saved).map(|e| __result)
                }
                Err(e) => Err(e),
            }
        }
    }};
}

/// Run the block/expression as the Real User. This is thread safe.
#[macro_export]
macro_rules! as_real {
    ($ret:ty, $body:block) => {{ user::run_as!(user::Mode::Real, $ret, $body) }};
    ($body:block) => {{ user::run_as!(user::Mode::Real, $body) }};
    ($expr:expr) => {{ user::run_as!(user::Mode::Real, { $expr }) }};
}

/// Run the block/expression as the Effective User. This is thread safe.
#[macro_export]
macro_rules! as_effective {
    ($ret:ty, $body:block) => {{ user::run_as!(user::Mode::Effective, $ret, $body) }};
    ($body:block) => {{ user::run_as!(user::Mode::Effective, $body) }};
    ($expr:expr) => {{ user::run_as!(user::Mode::Effective, { $expr }) }};
}
