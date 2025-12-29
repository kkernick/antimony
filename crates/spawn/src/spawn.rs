//! Spawn subprocesses with more fine-grained control over File Descriptors,
//! UID/GID, and File Stream handling.
#![allow(dead_code)]

use crate::{Stream, handle::Handle};
use log::trace;
use nix::{
    sys::{prctl, signal::Signal::SIGTERM},
    unistd::{ForkResult, close, dup2_stderr, dup2_stdin, dup2_stdout, execv, execve, fork, pipe},
};
use parking_lot::Mutex;
use std::{
    borrow::Cow,
    collections::HashMap,
    convert::Infallible,
    env, error,
    ffi::{CString, NulError},
    fmt,
    os::fd::OwnedFd,
    process::exit,
};
use which::which;

#[cfg(feature = "seccomp")]
use seccomp::filter::Filter;

#[cfg(feature = "fd")]
use {
    nix::fcntl::{FcntlArg, FdFlag, fcntl},
    std::os::fd::AsRawFd,
};

#[cfg(feature = "cache")]
use std::{fs, path::Path};

/// Errors related to the Spawner.
#[derive(Debug)]
pub enum Error {
    /// Errors when passed arguments contain Null values.
    Null(NulError),

    /// Errors when the cache is improperly accessed.
    #[cfg(feature = "cache")]
    Cache(&'static str),

    /// Errors reading/writing to the cache.
    Io(std::io::Error),

    /// Errors to various functions that return `Errno`.
    Errno(Option<ForkResult>, &'static str, nix::errno::Errno),

    /// Errors resolving binary paths.
    Path(String),

    /// An error when trying to fork.
    Fork(nix::errno::Errno),

    /// An error when the spawner fails to parse the environment.
    Environment,

    /// An error trying to apply the *SECCOMP* Filter.
    #[cfg(feature = "seccomp")]
    Seccomp(seccomp::filter::Error),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Null(error) => write!(f, "Provided string contains null values: {error}"),

            #[cfg(feature = "cache")]
            Self::Cache(error) => write!(f, "Cache error: {error}"),

            Self::Io(error) => write!(f, "Io error: {error}"),

            Self::Errno(fork, context, errno) => {
                let source = match fork {
                    Some(ForkResult::Child) => "child",
                    Some(ForkResult::Parent { child: _ }) | None => "parent",
                };

                write!(f, "{source} failed to {context}: {errno}",)
            }
            Self::Path(path) => write!(f, "Could not resolve binary: {path}"),
            Self::Fork(errno) => write!(f, "Failed to fork: {errno}"),
            Self::Environment => write!(f, "Failed to parse environment"),

            #[cfg(feature = "seccomp")]
            Self::Seccomp(error) => write!(f, "Failed to load SECCOMP filter: {error}"),
        }
    }
}
impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Null(error) => Some(error),

            #[cfg(feature = "cache")]
            Self::Io(error) => Some(error),

            Self::Errno(_, _, errno) => Some(errno),
            Self::Fork(errno) => Some(errno),

            #[cfg(feature = "seccomp")]
            Self::Seccomp(error) => Some(error),
            _ => None,
        }
    }
}

/// How to handle the standard input/out/error streams
#[derive(Default)]
pub enum StreamMode {
    /// Collect the stream contents in a Stream object via a
    /// pipe that can be retrieved in the `spawn::Handle`
    Pipe,

    /// Share STDIN/STDOUT/STDERR with the process, such that it can write
    /// to the parent. This is the default.
    #[default]
    Share,

    /// Send the output to the system logger at the provided level.
    Log(log::Level),
}

/// Spawn a child.
/// ## Thread Safety
/// Calls to the Spawner's arguments and file descriptors are
/// thread safe, and their order is guaranteed. All other functions are not
/// thread safe.
///
/// ## Examples
/// Launch bash in a child, inheriting the parent's input/output/error:
/// ```rust
/// spawn::Spawner::new("bash").unwrap().spawn().unwrap();
/// ```
///
/// Launch cat, feeding it input from the parent:
/// ```rust
/// use std::io::Write;
/// let mut handle = spawn::Spawner::new("cat").unwrap()
///     .input(spawn::StreamMode::Pipe)
///     .output(spawn::StreamMode::Pipe)
///     .spawn()
///     .unwrap();
/// let string = "Hello, World!";
/// write!(handle, "{}", &string);
/// handle.close();
/// let output = handle.output().unwrap().read_all().unwrap();
/// assert!(output == string);
/// ```
pub struct Spawner {
    /// The binary to run
    cmd: String,

