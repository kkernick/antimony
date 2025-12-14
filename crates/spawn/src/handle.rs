//!
//! The Spawn Handle is produced after consuming a Spawner via `spawn()`. It
//! mediates access to the child's input, output, error (As long as the
//! Spawner was configured to hook such descriptors), as well as mediating
//! signal handling and teardown.
//!
//!

use log::warn;
use nix::{
    errno::Errno,
    sys::{
        signal::{
            Signal::{self, SIGTERM},
            kill,
        },
        wait::{WaitPidFlag, WaitStatus, waitpid},
    },
    unistd::Pid,
};
use parking_lot::{Condvar, Mutex, MutexGuard};
use std::{
    collections::VecDeque,
    error, fmt,
    fs::File,
    io::{self, Read, Write},
    os::fd::OwnedFd,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle, sleep},
    time::{Duration, Instant},
};

/// Errors related to a ProcessHandle
#[derive(Debug)]
pub enum Error {
    /// Errors related to communicating with the process, such as
    /// when waiting, killing, or sending a signal fails.
    Comm(Errno),

    /// Errors when a Handle's descriptor functions are called, but
    /// the Spawner made no such descriptors.
    NoFile,

    /// Errors when no associate has the provided name.,
    NoAssociate(String),

    /// Errors when the Child fails; returned when the Handle's readers
    /// get strange output from the child.
    Child,

    /// Error when a Handle tries to write to a child standard input, but
    /// the child no longer exist.
    Input,

    /// Error trying to write to standard input.
    Io(io::Error),

