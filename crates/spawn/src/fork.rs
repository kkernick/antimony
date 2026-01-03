//! This file contains a highly experimental interface that runs Rust closures
//! within new processes.
//!
//! For all intents and purposes, you probably shouldn't use this. If you need
//! to run native rust code, you're better using a thread. If you need to run
//! in a separate process, you're probably better writing a separate binary
//! and launching it via a `Spawner`.
//!
//! This file exists mostly as a proof-of-concept, as the original use is
//! no longer required. It serves the niche use case where:
//!
//! * You need to run native code, but without the privilege of the process.
//! * You want to run native, untrusted code under a SECCOMP policy.
//! * You're SetUID and need to drop privilege for a function.
//!
//! This was originally designed for the `notify` crate, where when running
//! underneath Antimony, its SetUID privilege was causing the User Bus
//! to refuse a connection. This functionality allowed the Connection code
//! to be run after `user::drop`. However, Antimony pivoted toward just
//! using a binary due to the unsafe nature of this functionality.
//!
//! Chiefly, you're only allowed to run async-safe functions within a fork,
//! and are not allowed to make any allocations. That severely restricts
//! the kinds of things you can do with this.

use crate::{HandleError, SpawnError, Stream, StreamMode, clear_capabilities, cond_pipe};
use caps::{Capability, CapsHashSet};
use common::stream::receive_fd;
use core::fmt;
use log::warn;
use nix::{
    sys::{
        prctl,
        signal::{self, SigHandler, Signal},
        socket::{self, ControlMessage, MsgFlags},
    },
    unistd::{ForkResult, close},
};
use std::{
    error,
    io::{IoSlice, Write},
    os::{
        fd::{AsRawFd, IntoRawFd, OwnedFd},
        unix::net::{UnixListener, UnixStream},
    },
    panic::UnwindSafe,
    process::exit,
    thread::sleep,
    time::Duration,
};

#[cfg(feature = "seccomp")]
use {parking_lot::Mutex, seccomp::filter::Filter};

/// Errors related to Fork
#[derive(Debug)]
pub enum Error {
    /// Errors preparing the fork
    Spawn(SpawnError),

    /// Errors communicating with the fork
    Handle(HandleError),

    /// Errors serializing the return data.
    Postcard(postcard::Error),

    /// Generic IO errors.
    Io(std::io::Error),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Spawn(e) => write!(f, "Failure creating fork: {e}"),
            Self::Handle(e) => write!(f, "Failure communicating with fork: {e}"),
            Self::Postcard(e) => write!(f, "Serialization/Deserialization error: {e}"),
            Self::Io(e) => write!(f, "Failed to send FD: {e}"),
        }
    }
}
impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Spawn(e) => Some(e),
            Self::Handle(e) => Some(e),
            Self::Postcard(e) => Some(e),
            Self::Io(e) => Some(e),
        }
    }
}
impl From<SpawnError> for Error {
    fn from(value: SpawnError) -> Self {
        Self::Spawn(value)
    }
}
impl From<HandleError> for Error {
    fn from(value: HandleError) -> Self {
        Self::Handle(value)
    }
}
impl From<postcard::Error> for Error {
    fn from(value: postcard::Error) -> Self {
        Self::Postcard(value)
    }
}
impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

/// A `Spawner`-like structure that executes a closure instead of another process. Specifically,
/// it forks the current caller, runs the closure within the child, then serializes and returns
/// the result to the parent via a pipe.
///
/// Your return type, if one exists, must implement Serialize + Deserialize from `serde`, as the
/// closure is run under a separate process (the child). This means that "returning" the result
/// requires serializing the result and sending it back to the parent. This operates identically
/// to how standard Input/Output/Error is captured in Spawn.
#[derive(Default)]
pub struct Fork {
    /// The User to run the program under.
    #[cfg(feature = "user")]
    mode: Option<user::Mode>,

    /// An optional *SECCOMP* policy to load on the child.
    #[cfg(feature = "seccomp")]
    seccomp: Mutex<Option<Filter>>,

    /// Don't clear privileges.
    no_new_privileges: bool,

    /// Whitelisted capabilities.
    whitelist: CapsHashSet,
}
impl Fork {
    /// Construct a new fork instance.
    pub fn new() -> Self {
        Self::default()
    }

    /// See `Spawner::mode`
    #[cfg(feature = "user")]
    pub fn mode(mut self, mode: user::Mode) -> Self {
        self.mode_i(mode);
        self
    }

