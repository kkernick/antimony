//! The Spawn Handle is produced after consuming a Spawner via `spawn()`. It
//! mediates access to the child's input, output, error (As long as the
//! Spawner was configured to hook such descriptors), as well as mediating
//! signal handling and teardown.

use log::warn;
use nix::{
    errno::Errno,
    sys::{
        signal::{Signal, kill, raise},
        wait::{WaitPidFlag, WaitStatus, waitpid},
    },
    unistd::Pid,
};
use parking_lot::{Condvar, Mutex, MutexGuard};
use signal_hook::{consts::signal, iterator::Signals};
use std::{
    collections::VecDeque,
    fs::File,
    io::{self, Read, Write},
    os::fd::OwnedFd,
    sync::Arc,
    thread::{self, JoinHandle, sleep},
    time::Duration,
};
use thiserror::Error;

/// Errors related to a ProcessHandle
#[derive(Debug, Error)]
pub enum Error {
    /// Errors related to communicating with the process, such as
    /// when waiting, killing, or sending a signal fails.
    #[error("Communication error: {0}")]
    Comm(Errno),

    /// Errors when a Handle's descriptor functions are called, but
    /// the Spawner made no such descriptors.
    #[error("No such file was created.")]
    NoFile,

    /// Errors when no associate has the provided name.
    #[error("No such associate found: {0}")]
    NoAssociate(String),

    /// Errors when the Child fails; returned when the Handle's readers
    /// get strange output from the child.
    #[error("Error in child process")]
    Child,

    /// Error when a Handle tries to write to a child standard input, but
    /// the child no longer exist.
    #[error("Failed to write to child")]
    Input,

    /// The parent received a termination signal
    #[error("Failed to communicate with child.")]
    Signal,

    /// Error trying to write to standard input.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// Timeout error
    #[error("Timeout")]
    Timeout,

    /// User switching errors.
    #[error("Failed to switch user: {0}")]
    User(Errno),
}

/// The shared state between StreamHandle and Worker Thread.
struct InnerBuffer {
    /// The current contents from the pipe
    buffer: VecDeque<u8>,

    /// Whether the Thread is still working.
    finished: bool,
}

/// The shared state between thread and handle.
struct SharedBuffer {
    state: Mutex<InnerBuffer>,
    condvar: Condvar,
}

/// A handle on a process' Output or Error streams.
/// The Handle can either be used asynchronously to read content as it is filled by the child,
/// or synchronously by calling `read_all`, which will wait until the child terminates, then
/// collect all output. For async, you can use `read_line`, or `read` for an exact byte count.
///
/// Content pulled with async functions are removed from the handle--it will not be present in `read_all`.
/// Therefore, you likely want to either use this handle in one of the two modes.
///
/// ## Examples
///
/// Synchronous.
/// ```rust
/// use std::os::fd::{OwnedFd, FromRawFd};
/// let mut handle = spawn::Stream::new(unsafe {OwnedFd::from_raw_fd(1)});
/// handle.read_all().unwrap();
/// ```
///
/// Asynchronous.
/// ```rust
/// use std::os::fd::{OwnedFd, FromRawFd};
/// let mut handle = spawn::Stream::new(unsafe {OwnedFd::from_raw_fd(1)});
/// while let Some(line) = handle.read_line() {
///     println!("{line}");
/// }
/// ```
pub struct Stream {
    /// The shared buffer.
    shared: Arc<SharedBuffer>,

    /// The worker.
    thread: Option<JoinHandle<()>>,
}

impl Stream {
    /// Construct a new StreamHandle from an OwnedFd connected to the child.
    pub fn new(owned_fd: OwnedFd) -> Self {
        let mut file = File::from(owned_fd);
        let shared = Arc::new(SharedBuffer {
            state: Mutex::new(InnerBuffer {
                buffer: VecDeque::new(),
                finished: false,
            }),
            condvar: Condvar::new(),
        });

        let thread_shared = Arc::clone(&shared);

        // Spawn the worker thread.
        let handle = thread::spawn(move || {
            let _ = (|| -> io::Result<()> {
                let mut buf = [0u8; 4096];
                loop {
                    let n = file.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    let mut state = thread_shared.state.lock();
                    state.buffer.extend(&buf[..n]);
                    thread_shared.condvar.notify_all();
                }
                Ok(())
            })();

            let mut state = thread_shared.state.lock();
            state.finished = true;
            thread_shared.condvar.notify_all();
        });

        Stream {
            shared,
            thread: Some(handle),
        }
    }