    /// A unique name for the process, to be used to reference it by the Handle.
    unique_name: Option<String>,

    /// Arguments
    args: Mutex<Vec<CString>>,

    /// Whether to pipe **STDIN**. This lets you call `Handle::write()` to
    /// the process handle to send any Display value to the child.
    input: StreamMode,

    /// Capture the child's **STDOUT**.
    output: StreamMode,

    /// Capture the child's **STDERR**.
    error: StreamMode,

    /// Clear the environment before spawning the child.
    preserve_env: bool,

    /// Environment variables
    env: Vec<CString>,

    /// A list of other Pids that the eventual Handle should be responsible for,
    /// attached to the main child.
    associated: Vec<Handle>,

    /// An index to cache parts of the command line
    #[cfg(feature = "cache")]
    cache_index: Mutex<Option<usize>>,

    /// FD's to pass to the program. These do not include 0,1,2 who's
    /// logic is controlled via input/capture respectively.
    #[cfg(feature = "fd")]
    fds: Mutex<Vec<OwnedFd>>,

    /// The User to run the program under.
    #[cfg(feature = "user")]
    mode: Option<user::Mode>,

    /// Use `pkexec` to elevate via *Polkit*.
    #[cfg(feature = "elevate")]
    elevate: bool,

    /// An optional *SECCOMP* policy to load on the child.
    #[cfg(feature = "seccomp")]
    seccomp: Mutex<Option<Filter>>,
}
impl<'a> Spawner {
    /// Construct a `Spawner` to spawn *cmd*.
    /// *cmd* will be resolved from **PATH**.
    pub fn new(cmd: impl Into<String>) -> Result<Self, Error> {
        let cmd = cmd.into();
        let path = which::which(&cmd).map_err(|_| Error::Path(cmd))?;
        Ok(Self::abs(path))
    }

    pub fn abs(cmd: impl Into<String>) -> Self {
        Self {
            cmd: cmd.into(),
            unique_name: None,
            args: Mutex::new(vec![]),

            input: StreamMode::Share,
            output: StreamMode::Share,
            error: StreamMode::Share,

            preserve_env: false,
            env: Vec::new(),

            associated: Vec::new(),

            #[cfg(feature = "cache")]
            cache_index: Mutex::new(None),

            #[cfg(feature = "fd")]
            fds: Mutex::new(vec![]),

            #[cfg(feature = "user")]
            mode: None,

            #[cfg(feature = "elevate")]
            elevate: false,

            #[cfg(feature = "seccomp")]
            seccomp: Mutex::new(None),
        }
    }

    /// Resolve an environment variable.
    /// Fails if the value contains a NULL byte, or the key could not
    /// be resolved.
    /// This function is not thread safe.
    fn resolve_env(var: String) -> Result<CString, Error> {
        if var.contains('=') {
            CString::new(var).map_err(Error::Null)
        } else {
            let val = env::var(&var).map_err(|_| Error::Path(var.clone()))?;
            CString::new(format!("{var}={val}")).map_err(Error::Null)
        }
    }

    /// Control whether to hook the child's standard input.
    /// This function is not thread safe.
    pub fn input(mut self, input: StreamMode) -> Self {
        self.input_i(input);
        self
    }

    /// Control whether to hook the child's standard output.
    /// This function is not thread safe.
    pub fn output(mut self, output: StreamMode) -> Self {
        self.output_i(output);
        self
    }

    /// Control whether to hook the child's standard error.
    /// This function is not thread safe.
    pub fn error(mut self, error: StreamMode) -> Self {
        self.error_i(error);
        self
    }

    /// Give a unique name to the process, so you can refer to the Handle.
    /// If no name is set, the string passed to Spawn::new() will be used
    pub fn name(mut self, name: &str) -> Self {
        self.unique_name = Some(name.to_string());
        self
    }

    /// Attach another process that is attached to the main child, and should be killed
    /// when the eventual Handle goes out of scope.
    pub fn associate(&mut self, process: Handle) {
        self.associated.push(process);
    }

    /// Returns a mutable reference to an associate within the Handle, if it exists.
    /// The associate is another Handle instance.
    pub fn get_associate(&mut self, name: &str) -> Option<&mut Handle> {
        self.associated
            .iter_mut()
            .find(|handle| handle.name == name)
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

    /// Elevate the child to root privilege by using *PolKit* for authentication.
    /// `pkexec` must exist, and must be in path.
    /// The operating set of the child must ensure the real user can
    /// authorize via *PolKit*.
    /// This function is not thread safe.
    #[cfg(feature = "elevate")]
    pub fn elevate(mut self, elevate: bool) -> Self {
        self.elevate_i(elevate);
        self
    }

    /// Preserve the environment of the parent when launching the child.
    /// `Spawner` defaults to clearing the environment.
    /// This function is not thread safe.
    pub fn preserve_env(mut self, preserve: bool) -> Self {
        self.preserve_env_i(preserve);
        self
    }

    /// Sets an environment variable to pass to the process. If the string contains
    /// a keypair (USER=user), the provided value will be passed, if only a key is
    /// passed (USER) it will be looked up from the caller's environment.
    ///
    /// Returns an error if the variable contains NULL, or the key doesn't exist
    /// in the parent environment
    ///
    /// This function is not thread safe.
    pub fn env(mut self, var: impl Into<Cow<'a, str>>) -> Result<Self, Error> {
        self.env_i(var)?;
        Ok(self)
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

    /// Move a new argument to the argument vector.
    /// This function is guaranteed to append to the end of the current argument
    /// vector.
    /// This function is thread safe.
    /// This function will fail if the argument contains a NULL byte.
    pub fn arg(self, arg: impl Into<Cow<'a, str>>) -> Result<Self, Error> {
        self.arg_i(arg)?;
        Ok(self)
    }

    /// Move a new FD to the `Spawner`.
    /// FD's will be shared to the child under the same value.
    /// Any FD's in the parent not explicitly passed, which includes
    /// this function, and the input/output/error functions, will be dropped.
    /// This function is thread safe.
    #[cfg(feature = "fd")]
    pub fn fd(self, fd: impl Into<OwnedFd>) -> Self {
        self.fd_i(fd);
        self
    }

    /// Move a FD to the `Spawner`, and attach it to an argument to ensure the
    /// value is identical.
    /// This function is thread safe.
    /// This function will fail if the argument contains a NULL byte.
    ///
    /// ## Example
    /// Bubblewrap supports the --file flag, which accepts a FD and destination.
    /// If you want to ensure you don't accidentally mismatch FDs, you can
    /// commit both the FD and argument in the same transaction:
    /// ```rust
    /// let file = std::fs::File::create("file.txt").unwrap();
    /// spawn::Spawner::new("bwrap").unwrap()
    ///     .fd_arg("--file", file).unwrap()
    ///     .arg("/file.txt").unwrap()
    ///     .spawn().unwrap();
    /// std::fs::remove_file("file.txt").unwrap();
    /// ```
    #[cfg(feature = "fd")]
    pub fn fd_arg(
        self,
        arg: impl Into<Cow<'a, str>>,
        fd: impl Into<OwnedFd>,
    ) -> Result<Self, Error> {
        self.fd_arg_i(arg, fd)?;
        Ok(self)
    }

    /// Move an iterator of arguments into the `Spawner`.
    /// This function is thread safe, and guarantees that the arguments
    /// in the iterator will appear sequentially, and in the same order.
    /// This function will fail if an argument contains a NULL byte.
    pub fn args<I, S>(self, args: I) -> Result<Self, Error>
    where
        I: IntoIterator<Item = S>,
        S: Into<Cow<'a, str>>,
    {
        self.args_i(args)?;
        Ok(self)
    }

    /// Move an iterator of FD's to the `Spawner`.
    /// This function is thread safe.
    #[cfg(feature = "fd")]
    pub fn fds<I, S>(self, fds: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OwnedFd>,
    {
        self.fds_i(fds);
        self
    }

    /// Set the input flag without consuming the `Spawner`.
    /// This function is not thread safe.
    pub fn input_i(&mut self, input: StreamMode) {
        self.input = input;
    }

    /// Set the output flag without consuming the `Spawner`.
    /// This function is not thread safe.
    pub fn output_i(&mut self, output: StreamMode) {
        self.output = output;
    }

    /// Set the error flag without consuming the `Spawner`.
    /// This function is not thread safe.
    pub fn error_i(&mut self, error: StreamMode) {
        self.error = error
    }

    /// Set the elevate flag without consuming the `Spawner`.
    /// This function is not thread safe.
    #[cfg(feature = "elevate")]
    pub fn elevate_i(&mut self, elevate: bool) {
        self.elevate = elevate
    }

    /// Set the preserve environment flag without consuming the `Spawner`.
    /// This function is not thread safe.
    pub fn preserve_env_i(&mut self, preserve: bool) {
        self.preserve_env = preserve;
    }

    /// Sets an environment variable to the child process.
    /// Fails if the key doesn't exist, or the var contains a NULL byte.
    pub fn env_i(&mut self, var: impl Into<Cow<'a, str>>) -> Result<(), Error> {
        self.env.push(Self::resolve_env(var.into().into_owned())?);
        Ok(())
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

    /// Move an argument to the `Spawner` in-place.
    /// This function is thread safe.
    /// This argument will fail if the argument contains a NULL byte.
    pub fn arg_i(&self, arg: impl Into<Cow<'a, str>>) -> Result<(), Error> {
        self.args
            .lock()
            .push(CString::new(arg.into().as_ref()).map_err(Error::Null)?);
        Ok(())
    }

    /// Move a FD to the `Spawner` in-place.
    /// This function is thread safe.
    #[cfg(feature = "fd")]
    pub fn fd_i(&self, fd: impl Into<OwnedFd>) {
        self.fds.lock().push(fd.into());
    }

    /// Move FDs to the `Spawner` in-place, passing it as an argument.
    /// This function is thread safe.
    #[cfg(feature = "fd")]
    pub fn fd_arg_i(
        &self,
        arg: impl Into<Cow<'a, str>>,
        fd: impl Into<OwnedFd>,
    ) -> Result<(), Error> {
        let fd = fd.into();
        self.args_i([arg.into(), Cow::Owned(format!("{}", fd.as_raw_fd()))])?;
        self.fd_i(fd);
        Ok(())
    }

    /// Move an iterator of FDs to the `Spawner` in-place.
    /// This function is thread safe.
    #[cfg(feature = "fd")]
    pub fn fds_i<I, S>(&self, fds: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<OwnedFd>,
    {
        self.fds.lock().extend(fds.into_iter().map(Into::into));
    }

    /// Move an iterator of arguments to the `Spawner` in-place.
    /// This function is thread safe, and both sequence and order
    /// are guaranteed.
    /// This function will fail if any argument contains a NULL byte.
    pub fn args_i<I, S>(&self, args: I) -> Result<(), Error>
    where
        I: IntoIterator<Item = S>,
        S: Into<Cow<'a, str>>,
    {
        let mut a = self.args.lock();

        for s in args {
            let cow: Cow<'a, str> = s.into();
            a.push(CString::new(cow.as_ref()).map_err(Error::Null)?);
        }
        Ok(())
    }

    /// Set the cache index.
    /// Once the cache flag has been set, all subsequent arguments will
    /// be cached to the file provided to cache_write.
    /// On future runs, `cache_read` can be used to append those cached
    /// contents to the `Spawner`'s arguments.
    /// This function is thread safe.
    /// This function fails if cache_start is called twice without having
    /// first called cache_write.
    ///
    /// ## Examples
    ///
    /// ```rust
    /// let cache = std::path::PathBuf::from("cmd.cache");
    /// let mut handle = spawn::Spawner::new("bash").unwrap();
    /// if cache.exists() {
    ///     handle.cache_read(&cache).unwrap();
    /// } else {
    ///     handle.cache_start().unwrap();
    ///     handle.arg_i("arg").unwrap();
    ///     handle.cache_write(&cache).unwrap();
    /// }
    /// std::fs::remove_file(cache);
    /// ```
    ///
    /// ## Caveat
    ///
    /// Because the cache is written to disk, ephemeral values, such
    /// as FD values, temporary files, etc, must not be passed to the
    /// Spawner, otherwise those values would be cached, and likely
    /// be invalid when trying to use the cached results.
    #[cfg(feature = "cache")]
    pub fn cache_start(&self) -> Result<(), Error> {
        let mut index = self.cache_index.lock();
        if index.is_some() {
            Err(Error::Cache("Caching already started!"))
        } else {
            *index = Some(self.args.lock().len());
            Ok(())
        }
    }

    /// Write all arguments added to the `Spawner` since `cache_start`
    /// was called to the file provided.
    /// This function is thread safe.
    /// This function will fail if `cache_start` was not called,
    /// or if there are errors writing to the provided path.
    #[cfg(feature = "cache")]
    pub fn cache_write(&self, path: &Path) -> Result<(), Error> {
        use std::io::Write;
        let mut index = self.cache_index.lock();
        if let Some(i) = *index {
            let args = self.args.lock();

            if let Some(parent) = path.parent()
                && !parent.exists()
            {
                fs::create_dir(parent).map_err(Error::Io)?;
            }

            let mut file = fs::File::create(path).map_err(Error::Io)?;
            for arg in &args[i..] {
                writeln!(file, "{}", arg.to_string_lossy()).map_err(Error::Io)?;
            }
            *index = None;
            Ok(())
        } else {
            Err(Error::Cache("Cache not started!"))
        }
    }

    /// Read from the cache file, adding its contents to the `Spawner`'s
    /// arguments.
    /// This function is thread safe.
    /// This function will fail if there is an error reading the file,
    /// or if the contents contain strings will NULL bytes.
    #[cfg(feature = "cache")]
    pub fn cache_read(&self, path: &Path) -> Result<(), Error> {
        let mut args = self.args.lock();

        for arg in fs::read_to_string(path).map_err(Error::Io)?.lines() {
            args.push(CString::new(arg).map_err(Error::Null)?);
        }
        Ok(())
    }

    /// Spawn the child process.
    /// This consumes the structure, returning a `spawn::Handle`.
    ///
    /// ## Errors
    /// This function can fail for many reasons:
    ///
    /// ### Parent Errors (Which will return Err)
    /// * The `fork` fails.
    /// * The Parent fails to setup/close/duplicate input/output/error pipes.
    ///
    /// ### Child Errors (Which will cause errors when using the `Handle`)
    /// * The child fails to close/duplicate input/output/error pipes.
    /// * The application to run cannot be resolved in **PATH**.
    /// * Elevate is enabled, but `pkexec` cannot be found in **PATH**.
    /// * The resolved application (Or `pkexec` if *elevate*) has a path containing a NULL byte.
    /// * `F_SETFD` cannot be cleared on owned FDs.
    /// * **SIGTERM** cannot be set as the Child's Death Sig.
    /// * A user mode has been set, but dropping to it fails.
    /// * A *SECCOMP* filter is set, but it fails to set.
    /// * `execve` Fails.
    #[allow(unused_mut)]
    pub fn spawn(mut self) -> Result<Handle, Error> {
        // Create our pipes based on whether we need t
        // hem.
        // Because we use these conditionals later on when using them,
        // we can unwrap() with impunity.
        let stdout = cond_pipe(&self.output)?;
        let stderr = cond_pipe(&self.error)?;
        let stdin = cond_pipe(&self.input)?;

        #[cfg(feature = "fd")]
        let fds = self.fds.into_inner();

        let mut cmd_c: Option<CString> = None;
        let mut args_c = Vec::<CString>::new();

        // Launch with pkexec if we're elevated.
        #[cfg(feature = "elevate")]
        if self.elevate {
            let polkit = CString::new(
                which("pkexec")
                    .map_err(|e| Error::Path(e.to_string()))?
                    .as_bytes(),
            )
            .map_err(Error::Null)?;

            if cmd_c.is_none() {
                cmd_c = Some(polkit.clone());
            }
            args_c.push(polkit);
        }

        let resolved = CString::new(self.cmd.clone()).map_err(Error::Null)?;
        let cmd_c = if let Some(cmd) = cmd_c {
            cmd
        } else {
            resolved.clone()
        };

        args_c.push(resolved);
        args_c.append(&mut self.args.into_inner());

        // Log if desired.
        if log::log_enabled!(log::Level::Trace) {
            let formatted = args_c
                .iter()
                .filter_map(|s| s.to_str().ok())
                .collect::<Vec<&str>>()
                .join(" ");
            trace!("{formatted:?}");
        }

        // Clear F_SETFD to allow passed FD's to persist after execve
        #[cfg(feature = "fd")]
        for fd in &fds {
            fcntl(fd, FcntlArg::F_SETFD(FdFlag::empty()))
                .map_err(|e| Error::Errno(None, "fnctl fd", e))?;
        }

        let envs: HashMap<String, String> = self
            .env
            .iter()
            .filter_map(|env| {
                if let Ok(estr) = env.clone().into_string() {
                    let mut split = estr.split('=');
                    if let Some(key) = split.next()
                        && let Some(value) = split.next()
                    {
                        return Some((key.to_string(), value.to_string()));
                    }
                }
                None
            })
            .collect();

        let fork = unsafe { fork() }.map_err(Error::Fork)?;
        match fork {
            ForkResult::Parent { child } => {
                let name = if let Some(name) = self.unique_name {
                    name
                } else {
                    self.cmd
                };

                // Set the relevant pipes.
                let stdin = if let Some((read, write)) = stdin {
                    close(read).map_err(|e| Error::Errno(Some(fork), "close input", e))?;
                    Some(write)
                } else {
                    None
                };

                let stdout = if let Some((read, write)) = stdout {
                    close(write).map_err(|e| Error::Errno(Some(fork), "close error", e))?;

                    if let StreamMode::Log(log) = self.output {
                        let name = name.clone();
                        std::thread::spawn(move || logger(log, read, name));
                        None
                    } else {
                        Some(read)
                    }
                } else {
                    None
                };

                let stderr = if let Some((read, write)) = stderr {
                    close(write).map_err(|e| Error::Errno(Some(fork), "close output", e))?;

                    if let StreamMode::Log(log) = self.error {
                        let name = name.clone();
                        std::thread::spawn(move || logger(log, read, name));
                        None
                    } else {
                        Some(read)
                    }
                } else {
                    None
                };

                let mode = self.mode.unwrap_or(
                    user::current().map_err(|e| Error::Errno(Some(fork), "getresuid", e))?,
                );

                // Return.
                let handle = Handle::new(
                    name,
                    child,
                    #[cfg(feature = "user")]
                    mode,
                    stdin,
                    stdout,
                    stderr,
                    self.associated,
                );
                Ok(handle)
            }

            ForkResult::Child => {
                let result = || -> Result<Infallible, Error> {
                    // Setup the pipes.
                    if let Some((read, write)) = stdout {
                        close(read).map_err(|e| Error::Errno(Some(fork), "close output", e))?;
                        dup2_stdout(write)
                            .map_err(|e| Error::Errno(Some(fork), "dup output", e))?;
                    }
                    if let Some((read, write)) = stderr {
                        close(read).map_err(|e| Error::Errno(Some(fork), "close error", e))?;
                        dup2_stderr(write).map_err(|e| Error::Errno(Some(fork), "dup error", e))?;
                    }
                    if let Some((read, write)) = stdin {
                        close(write).map_err(|e| Error::Errno(Some(fork), "close input", e))?;
                        dup2_stdin(read).map_err(|e| Error::Errno(Some(fork), "dup input", e))?;
                    }

                    // Ensure that the child dies when the parent does.
                    prctl::set_pdeathsig(SIGTERM)
                        .map_err(|e| Error::Errno(Some(fork), "set death signal", e))?;

                    // Drop modes
                    #[cfg(feature = "user")]
                    if let Some(mode) = self.mode {
                        user::drop(mode)
                            .map_err(|e| Error::Errno(Some(fork), "drop user privilege", e))?;
                    }

                    // Apply SECCOMP.
                    // Because we can't just trust the application is able/willing to
                    // apply a SECCOMP filter on it's own, we have to do it before the execve
                    // call. That means the SECCOMP filter needs to either Allow, Log, Notify,
                    // or some other mechanism to let the process to spawn.
                    #[cfg(feature = "seccomp")]
                    if let Some(filter) = self.seccomp.into_inner() {
                        filter.load().map_err(Error::Seccomp)?;
                    }

                    for (key, value) in envs {
                        unsafe { env::set_var(key, value) };
                    }

                    // Execve
                    if self.preserve_env {
                        execv(&cmd_c, &args_c)
                    } else {
                        execve(&cmd_c, &args_c, &self.env)
                    }
                    .map_err(|errno| Error::Errno(Some(fork), "exec", errno))
                }();

                let e = result.unwrap_err();
                log::error!("Failed to spawn child: {e}");
                exit(-1);
            }
        }
    }
}

/// Conditionally create a pipe.
/// Returns either a set of `None`, or the result of `pipe()`
fn cond_pipe(cond: &StreamMode) -> Result<Option<(OwnedFd, OwnedFd)>, Error> {
    match cond {
        StreamMode::Pipe => match pipe() {
            Ok((r, w)) => Ok(Some((r, w))),
            Err(e) => Err(Error::Errno(None, "pipe", e)),
        },
        StreamMode::Log(e) => {
            if log::log_enabled!(*e) {
                match pipe() {
                    Ok((r, w)) => Ok(Some((r, w))),
                    Err(e) => Err(Error::Errno(None, "pipe", e)),
                }
            } else {
                Ok(None)
            }
        }
        StreamMode::Share => Ok(None),
    }
}

/// Log all activity from the child at the desired level.
pub fn logger(level: log::Level, fd: OwnedFd, name: String) {
    let stream = Stream::new(fd);
    while let Some(line) = stream.read_line() {
        log::log!(level, "{name}: {line}");
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use std::io::Write;

    use super::*;

    #[test]
    fn bash() -> Result<()> {
        let string = "Hello, World!";
        let mut handle = Spawner::new("bash")?
            .args(["-c", &format!("echo '{string}'")])?
            .output(StreamMode::Pipe)
            .error(StreamMode::Pipe)
            .spawn()?;

        let output = handle.output()?.read_all()?;
        assert!(output.trim() == string);
        Ok(())
    }

    #[test]
    fn cat() -> Result<()> {
        let mut handle = Spawner::new("cat")?
            .input(StreamMode::Pipe)
            .output(StreamMode::Pipe)
            .spawn()?;

        let string = "Hello, World!";
        write!(handle, "{string}")?;
        handle.close()?;

        let output = handle.output()?.read_all()?;
        assert!(output.trim() == string);
        Ok(())
    }

    #[test]
    fn read() -> Result<()> {
        let string = "Hello!";
        let mut handle = Spawner::new("echo")?
            .arg(string)?
            .output(StreamMode::Pipe)
            .spawn()?;

        let bytes = handle.output()?.read_bytes(string.len())?;
        let output = String::from_utf8_lossy(&bytes);
        assert!(output.trim() == string);
        Ok(())
    }

    #[test]
    fn clear_env() -> Result<()> {
        let mut handle = Spawner::new("bash")?
            .args(["-c", "echo $USER"])?
            .output(StreamMode::Pipe)
            .error(StreamMode::Pipe)
            .spawn()?;

        let output = handle.output()?.read_all()?;
        assert!(output.trim().is_empty());
        Ok(())
    }

    #[test]
    fn preserve_env() -> Result<()> {
        let user = "Test";
        let mut handle = Spawner::new("bash")?
            .args(["-c", "echo $USER"])?
            .env(format!("USER={user}"))?
            .output(StreamMode::Pipe)
            .error(StreamMode::Pipe)
            .spawn()?;

        let output = handle.output()?.read_all()?;
        assert!(output.trim() == user);
        Ok(())
    }
}