    /// See `Spawner::seccomp`
    #[cfg(feature = "seccomp")]
    pub fn seccomp(self, seccomp: Filter) -> Self {
        self.seccomp_i(seccomp);
        self
    }

    /// See `Spawner::cap`
    pub fn cap(mut self, cap: Capability) -> Self {
        self.whitelist.insert(cap);
        self
    }

    /// See `Spawner::caps`
    pub fn caps(mut self, caps: impl IntoIterator<Item = Capability>) -> Self {
        caps.into_iter().for_each(|cap| {
            self.whitelist.insert(cap);
        });
        self
    }

    /// See `Spawner::new_privileges`
    pub fn new_privileges(mut self, allow: bool) -> Self {
        self.no_new_privileges = !allow;
        self
    }

    /// See `Spawner::mode_i`
    #[cfg(feature = "user")]
    pub fn mode_i(&mut self, mode: user::Mode) {
        self.mode = Some(mode);
    }

    /// See `Spawner::seccomp_i`
    #[cfg(feature = "seccomp")]
    pub fn seccomp_i(&self, seccomp: Filter) {
        *self.seccomp.lock() = Some(seccomp)
    }

    /// See `Spawner::cap_i`
    pub fn cap_i(&mut self, cap: Capability) {
        self.whitelist.insert(cap);
    }

    /// See `Spawner::caps_i`
    pub fn caps_i(&mut self, caps: impl IntoIterator<Item = Capability>) {
        caps.into_iter().for_each(|cap| {
            self.whitelist.insert(cap);
        });
    }

    /// See `Spawner::new_privileges_i`
    pub fn new_privileges_i(mut self, allow: bool) {
        self.no_new_privileges = !allow;
    }

    /// Run a closure within a fork.
    ///
    /// ## Example
    ///
    /// ```rust
    /// let result = unsafe { spawn::Fork::new().fork(|| 1) }.unwrap();
    /// assert!(result == 1);
    /// ```
    ///
    /// ## Safety
    ///
    /// This function does not call execve. The closure runs
    /// within the fork, which has several considerations:
    ///
    /// 1.  The code should not make allocations. Though the default
    ///     memory allocator on Linux often works in such an environment,
    ///     it should not be relied upon.
    /// 2.  If you had a signal handler installed, this tries and drops
    ///     all of them. Other such primitives may fail at any point, and
    ///     should not be relied upon.
    ///
    /// ***
    ///
    /// If your closure returns a value, it must implement Serialize, as the
    /// closure is running under a separate process, and must be transmitted
    /// to the parent through a pipe.
    ///
    ///
    #[allow(dead_code)]
    pub unsafe fn fork<F, R>(self, op: F) -> Result<R, Error>
    where
        F: FnOnce() -> R + UnwindSafe,
        R: serde::Serialize + serde::de::DeserializeOwned,
    {
        // Get a pipe to transmit the return value
        let (read, write) = cond_pipe(&StreamMode::Pipe)?.unwrap();
        let all = caps::all();
        let diff: CapsHashSet = all.difference(&self.whitelist).copied().collect();

        // Prepare the filter.
        #[cfg(feature = "seccomp")]
        let filter = {
            let mut filter = self.seccomp.into_inner();
            if let Some(filter) = &mut filter {
                filter.setup().map_err(SpawnError::Seccomp)?;
            }
            filter
        };

        let fork = unsafe { nix::unistd::fork() }.map_err(SpawnError::Fork)?;
        match fork {
            ForkResult::Parent { child: _child } => {
                // The parent reads from the pipe, then deserializes the bytes.
                close(write).map_err(|e| SpawnError::Errno(Some(fork), "close write", e))?;
                let stream = Stream::new(read);
                let bytes = stream.read_bytes(None)?;
                Ok(postcard::from_bytes(&bytes)?)
            }

            ForkResult::Child => {
                // Prepare signals.
                let _ = prctl::set_pdeathsig(signal::SIGTERM);
                for sig in Signal::iterator() {
                    unsafe {
                        let _ = signal::signal(sig, SigHandler::SigDfl);
                    }
                }

                // Drop modes
                #[cfg(feature = "user")]
                if let Some(mode) = self.mode {
                    let _ = user::drop(mode);
                }

                // Drop capabilities and privileges
                clear_capabilities(diff);
                if self.no_new_privileges
                    && let Err(e) = prctl::set_no_new_privs()
                {
                    warn!("Could not set NO_NEW_PRIVS: {e}");
                }

                // Apply SECCOMP.
                #[cfg(feature = "seccomp")]
                if let Some(filter) = filter {
                    filter.load();
                }

                // Execute the closure, send the serialized result to the parent.
                if std::panic::catch_unwind(|| {
                    close(read).expect("Failed to close read");
                    let result = op();
                    let bytes = postcard::to_allocvec(&result).expect("Failed to serialize");
                    let mut file = std::fs::File::from(write);
                    file.write_all(&bytes).expect("Failed to write bytes");
                    file.flush().expect("Failed to flush write");
                })
                .is_err()
                {
                    exit(1)
                } else {
                    exit(0)
                }
            }
        }
    }