    /// Drain the current contents of the buffer.
    fn drain(&self, state: &mut MutexGuard<InnerBuffer>, upto: Option<usize>) -> Vec<u8> {
        match upto {
            Some(n) => {
                if n > state.buffer.len() {
                    state.buffer.drain(..).collect()
                } else {
                    state.buffer.drain(..=n).collect()
                }
            }
            None => state.buffer.drain(..).collect(),
        }
    }

    /// Read a line from the stream.
    /// This function is blocking, and will wait until a full line has been
    /// written to the stream. The line will then be removed from the Handle.
    pub fn read_line(&self) -> Option<String> {
        let mut state = self.shared.state.lock();
        loop {
            if let Some(pos) = state.buffer.iter().position(|&b| b == b'\n') {
                let line = String::from_utf8_lossy(&self.drain(&mut state, Some(pos))).into_owned();
                return Some(line);
            }

            if state.finished {
                if !state.buffer.is_empty() {
                    let rest = String::from_utf8_lossy(&self.drain(&mut state, None)).into_owned();
                    return Some(rest);
                } else {
                    return None;
                }
            }
            self.shared.condvar.wait(&mut state);
        }
    }

    /// Read the exact amount of bytes specified, or else throw an error.
    /// This function is blocking.
    pub fn read_bytes(&self, bytes: Option<usize>) -> Result<Vec<u8>, Error> {
        let mut state = self.shared.state.lock();
        let mut res = self.drain(&mut state, bytes);
        while res.is_empty() {
            self.shared.condvar.wait(&mut state);
            res = self.drain(&mut state, bytes);
        }
        Ok(res)
    }

    /// Wait for the thread to terminate (The subprocess closes their side of the pipe),
    /// then return the entire contents of the stream.
    ///
    /// This function is blocking.
    pub fn read_all(&mut self) -> Result<String, Error> {
        self.wait()?;
        let mut state = self.shared.state.lock();
        Ok(String::from_utf8_lossy(&self.drain(&mut state, None)).into_owned())
    }

