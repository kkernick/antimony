use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use log::trace;
use once_cell::sync::Lazy;

static SEMAPHORE: Lazy<Arc<(Mutex<bool>, Condvar)>> =
    Lazy::new(|| Arc::new((Mutex::new(false), Condvar::new())));

pub struct Sync {
    sem: Arc<(Mutex<bool>, Condvar)>,
    guard: MutexGuard<'static, bool>,
}
impl Sync {
    pub fn new() -> Self {
        let sem = Arc::clone(&SEMAPHORE);
        let (mutex, cvar) = &*sem;
        let guard: MutexGuard<'static, bool> = unsafe {
            let tmp_guard = mutex.lock().expect("Poisoned mutex");
            std::mem::transmute::<MutexGuard<'_, bool>, MutexGuard<'static, bool>>(tmp_guard)
        };
        let mut guard = guard;
        while *guard {
            trace!("Waiting for lock");
            guard = cvar.wait(guard).expect("Waiting failed");
        }
        trace!("Lock acquired");
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
        trace!("Dropping lock");
        *self.guard = false;
        let (_, cvar) = &*self.sem;
        cvar.notify_one();
    }
}

#[macro_export]
macro_rules! sync_run_as {
    ($mode:path, $ret:ty, $body:block) => {{
        let lock = user::sync::Sync::new();
        let __saved = user::save().expect("Failed to save user mode");
        user::set($mode).expect("Failed to set user mode");
        let __result: $ret = (|| -> $ret { $body })();
        user::restore(__saved).expect("Failed to restore user mode");
        drop(lock);
        __result
    }};

    ($mode:path, $ret:ty, $expr:expr) => {{
        let lock = user::sync::Sync::new();
        let __saved = user::save().expect("Failed to save user mode");
        ::user::set($mode).expect("Failed to set user mode");
        let __result: $ret = (|| -> $ret { $expr })();
        user::restore(__saved).expect("Failed to restore user mode");
        drop(lock);
        __result
    }};

    ($mode:path, $body:block) => {{
        let lock = user::sync::Sync::new();
        let __saved = user::save().expect("Failed to save user mode");
        user::set($mode).expect("Failed to set user mode");
        let __result = (|| $body)();
        user::restore(__saved).expect("Failed to restore user mode");
        drop(lock);
        __result
    }};

    ($mode:path, $expr:expr) => {{
        let lock = user::sync::Sync::new();
        let __saved = user::save().expect("Failed to save user mode");
        user::set($mode).expect("Failed to set user mode");
        let __result = $expr;
        user::restore(__saved).expect("Failed to restore user mode");
        drop(lock);
        __result
    }};
}
pub use sync_run_as as run_as;