    /// This is a specialized version of fork() that uses the SCM-Rights of a
    /// Unix Socket to transmit a FD to the parent. This could be used to open a file
    /// under one operating mode, and send the FD to the parent under another
    /// operating mode.
    ///
    /// ## Safety
    ///
    /// See fork()
    #[allow(dead_code)]
    pub unsafe fn fork_fd<F, R>(self, op: F) -> Result<OwnedFd, Error>
    where
        F: FnOnce() -> R + UnwindSafe,
        R: Into<OwnedFd>,
    {
        let socket_path = temp::Builder::new().make(false).create::<temp::File>()?;
        let all = caps::all();
        let diff: CapsHashSet = all.difference(&self.whitelist).copied().collect();

        #[cfg(feature = "seccomp")]
        let filter = {
            let mut filter = self.seccomp.into_inner();
            if let Some(filter) = &mut filter {
                filter.setup().map_err(SpawnError::Seccomp)?;
            }
            filter
        };

        let fork = unsafe { nix::unistd::fork() }.map_err(SpawnError::Fork)?;
        match fork {
            ForkResult::Parent { child: _child } => {
                let listener = UnixListener::bind(socket_path.full())?;
                if let Some((fd, _)) = receive_fd(&listener)? {
                    Ok(fd)
                } else {
                    Err(Error::Io(std::io::ErrorKind::InvalidData.into()))
                }
            }

            ForkResult::Child => {
                let _ = prctl::set_pdeathsig(Signal::SIGTERM);
                for sig in Signal::iterator() {
                    unsafe {
                        let _ = signal::signal(sig, SigHandler::SigDfl);
                    }
                }

                // Drop modes
                #[cfg(feature = "user")]
                if let Some(mode) = self.mode {
                    let _ = user::drop(mode);
                }

                clear_capabilities(diff);
                if self.no_new_privileges
                    && let Err(e) = prctl::set_no_new_privs()
                {
                    warn!("Could not set NO_NEW_PRIVS: {e}");
                }

                while !socket_path.full().exists() {
                    sleep(Duration::from_millis(10));
                }

                let stream = UnixStream::connect(socket_path.full())?;

                // Apply SECCOMP.
                #[cfg(feature = "seccomp")]
                if let Some(filter) = filter {
                    filter.load();
                }

                if std::panic::catch_unwind(|| {
                    let fd: OwnedFd = op().into();
                    let raw_fd = stream.as_raw_fd();
                    let name_bytes = b"fork";
                    let io = [IoSlice::new(name_bytes)];
                    let fds = [fd.into_raw_fd()];
                    let msgs = [ControlMessage::ScmRights(&fds)];
                    socket::sendmsg::<()>(raw_fd, &io, &msgs, MsgFlags::empty(), None)
                        .expect("Failed to send the FD");
                })
                .is_err()
                {
                    exit(1)
                } else {
                    exit(0)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use std::io::Read;

    #[test]
    fn number() -> Result<()> {
        let result = unsafe { Fork::new().fork(|| 1) }?;
        assert!(result == 1);
        Ok(())
    }

    #[test]
    fn string() -> Result<()> {
        let str = "This is a test!".to_string();
        let result = unsafe { crate::Fork::new().fork(|| str.clone()) }?;
        assert!(result == str);
        Ok(())
    }

    #[test]
    fn file() -> Result<()> {
        let path = "/tmp/test";
        let str = "Hello, world!";
        let mut file: std::fs::File = unsafe {
            crate::Fork::new().fork_fd(|| {
                let mut file = std::fs::File::create(path).expect("Failed to create temp");
                writeln!(file, "{}", str).expect("Failed to write file");
                drop(file);
                std::fs::File::open(path).expect("Failed to open temp")
            })
        }?
        .into();

        let mut result = String::new();
        file.read_to_string(&mut result)?;
        drop(file);
        std::fs::remove_file(path)?;
        assert!(result.trim_matches('\n') == str);
        Ok(())
    }
}