    /// Join the worker thread, waiting until the subprocess closes their side of the pipe.
    pub fn wait(&mut self) -> Result<(), Error> {
        if let Some(handle) = self.thread.take() {
            match handle.join() {
                Ok(_) => Ok(()),
                Err(_) => Err(Error::Child),
            }
        } else {
            Ok(())
        }
    }
}
impl Drop for Stream {
    fn drop(&mut self) {
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

/// A handle to a child process created via `Spawner::spawn()`
/// If input/output/error redirection were setup in the Spawner,
/// you can use their related functions to access them.
///
/// Additionally, if there are other associated handles (Such as an auxiliary
/// task to the one launched by the handle), you can delegate them as associates
/// and allow the caller to manage their lifetimes. This allows you to only manage
/// a single handle, with all its associates being cleanup when it does.
///
/// You should never construct a Handle yourself, it should always be returned through
/// a Spawner.
pub struct Handle {
    /// The name of the spawned binary.
    name: String,

    /// The child PID. Once wait has been called, it is set to None
    child: Option<Pid>,

    /// The exit code, if the child has exited.
    exit: i32,

    /// A list of other Pids that the Handle should be responsible for,
    /// attached to the main child.
    associated: Vec<Handle>,

    /// The child's standard input.
    stdin: Option<File>,

    /// The child's standard output.
    stdout: Option<Stream>,

    /// The child's standard error.
    stderr: Option<Stream>,

    #[cfg(feature = "user")]
    mode: user::Mode,
}
impl Handle {
    /// Construct a new `Handle` from a Child PID and pipes
    pub fn new(
        name: String,
        pid: Pid,

        #[cfg(feature = "user")] mode: user::Mode,

        stdin: Option<OwnedFd>,
        stdout: Option<OwnedFd>,
        stderr: Option<OwnedFd>,
        associates: Vec<Handle>,
    ) -> Self {
        Self {
            name,
            child: Some(pid),
            exit: -1,
            stdin: stdin.map(File::from),
            stdout: stdout.map(Stream::new),
            stderr: stderr.map(Stream::new),
            associated: associates,

            #[cfg(feature = "user")]
            mode,
        }
    }

    /// Get the name of the handle.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the pid of the child.
    pub fn pid(&self) -> &Option<Pid> {
        &self.child
    }

    /// Wait for the child to exit, with a timeout in case of no activity.
    ///
    /// Note that this function uses a signal handler to ensure it does not
    /// hang the process, as well as efficiently wait the timeout. You cannot
    /// use this function in multi-threaded environments.
    pub fn wait_timeout(&mut self, timeout: Duration) -> Result<i32, Error> {
        if let Some(pid) = self.alive()? {
            let mut signals = Signals::new([
                signal::SIGTERM,
                signal::SIGINT,
                signal::SIGCHLD,
                signal::SIGALRM,
            ])?;

            let _ = thread::spawn(move || {
                sleep(timeout);
                let _ = raise(Signal::SIGALRM);
            });

            'outer: loop {
                for signal in signals.wait() {
                    match signal {
                        signal::SIGCHLD => match waitpid(pid, None) {
                            Ok(status) => {
                                self.child = None;
                                if let WaitStatus::Exited(_, code) = status {
                                    self.exit = code;
                                    break 'outer;
                                }
                            }
                            Err(Errno::ECHILD) => {
                                self.child = None;
                                self.exit = -1;
                                break 'outer;
                            }
                            Err(e) => return Err(Error::Comm(e)),
                        },
                        signal::SIGALRM => return Err(Error::Timeout),
                        _ => return Err(Error::Signal),
                    }
                }
            }

            // Collect the error code and return
            self.wait()
        } else {
            Ok(self.exit)
        }
    }

    /// Wait for the child to exit.
    ///
    /// Note that this function uses a signal handler to ensure it does not
    /// hang the process. You cannot use this function in multi-threaded environments.
    pub fn wait(&mut self) -> Result<i32, Error> {
        if let Some(pid) = self.alive()? {
            let mut signals = Signals::new([signal::SIGTERM, signal::SIGINT, signal::SIGCHLD])?;
            'outer: loop {
                for signal in signals.wait() {
                    match signal {
                        signal::SIGCHLD => match waitpid(pid, None) {
                            Ok(status) => {
                                self.child = None;
                                if let WaitStatus::Exited(_, code) = status {
                                    self.exit = code;
                                    break 'outer;
                                }
                            }
                            Err(Errno::ECHILD) => {
                                self.child = None;
                                self.exit = -1;
                                break 'outer;
                            }
                            Err(e) => return Err(Error::Comm(e)),
                        },
                        _ => return Err(Error::Signal),
                    }
                }
            }
        }
        Ok(self.exit)
    }

