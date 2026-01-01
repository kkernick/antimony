//! This file contains a highly experimental interface that runs Rust closures
//! within new processes.

use crate::{
    HandleError, SpawnError, Stream, StreamMode,
    spawn::{clear_capabilities, cond_pipe},
};
use caps::{Capability, CapsHashSet};
use common::receive_fd;
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

#[derive(Debug)]
pub enum Error {
    Spawn(SpawnError),
    Handle(HandleError),
    Postcard(postcard::Error),
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
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop privilege to the provided user mode on the child,
    /// immediately after the fork. This does not affected the parent
    /// process, but prevents the the child from changing outside
    /// of the assigned UID.
    ///
    /// If is set to *Existing*, the child is launched with the exact
    /// same operating set as the parent, persisting SetUID privilege.
    ///
    /// If mode is not set, it adopts whatever operating set the parent
    /// is in when spawn() is called.
    ///
    /// If the parent is not SetUID, this parameter is a no-op
    /// This function is not thread safe.
    ///
    /// If drop is set to true, the handle will switch to this
    /// mode when tearing down to avoid permission errors.
    /// Note that drop does not function in a multi-threaded
    /// environment, as multiple teardowns can change the mode
    /// between saving and restoring.
    #[cfg(feature = "user")]
    pub fn mode(mut self, mode: user::Mode) -> Self {
        self.mode_i(mode);
        self
    }

    /// Move a *SECCOMP* filter to the `Spawner`, loading in the child after forking.
    /// *SECCOMP* is the last operation applied. This has several consequences:
    ///
    /// 1.  The child will be running under the assigned operating set mode,
    ///     and said operating set must have permission to load the filter.
    ///  2.  If using Notify, the path to the monitor socket must
    ///      be accessible by the operating set mode.
    ///  3.  Your *SECCOMP* filter must permit `execve` to launch the application.
    ///      This does not have to be ALLOW. See the caveats to Notify if
    ///      you are using it.
    ///
    /// This function is thread safe.
    #[cfg(feature = "seccomp")]
    pub fn seccomp(self, seccomp: Filter) -> Self {
        self.seccomp_i(seccomp);
        self
    }

    pub fn cap(mut self, cap: Capability) -> Self {
        self.whitelist.insert(cap);
        self
    }

    pub fn caps(mut self, caps: impl IntoIterator<Item = Capability>) -> Self {
        caps.into_iter().for_each(|cap| {
            self.whitelist.insert(cap);
        });
        self
    }

    pub fn new_privileges(mut self, allow: bool) -> Self {
        self.no_new_privileges = !allow;
        self
    }

    /// Set the user mode without consuming the `Spawner`.
    /// This function is not thread safe.
    ///
    /// If drop is set to true, the handle will switch to this
    /// mode when tearing down to avoid permission errors.
    /// Note that drop does not function in a multi-threaded
    /// environment, as multiple teardowns can change the mode
    /// between saving and restoring.
    #[cfg(feature = "user")]
    pub fn mode_i(&mut self, mode: user::Mode) {
        self.mode = Some(mode);
    }

    /// Set a *SECCOMP* filter without consuming the `Spawner`.
    /// This function is thread safe.
    #[cfg(feature = "seccomp")]
    pub fn seccomp_i(&self, seccomp: Filter) {
        *self.seccomp.lock() = Some(seccomp)
    }

    pub fn cap_i(&mut self, cap: Capability) {
        self.whitelist.insert(cap);
    }

    pub fn caps_i(&mut self, caps: impl IntoIterator<Item = Capability>) {
        caps.into_iter().for_each(|cap| {
            self.whitelist.insert(cap);
        });
    }

    pub fn new_privileges_i(mut self, allow: bool) {
        self.no_new_privileges = !allow;
    }

    /// Run a closure within a fork.
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
    #[allow(dead_code)]
    pub unsafe fn fork<F, R>(self, op: F) -> Result<R, Error>
    where
        F: FnOnce() -> R + UnwindSafe,
        R: serde::Serialize + serde::de::DeserializeOwned,
    {
        let (read, write) = cond_pipe(&StreamMode::Pipe)?.unwrap();

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
                close(write).map_err(|e| SpawnError::Errno(Some(fork), "close write", e))?;
                let stream = Stream::new(read);
                let bytes = stream.read_bytes(None)?;
                Ok(postcard::from_bytes(&bytes)?)
            }

            ForkResult::Child => {
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

                clear_capabilities(self.whitelist);

                if self.no_new_privileges
                    && let Err(e) = prctl::set_no_new_privs()
                {
                    warn!("Could not set NO_NEW_PRIVS: {e}");
                }

                // Apply SECCOMP.
                // Because we can't just trust the application is able/willing to
                // apply a SECCOMP filter on it's own, we have to do it before the execve
                // call. That means the SECCOMP filter needs to either Allow, Log, Notify,
                // or some other mechanism to let the process to spawn.
                #[cfg(feature = "seccomp")]
                if let Some(filter) = filter {
                    filter.load();
                }

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

    /// Run a closure returning a File Descriptor within a fork.
    ///
    /// ### Safety
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
                // Because we can't just trust the application is able/willing to
                // apply a SECCOMP filter on it's own, we have to do it before the execve
                // call. That means the SECCOMP filter needs to either Allow, Log, Notify,
                // or some other mechanism to let the process to spawn.
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
    use std::io::Read;

    use super::*;
    use anyhow::Result;

    #[test]
    fn number() -> Result<()> {
        let result = unsafe { Fork::new().fork(|| 1) }?;
        assert!(result == 1);
        Ok(())
    }

    #[test]
    fn string() -> Result<()> {
        let str = "This is a test!".to_string();
        let result = unsafe { Fork::new().fork(|| str.clone()) }?;
        assert!(result == str);
        Ok(())
    }

    #[test]
    fn file() -> Result<()> {
        let path = "/tmp/test";
        let str = "Hello, world!";
        let mut file: std::fs::File = unsafe {
            Fork::new().fork_fd(|| {
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
