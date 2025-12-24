//! This namespace includes a thread-safe run_as! implementation.
//! If multiple threads are running, and they change the operating mode,
//! it is possible (and in fact, likely) that one thread will change
//! the operating mode from underneath another thread within a typical
//! save-set-restore block. This means that the standard user::* functions
//! provide no guarantee that the mode will remain the same between set and
//! restore if multiple threads are running.
//!
//! The functionality in this file provides that guarantee. sync_run_as
//! will obtain exclusive control over the operating mode, which guarantees
//! that all code within the block will be executing under the provided
//! mode.
//!
//! There are caveats to using this implementation, which should be heeded,
//! as improper usage can void this guarantee, or cause deadlocks.
//!     1.  The guarantee is only valid for other threads using the sync implementation.
//!         If another thread uses the standard user::* functions directly, they will
//!         void thread-safety. This doesn't mean the two modes cannot be mixed, just
//!         ensure that the standard functions are only used when no other thread using
//!         sync function is also running, and relying on its guarantee.
//!     2. sync::run_ascannot be nested. If it is run within an existing sync::run_as
//!         block, the program will deadlock.
//!
//! A crucial thing to understand is that sync_run_as does not need to be used in any
//! program that is multi-threaded. In fact, naively switching from run_as to the
//! synchronized variant will invariably deadlock the program. It is *ONLY* useful in
//! situations where multiple threads will be changing the mode at once, and thus
//! the program need a guarantee that the mode will not change between set and restore. For
//! example:
//!     *   spawn::Handle uses a synchronized run_as when a mode has been set on its drop
//!         function. This is because multiple handles may be dropped at once, and it must
//!         ensure that the signal can be sent before another changes the mode.
//!     *   antimony-monitor uses a synchronized run_as when committing to the database
//!         from a worker thread, as multiple such workers may be running and committing
//!         at the same time.
//!
//! Remember that regular run_as will still restore the user environment after its completed.
//! This implementation only protects the interior of the macro.
#![cfg(feature = "sync")]

use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard};

/// The global semaphore controls which thread is allowed to change users.
static SEMAPHORE: LazyLock<Arc<(Mutex<bool>, Condvar)>> =
    LazyLock::new(|| Arc::new((Mutex::new(false), Condvar::new())));

/// A synchronization primitive.
/// Only one thread will ever have a Sync object. When the Sync object
/// drops, control is relinquished to another thread.
///
/// Though this object is designed with this crate in mind, there's nothing
/// implementation-specific to this object; you could use it for any logic
/// that requires something along the following:
///
/// ```rust
/// let lock = user::sync::Sync::new();
/// // Do things
/// drop(lock);
/// ```
///
///
/// Note that the lock automatically relinquishes control on drop, or when
/// falling out of scope.
pub struct Sync {
    sem: Arc<(Mutex<bool>, Condvar)>,
    guard: MutexGuard<'static, bool>,
}
impl Sync {
    /// Take ownership of the shared semaphore.
    /// This function is blocking.
    pub fn new() -> Self {
        let sem = Arc::clone(&SEMAPHORE);
        let (mutex, cvar) = &*sem;
        let mut guard: MutexGuard<'static, bool> = unsafe {
            let tmp_guard = mutex.lock().expect("Sync poisoned!");
            std::mem::transmute::<MutexGuard<'_, bool>, MutexGuard<'static, bool>>(tmp_guard)
        };
        while *guard {
            guard = cvar.wait(guard).expect("Sync poisoned!");
        }
        *guard = true;
        Self { sem, guard }
    }
}
impl Default for Sync {
    fn default() -> Self {
        Self::new()
    }
}
impl Drop for Sync {
    fn drop(&mut self) {
        *self.guard = false;
        let (_, cvar) = &*self.sem;
        cvar.notify_one();
    }
}

pub fn obtain_lock() -> Option<Sync> {
    if *crate::SETUID {
        Some(Sync::new())
    } else {
        None
    }
}

/// Synchronized run_as.
/// See the doc comment in user::sync.rs
#[macro_export]
macro_rules! sync_run_as {
    ($mode:path, $ret:ty, $body:block) => {{
        {
            let lock = user::sync::obtain_lock();
            user::run_as!($mode, || -> $ret { $body }())
        }
    }};

    ($mode:path, $body:block) => {{
        {
            let lock = user::sync::obtain_lock();
            user::run_as!($mode, $body)
        }
    }};

    ($mode:path, $expr:expr) => {{
        {
            let lock = user::sync::obtain_lock();
            user::run_as!($mode, { $expr })
        }
    }};
}
pub use sync_run_as as run_as;

/// Synchronized try_run_as.
/// See the doc comment in user::sync.rs
#[macro_export]
macro_rules! sync_try_run_as {
    ($mode:path, $ret:ty, $body:block) => {{
        {
            let lock = user::sync::obtain_lock();
            user::try_run_as!($mode, || -> $ret { $body }())
        }
    }};

    ($mode:path, $body:block) => {{
        {
            let lock = user::sync::obtain_lock();
            user::try_run_as!($mode, $body)
        }
    }};

    ($mode:path, $expr:expr) => {{
        {
            let lock = user::sync::obtain_lock();
            user::try_run_as!($mode, { $expr })
        }
    }};
}
pub use sync_try_run_as as try_run_as;