    /// Timeout error
    Timeout,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Comm(e) => write!(
                f,
                "There was an error communicating to the child: {}",
                e.desc()
            ),
            Self::NoFile => write!(
                f,
                "The requested File Handle does not exist. Ensure --capture and --input are established during spawn()"
            ),
            Self::Child => write!(f, "The child process terminated prematurely"),
            Self::Input => write!(f, "Cannot read input, as child has already terminated!"),
            Self::Io(e) => write!(f, "IO Error: {e}"),
            Self::NoAssociate(name) => write!(f, "No such associate: {name}"),
            Self::Timeout => write!(f, "Timeout"),
        }
    }
}
impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Error::Comm(errno) => Some(errno),
            Error::Io(error) => Some(error),
            _ => None,
        }
    }
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
/// use os::fd::{OwnedFd, FromRawFd};
/// let mut handle = spawn::Stream::new(unsafe {OwnedFd::from_raw_fd(1)});
/// handle.read_all().unwrap();
/// ```
///
/// Asynchronous.
/// ```rust
/// use os::fd::{OwnedFd, FromRawFd};
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
    pub fn read_bytes(&self, bytes: usize) -> Result<Vec<u8>, Error> {
        let mut state = self.shared.state.lock();
        if state.finished {
            return Err(Error::NoFile);
        }
        let mut res = self.drain(&mut state, Some(bytes));
        while res.is_empty() {
            self.shared.condvar.wait(&mut state);
            res = self.drain(&mut state, Some(bytes));
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
/// and allow the caller to manage their lifetimes; this allows you to only manage
/// a single handle, with all its associates being cleanup when it does.
///
/// You should never construct a Handle yourself, it should always be returned through
/// a Spawner.
pub struct Handle {
    /// The name of the spawned binary.
    pub(super) name: String,

    /// The child PID. Once wait has been called, it is set to None
    child: Option<Pid>,
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
}
impl Handle {
    /// Construct a new `Handle` from a Child PID and pipes
    pub fn new(
        name: String,
        pid: Pid,

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
        }
    }

    /// Get the name of the handle.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn pid(&self) -> &Option<Pid> {
        &self.child
    }

    /// Wait for the child to terminate, then return the exit
    /// code.
    pub fn wait(&mut self, timeout: Option<Duration>) -> Result<i32, Error> {
        if let Some(pid) = self.child {
            let start = Instant::now();
            loop {
                match waitpid(
                    pid,
                    if timeout.is_some() {
                        Some(WaitPidFlag::WNOHANG)
                    } else {
                        None
                    },
                ) {
                    Ok(status) => {
                        self.child = None;
                        if let WaitStatus::Exited(_, code) = status {
                            self.exit = code;
                            break;
                        }
                    }
                    Err(e) => return Err(Error::Comm(e)),
                }

                if let Some(duration) = timeout {
                    let now = Instant::now().duration_since(start);
                    if now >= duration {
                        warn!("Aborting process early");
                        kill(pid, SIGTERM).map_err(Error::Comm)?;
                        return Err(Error::Timeout);
                    }
                }
            }
        }
        Ok(self.exit)
    }

    /// Wait for a child to terminate, but while ensuring a signal to the parent
    /// does not abruptly tear down the child.
    /// When SIGTERM or SIGINT is sent to the parent, it will send `sig` to the child,
    /// collect the exit code, and return gracefully.
    /// Because we are busy waiting, the loop waits 1 seconds between checking the state.
    pub fn wait_for_signal(&mut self, sig: Signal, timeout: Duration) -> Result<i32, Error> {
        if let Some(pid) = self.child {
            // Hook SIGTERM and SIGINT
            let term = Arc::new(AtomicBool::new(false));
            signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&term))
                .map_err(Error::Io)?;
            signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term))
                .map_err(Error::Io)?;

            // Wait until either we are hit with a signal, or the child exits.
            while !term.load(Ordering::Relaxed) {
                match waitpid(pid, Some(WaitPidFlag::WNOHANG)) {
                    Ok(status) => {
                        self.child = None;
                        if let WaitStatus::Exited(_, code) = status {
                            self.exit = code;
                            break;
                        }
                    }
                    Err(e) => return Err(Error::Comm(e)),
                }
                sleep(timeout);
            }

            // If the child is still alive, send it the signal
            if self.alive()? {
                self.signal(sig)?;
            }

            // Collect the error code and return
            self.wait(None)
        } else {
            Ok(self.exit)
        }
    }

    /// Check if the process is still alive, non-blocking.
    pub fn alive(&self) -> Result<bool, Error> {
        if let Some(pid) = self.child {
            match waitpid(pid, Some(WaitPidFlag::WNOHANG)) {
                Ok(WaitStatus::StillAlive) => Ok(true),
                Ok(WaitStatus::Exited(_, _)) | Ok(WaitStatus::Signaled(_, _, _)) => Ok(false),
                Ok(_) => Ok(true),
                Err(e) => Err(Error::Comm(e)),
            }
        } else {
            Ok(false)
        }
    }

    /// Send a signal to the child.
    /// If the child no longer exists, this returns an `Err`.
    pub fn signal(&mut self, sig: Signal) -> Result<(), Error> {
        if let Some(pid) = self.child
            && let Err(e) = kill(pid, sig)
        {
            return Err(Error::Comm(e));
        }
        Ok(())
    }

    /// Detach the thread from manual cleanup.
    /// This function does nothing more than move the Pid of the child out of the Handle.
    /// When the Handle falls out of scope, it will not have a Pid to terminate, so the
    /// child process will linger.
    ///
    /// `Spawner` sets Death Sig to **SIGTERM**, which means that when the parent dies,
    /// its children are sent **SIGTERM**, so as long as your child responsibly
    /// handles **SIGTERM**, it will not become an orphan. If you cannot guarantee
    /// this, hold the `Pid` and manage it directly.
    ///
    /// You therefore have three options on what to do with the return value of this
    /// function:
    ///
    ///  1.  If there was no child to detach, you'll get a None, and do nothing.
    ///  2.  If you want to manage the child yourself (Or associate it with another
    ///      Handle), capture the value.
    ///  3.  If you want to truly detach it, don't capture the return value.
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

    /// Return the Stream associate with the child's standard error, if it exists.
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
    pub fn error_all(&mut self) -> Result<String, Error> {
        if let Some(error) = &mut self.stderr {
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
    pub fn output_all(&mut self) -> Result<String, Error> {
        if let Some(output) = &mut self.stdout {
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
        if let Some(pid) = self.child {
            let _ = kill(pid, Signal::SIGTERM);
            let _ = waitpid(pid, None);
        }
        self.associated.iter_mut().for_each(|process| {
            let _ = process.signal(Signal::SIGTERM);
        });
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