    /// Wait for the child without signal handlers.
    ///
    /// This function is a thread-safe version of wait, but
    /// means that signals will not be caught.
    pub fn wait_blocking(&mut self) -> Result<i32, Error> {
        if let Some(pid) = self.alive()? {
            'outer: loop {
                match waitpid(pid, None) {
                    Ok(status) => {
                        self.child = None;
                        if let WaitStatus::Exited(_, code) = status {
                            self.exit = code;
                            break 'outer;
                        }
                    }
                    Err(e) => return Err(Error::Comm(e)),
                }
            }
        }
        Ok(self.exit)
    }

    /// Check if the process is still alive, non-blocking.
    pub fn alive(&mut self) -> Result<Option<Pid>, Error> {
        if let Some(pid) = self.child {
            loop {
                match waitpid(pid, Some(WaitPidFlag::WNOHANG)) {
                    Ok(WaitStatus::StillAlive) => break Ok(Some(pid)),
                    Ok(WaitStatus::Exited(_, exit)) => {
                        self.child.take();
                        self.exit = exit;
                        break Ok(None);
                    }
                    Ok(WaitStatus::Signaled(_, _, _)) => {
                        self.child.take();
                        self.exit = -1;
                        break Ok(None);
                    }
                    Ok(_) => continue,
                    Err(Errno::ECHILD) => {
                        self.child = None;
                        self.exit = -1;
                        break Ok(None);
                    }
                    Err(e) => break Err(Error::Comm(e)),
                }
            }
        } else {
            Ok(None)
        }
    }

    /// Terminate the process with a SIGTERM request, but
    /// do not consume the Handle.
    pub fn terminate(&mut self) -> Result<(), Error> {
        if let Some(pid) = self.alive()? {
            match self.signal(Signal::SIGTERM) {
                Ok(_) => {
                    let _ = waitpid(pid, None);
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// Send a signal to the child.
    pub fn signal(&mut self, sig: Signal) -> Result<(), Error> {
        if let Some(pid) = self.alive()? {
            #[cfg(feature = "user")]
            let result = {
                let mode = self.mode;
                user::run_as!(mode, kill(pid, sig)).map_err(Error::User)?
            };

            #[cfg(not(feature = "user"))]
            let result = kill(pid, sig);

            match result {
                Ok(_) => Ok(()),
                Err(Errno::ESRCH) => {
                    self.child = None;
                    Ok(())
                }
                Err(e) => Err(Error::Comm(e)),
            }
        } else {
            Ok(())
        }
    }

    /// Detach the thread from manual cleanup.
    /// This function does nothing more than move the Pid of the child out of the Handle.
    /// When the Handle falls out of scope, it will not have a Pid to terminate, so the
    /// child process will linger.
    ///
    /// `Spawner` sets Death Sig to **SIGKILL**, which means that when the parent dies,
    /// its children are sent **SIGKILL**. This means a detached thread should not
    /// become a Zombie Process, even if the Pid is dropped on program exit.
    ///
    /// You therefore have three options on what to do with the return value of this
    /// function:
    ///
    ///  1. If there was no child to detach, you'll get a None, and do nothing.
    ///  2. If you want to manage the child yourself (Or associate it with another
    ///     Handle), capture the value.
    ///  3. If you want to truly detach it, don't capture the return value. It will
    ///     run in the background, and will be killed if its still running at
    ///     program exit.
    pub fn detach(mut self) -> Option<Pid> {
        self.child.take()
    }

    /// Returns a mutable reference to an associate within the Handle, if it exists.
    /// The associate is another Handle instance.
    pub fn get_associate(&mut self, name: &str) -> Option<&mut Handle> {
        self.associated
            .iter_mut()
            .find(|handle| handle.name == name)
    }

    /// Return the Stream associated with the child's standard error, if it exists.
    /// Note that pulling from the Stream consumes the contents--calling `error_all`
    /// will only return the contents from when you last pulled from the Stream.
    pub fn error(&mut self) -> Result<&mut Stream, Error> {
        if let Some(error) = &mut self.stderr {
            Ok(error)
        } else {
            Err(Error::NoFile)
        }
    }

    /// Waits for the child to terminate, then returns its entire standard error.
    pub fn error_all(mut self) -> Result<String, Error> {
        self.wait_blocking()?;
        if let Some(mut error) = self.stderr.take() {
            error.read_all()
        } else {
            Err(Error::NoFile)
        }
    }

    /// Return the Stream associate with the child's standard output, if it exists.
    /// Note that pulling from the Stream consumes the contents--calling `output_all`
    /// will only return the contents from when you last pulled from the Stream.
    pub fn output(&mut self) -> Result<&mut Stream, Error> {
        if let Some(output) = &mut self.stdout {
            Ok(output)
        } else {
            Err(Error::NoFile)
        }
    }

    /// Waits for the child to terminate, then returns its entire standard output.
    /// If you need the exit code, use wait() first.
    pub fn output_all(mut self) -> Result<String, Error> {
        self.wait_blocking()?;
        if let Some(mut output) = self.stdout.take() {
            output.read_all()
        } else {
            Err(Error::NoFile)
        }
    }

    /// Closes the Handle's side of the standard input pipe, if it exists.
    /// This sends an EOF to the child.
    pub fn close(&mut self) -> Result<(), Error> {
        if self.stdin.take().is_some() {
            Ok(())
        } else {
            Err(Error::NoFile)
        }
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        if let Ok(pid) = self.alive() {
            if let Some(pid) = pid {
                match self.signal(Signal::SIGKILL) {
                    Ok(_) => {
                        let _ = waitpid(pid, None);
                    }
                    Err(e) => warn!("Failed to terminate process {pid}: {e}"),
                }
            }
        } else {
            warn!("Could not communicate with child!")
        }
    }
}
impl Write for Handle {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.stdin.as_mut() {
            Some(stdin) => stdin.write(buf),
            None => Err(io::Error::new(io::ErrorKind::BrokenPipe, "stdin is closed")),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.stdin.as_mut() {
            Some(stdin) => stdin.flush(),
            None => Ok(()),
        }
    }
}
