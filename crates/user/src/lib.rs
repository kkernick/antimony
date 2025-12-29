//! Helper utilities for switching modes in SetUID applications.

use nix::{
    errno::Errno,
    unistd::{ResGid, ResUid, getresgid, getresuid, setresgid, setresuid},
};
use parking_lot::{
    Condvar, Mutex, MutexGuard, RawMutex, RawThreadId, ReentrantMutex,
    lock_api::ReentrantMutexGuard,
};
use std::{
    error, fmt,
    sync::{Arc, LazyLock},
};

/// The Real, Effective, and Saved UID of the application.
pub static USER: LazyLock<ResUid> = LazyLock::new(|| getresuid().expect("Failed to get UID!"));

/// The Real, Effective, and Saved GID of the application.
pub static GROUP: LazyLock<ResGid> = LazyLock::new(|| getresgid().expect("Failed to get GID!"));

/// Whether the system is actually running under SetUid. If false, all functions here
/// are no-ops.
pub static SETUID: LazyLock<bool> = LazyLock::new(|| USER.effective != USER.real);

/// The global semaphore controls which thread is allowed to change users.
static SEMAPHORE: LazyLock<Semaphore> =
    LazyLock::new(|| Arc::new((ReentrantMutex::new(()), Mutex::new(false), Condvar::new())));

type Semaphore = Arc<(ReentrantMutex<()>, Mutex<bool>, Condvar)>;
type Guard = MutexGuard<'static, bool>;
type ThreadGuard = ReentrantMutexGuard<'static, RawMutex, RawThreadId, ()>;

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

/// Get the current user mode
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

/// A synchronization primitive.
/// Only one thread will ever have a Sync object. When the Sync object
/// drops, control is relinquished to another thread.
///
/// Though this object is designed with this crate in mind, there's nothing
/// implementation-specific to this object; you could use it for any logic
/// that requires something along the following:
///
/// ```rust
/// let lock = Sync::new();
/// // Do things
/// drop(lock);
/// ```
///
///
/// Note that the lock automatically relinquishes control on drop, or when
/// falling out of scope.
pub struct Sync {
    sem: Semaphore,
    guard: Guard,
    _thread_guard: ThreadGuard,
}
impl Sync {
    /// Take ownership of the shared semaphore.
    /// This function is blocking.
    pub fn new() -> Option<Self> {
        let sem = Arc::clone(&SEMAPHORE);
        let (thread_lock, mutex, cvar) = &*sem;

        if thread_lock.is_owned_by_current_thread() {
            log::trace!("Already owned by current thread. Stepping past.");
            return None;
        }

        let mut guard: Guard = unsafe {
            let tmp_guard = mutex.lock();
            std::mem::transmute::<MutexGuard<'_, bool>, Guard>(tmp_guard)
        };
        while *guard {
            cvar.wait(&mut guard);
        }

        let _thread_guard: ThreadGuard = unsafe {
            let tmp_guard = thread_lock.lock();
            std::mem::transmute::<ReentrantMutexGuard<'_, RawMutex, RawThreadId, ()>, ThreadGuard>(
                tmp_guard,
            )
        };

        *guard = true;
        Some(Self {
            sem,
            guard,
            _thread_guard,
        })
    }
}
impl Drop for Sync {
    fn drop(&mut self) {
        *self.guard = false;
        let (_, _, cvar) = &*self.sem;
        cvar.notify_one();
    }
}

pub fn obtain_lock() -> Option<Sync> {
    if *crate::SETUID { Sync::new() } else { None }
}

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

#[macro_export]
macro_rules! as_real {
    ($ret:ty, $body:block) => {{ user::run_as!(user::Mode::Real, $ret, $body) }};
    ($body:block) => {{ user::run_as!(user::Mode::Real, $body) }};
    ($expr:expr) => {{ user::run_as!(user::Mode::Real, { $expr }) }};
}

#[macro_export]
macro_rules! as_effective {
    ($ret:ty, $body:block) => {{ user::run_as!(user::Mode::Effective, $ret, $body) }};
    ($body:block) => {{ user::run_as!(user::Mode::Effective, $body) }};
    ($expr:expr) => {{ user::run_as!(user::Mode::Effective, { $expr }) }};
}
