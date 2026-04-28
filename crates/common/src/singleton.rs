//! This file implements a Reentrant Synchronization Singleton, which is used to guard
//! a critical path such that only a single thread may execute it at a time. The
//! general use is:
//!
//! ```rust
//! use std::sync::LazyLock;
//! use common::singleton::Semaphore;
//!
//! static SEM: LazyLock<Semaphore> = LazyLock::new(Semaphore::default);
//!
//! // Take control of the Singleton. This is a blocking operation.
//! let lock = common::singleton::Singleton::new(&SEM);
//! // ...
//! if let Some(lock) = lock {
//!     drop(lock)
//! }
//! ```
//!
//! The primitive is Reentrant, meaning that once a thread owns the object, subsequent
//! calls do not cause recursive deadlock. The intializer will simply return None,
//! and the original MutexGuard acquired by the thread further up the call-stack
//! will remain. This means that if you have multiple critical paths which may
//! overlap, you do not need to worry about causing deadlock--the Singleton will
//! remain owned by the thread for the scope highest in the call-chain:
//!
//! ```rust
//! use std::sync::LazyLock;
//! use common::singleton::Semaphore;
//!
//! static SEM: LazyLock<Semaphore> = LazyLock::new(Semaphore::default);
//! fn critical_write() {
//!     // Acquire a lock
//!     let _lock = common::singleton::Singleton::new(&SEM);
//!     println!("Rust already ensures only a single thread can write here, but we're being safe ;)");
//!
//!     // Because we already have the Singleton in this thread, this instance will be none. The MutexGuard
//!     // is held by the parent.
//!     assert!(_lock.is_none())
//! }
//!
//! // Acquire a lock for our critical section.
//! let _lock = common::singleton::Singleton::new(&SEM);
//! let x = 1;
//!
//! // Write. Though we already hold an instance of the Singleton, we can safely call this from this thread.
//! critical_write();
//!
//! // The lock will drop here, allowing the entire critical path to execute without multiple acquisitions.
//! ```

use parking_lot::{Condvar, Mutex, MutexGuard, ReentrantMutex, ReentrantMutexGuard};
use std::sync::{Arc, LazyLock};

/// A Semaphore implementation. Includes A ReentrantMutex to check if the current thread owns
/// the Singleton, a regular Mutex that holds a boolean we can modify to save whether the current
/// mutex is held, and a condition variable to alert waiting threads when the Singleton is available.
pub type Semaphore = Arc<(ReentrantMutex<()>, Mutex<bool>, Condvar)>;

/// More concise Mutex Guard types.
type Guard = MutexGuard<'static, bool>;
type ThreadGuard = ReentrantMutexGuard<'static, ()>;

/// The Singleton is a Reentrant Synchronization Type that can only be held by a single thread.
pub struct Singleton {
    sem: Semaphore,
    guard: Guard,
    _thread_guard: ThreadGuard,
}
impl Singleton {
    /// Take ownership of the Singleton, blocking until it becomes available.
    /// If the current thread already owns the Singleton, this function will
    /// return None. Otherwise, it will return an instance that, when dropped,
    /// will free the Singleton for another thread.
    pub fn new(sem: &'static LazyLock<Semaphore>) -> Option<Self> {
        // Get the semaphore.
        let sem = Arc::clone(sem);
        let (thread_lock, mutex, cvar) = &*sem;

        // If we already own it, just return
        if thread_lock.is_owned_by_current_thread() {
            return None;
        }

        // Otherwise, get a guard
        let mut guard: Guard = unsafe {
            let tmp_guard = mutex.lock();
            std::mem::transmute::<MutexGuard<'_, bool>, Guard>(tmp_guard)
        };
        while *guard {
            cvar.wait(&mut guard);
        }

        // Get the thread guard as well.
        let _thread_guard: ThreadGuard = unsafe {
            let tmp_guard = thread_lock.lock();
            std::mem::transmute::<ReentrantMutexGuard<'_, ()>, ThreadGuard>(tmp_guard)
        };

        // Notify that the Singleton is owned.
        *guard = true;
        Some(Self {
            sem,
            guard,
            _thread_guard,
        })
    }
}
impl Drop for Singleton {
    fn drop(&mut self) {
        *self.guard = false;
        let (_, _, cvar) = &*self.sem;
        cvar.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::LazyLock, thread};

    #[cfg(test)]
    static SEM: LazyLock<Semaphore> = LazyLock::new(Semaphore::default);

    #[cfg(test)]
    static mut COUNTER: u64 = 0;

    #[test]
    fn test_singleton() {
        const NUM_THREADS: usize = 50;
        let mut handles = Vec::with_capacity(NUM_THREADS);

        for _ in 0..NUM_THREADS {
            let handle = thread::spawn(|| {
                let _lock = Singleton::new(&SEM).expect("Could not acquire singleton");
                unsafe {
                    COUNTER += 1;
                }
            });
            handles.push(handle);
        }

        for h in handles {
            h.join().expect("Thread panicked");
        }
        let final_value = unsafe { COUNTER };
        assert_eq!(
            final_value, NUM_THREADS as u64,
            "Counter should equal the number of threads"
        );
    }
}
