//! Spawn subprocesses with more fine-grained control over File Descriptors,
//! UID/GID, and File Stream handling.

use crate::{clear_capabilities, cond_pipe, dup_null, format_iter, handle::Handle, logger};
use caps::{Capability, CapsHashSet};
use dashmap::{DashMap, DashSet, mapref::one::RefMut};
use log::{trace, warn};
use nix::{
    sys::{prctl, signal::Signal::SIGTERM},
    unistd::{ForkResult, close, dup2_stderr, dup2_stdin, dup2_stdout, execve, fork},
};
use parking_lot::Mutex;
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    error,
    ffi::{CString, NulError, OsString},
    fmt,
    os::fd::OwnedFd,
    process::exit,
    str::FromStr,
    sync::atomic::{AtomicBool, Ordering},
};

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
    Path(which::Error),

    /// An error when trying to fork.
    Fork(nix::errno::Errno),

    /// An error when the spawner fails to parse the environment.
    Environment,

    #[cfg(feature = "seccomp")]
    /// An error trying to apply the *SECCOMP* Filter.
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

    /// Send the output to the system logger at the provided level. If the log
    /// level is below this, output is discarded.
    Log(log::Level),

    /// Send output to /dev/null.
    Discard,

    #[cfg(feature = "fd")]
    /// Send the output to the provided File Descriptor.
    Fd(OwnedFd),
}

/// Spawn a child.
///
/// ## Thread Safety
///
/// This entire object is safe to pass and construct across multiple threads.
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
    unique_name: Mutex<Option<String>>,

    /// Arguments
    args: Mutex<Vec<CString>>,

    /// Whether to pipe **STDIN**. This lets you call `Handle::write()` to
    /// the process handle to send any Display value to the child.
    input: Mutex<StreamMode>,

    /// Capture the child's **STDOUT**.
    output: Mutex<StreamMode>,

    /// Capture the child's **STDERR**.
    error: Mutex<StreamMode>,

    /// Clear the environment before spawning the child.
    preserve_env: AtomicBool,

    /// Don't clear privileges.
    no_new_privileges: AtomicBool,

    /// Whitelisted capabilities.
    whitelist: DashSet<Capability>,

    /// Environment variables
    env: DashMap<CString, CString>,

    /// A list of other Pids that the eventual Handle should be responsible for,
    /// attached to the main child.
    associated: DashMap<String, Handle>,

    /// An index to cache parts of the command line
    #[cfg(feature = "cache")]
    cache_index: Mutex<Option<usize>>,

    /// FD's to pass to the program. These do not include 0,1,2 who's
    /// logic is controlled via input/capture respectively.
    #[cfg(feature = "fd")]
    fds: Mutex<Vec<OwnedFd>>,

    /// The User to run the program under.
    #[cfg(feature = "user")]
    mode: Mutex<Option<user::Mode>>,

    /// Use `pkexec` to elevate via *Polkit*.
    #[cfg(feature = "elevate")]
    elevate: AtomicBool,

    /// An optional *SECCOMP* policy to load on the child.
    #[cfg(feature = "seccomp")]
    seccomp: Mutex<Option<Filter>>,
}
impl<'a> Spawner {
    /// Construct a `Spawner` to spawn *cmd*.
    /// *cmd* will be resolved from **PATH**.
    pub fn new(cmd: impl Into<String>) -> Result<Self, Error> {
        let cmd = cmd.into();
        let path = which::which(&cmd).map_err(Error::Path)?;
        Ok(Self::abs(path))
    }

    /// Construct a `Spanwner` to spawn *cmd*.
    /// This function treats *cmd* as an absolute
    /// path. No resolution is performed.
    pub fn abs(cmd: impl Into<String>) -> Self {
        Self {
            cmd: cmd.into(),
            unique_name: Mutex::new(None),
            args: Mutex::default(),

            input: Mutex::new(StreamMode::Share),
            output: Mutex::new(StreamMode::Share),
            error: Mutex::new(StreamMode::Share),

            preserve_env: AtomicBool::new(false),
            no_new_privileges: AtomicBool::new(true),
            whitelist: DashSet::new(),
            env: DashMap::new(),

            associated: DashMap::new(),

            #[cfg(feature = "cache")]
            cache_index: Mutex::new(None),

            #[cfg(feature = "fd")]
            fds: Mutex::default(),

            #[cfg(feature = "user")]
            mode: Mutex::default(),

            #[cfg(feature = "elevate")]
            elevate: AtomicBool::new(false),

            #[cfg(feature = "seccomp")]
            seccomp: Mutex::new(None),
        }
    }

    /// Control whether to hook the child's standard input.
    pub fn input(self, input: StreamMode) -> Self {
        self.input_i(input);
        self
    }

    /// Control whether to hook the child's standard output.
    pub fn output(self, output: StreamMode) -> Self {
        self.output_i(output);
        self
    }

    /// Control whether to hook the child's standard error.
    pub fn error(self, error: StreamMode) -> Self {
        self.error_i(error);
        self
    }

    /// Give a unique name to the process, so you can refer to the Handle.
    /// If no name is set, the string passed to Spawn::new() will be used
    pub fn name(self, name: &str) -> Self {
        *self.unique_name.lock() = Some(name.to_string());
        self
    }

    /// Attach another process that is attached to the main child, and should be killed
    /// when the eventual Handle goes out of scope.
    pub fn associate(&self, process: Handle) {
        self.associated.insert(process.name().to_string(), process);
    }

    /// Returns a mutable reference to an associate within the Handle, if it exists.
    /// The associate is another Handle instance.
    pub fn get_associate<'b>(&'b self, name: &str) -> Option<RefMut<'b, String, Handle>> {
        self.associated.get_mut(name)
    }

    /// Drop privilege to the provided user mode on the child,
    /// immediately after the fork. This does not affected the parent
    /// process, but prevents the child from changing outside
    /// of the assigned UID.
    ///
    /// If is set to *Original*, the child is launched with the exact
    /// same operating set as the parent, persisting SetUID privilege.
    ///
    /// If mode is not set, or set to *Existing*, it adopts whatever operating
    /// set the parent is in when spawn() is called. This is ill-advised.
    ///
    /// If the parent is not SetUID, this parameter is a no-op
    #[cfg(feature = "user")]
    pub fn mode(self, mode: user::Mode) -> Self {
        self.mode_i(mode);
        self
    }

    /// Elevate the child to root privilege by using *PolKit* for authentication.
    /// `pkexec` must exist, and must be in path.
    /// The operating set of the child must ensure the real user can
    /// authorize via *PolKit*.
    #[cfg(feature = "elevate")]
    pub fn elevate(self, elevate: bool) -> Self {
        self.elevate_i(elevate);
        self
    }

    /// Preserve the environment of the parent when launching the child.
    /// `Spawner` defaults to clearing the environment.
    pub fn preserve_env(self, preserve: bool) -> Self {
        self.preserve_env_i(preserve);
        self
    }

    /// Add a capability to the child's capability set.
    /// Note that this function cannot grant capability the program
    /// does not possess, it merely prevents existing capabilities from
    /// being cleared.
    pub fn cap(self, cap: Capability) -> Self {
        self.whitelist.insert(cap);
        self
    }

    /// Add capabilities to the child's capability set.
    /// Note that this function cannot grant capability the program
    /// does not possess, it merely prevents existing capabilities from
    /// being cleared.
    pub fn caps(self, caps: impl IntoIterator<Item = Capability>) -> Self {
        caps.into_iter().for_each(|cap| {
            self.whitelist.insert(cap);
        });
        self
    }

    /// Control whether the child is allowed new privileges.
    /// Note that this function cannot grant privilege the program
    /// does not already have, but merely allows it access to existing privileges
    /// not shared by the parent.
    pub fn new_privileges(self, allow: bool) -> Self {
        self.new_privileges_i(allow);
        self
    }

    /// Sets an environment variable to pass to the process.
    /// Note that if preserve_env is set to true, this value will
    /// overwrite the existing value, if it exists.
    pub fn env(
        self,
        key: impl Into<Cow<'a, str>>,
        var: impl Into<Cow<'a, str>>,
    ) -> Result<Self, Error> {
        self.env_i(key, var)?;
        Ok(self)
    }

    /// Passes the value of the provided environment variable to the child.
    /// If preserve_env is true, this is functionally a no-op.
    pub fn pass_env(self, key: impl Into<Cow<'a, str>>) -> Result<Self, Error> {
        self.pass_env_i(key)?;
        Ok(self)
    }

    #[cfg(feature = "seccomp")]
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
    pub fn seccomp(self, seccomp: Filter) -> Self {
        self.seccomp_i(seccomp);
        self
    }

    /// Move a new argument to the argument vector.
    /// This function is guaranteed to append to the end of the current argument
    /// vector.
    pub fn arg(self, arg: impl Into<Cow<'a, str>>) -> Result<Self, Error> {
        self.arg_i(arg)?;
        Ok(self)
    }

    /// Move a new FD to the `Spawner`.
    /// FD's will be shared to the child under the same value.
    /// Any FD's in the parent not explicitly passed will be dropped.
    #[cfg(feature = "fd")]
    pub fn fd(self, fd: impl Into<OwnedFd>) -> Self {
        self.fd_i(fd);
        self
    }

    /// Move a FD to the `Spawner`, and attach it to an argument to ensure the
    /// value is identical.
    ///
    /// ## Example
    /// Bubblewrap supports the --file flag, which accepts a FD and destination.
    /// If you want to ensure you don't accidentally mismatch FDs, you can
    /// commit both the FD and argument in the same transaction:
    ///
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
    /// It is guaranteed that the arguments
    /// in the iterator will appear sequentially, and in the same order.
    pub fn args<I, S>(self, args: I) -> Result<Self, Error>
    where
        I: IntoIterator<Item = S>,
        S: Into<Cow<'a, str>>,
    {
        self.args_i(args)?;
        Ok(self)
    }

    /// Move an iterator of FD's to the `Spawner`.
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
    pub fn input_i(&self, input: StreamMode) {
        *self.input.lock() = input;
    }

    /// Set the output flag without consuming the `Spawner`.
    pub fn output_i(&self, output: StreamMode) {
        *self.output.lock() = output;
    }

    /// Set the error flag without consuming the `Spawner`.
    pub fn error_i(&self, error: StreamMode) {
        *self.error.lock() = error
    }

    #[cfg(feature = "elevate")]
    /// Set the elevate flag without consuming the `Spawner`.
    pub fn elevate_i(&self, elevate: bool) {
        self.elevate.store(elevate, Ordering::Relaxed)
    }

    /// Set the preserve environment flag without consuming the `Spawner`.
    pub fn preserve_env_i(&self, preserve: bool) {
        self.preserve_env.store(preserve, Ordering::Relaxed);
    }

    /// Add a capability without consuming the `Spawner`.
    pub fn cap_i(&mut self, cap: Capability) {
        self.whitelist.insert(cap);
    }

    /// Adds a capability set without consuming the `Spawner`.
    pub fn caps_i(&mut self, caps: impl IntoIterator<Item = Capability>) {
        caps.into_iter().for_each(|cap| {
            self.whitelist.insert(cap);
        });
    }

    /// Set the NO_NEW_PRIVS flag without consuming the `Spawner`.
    pub fn new_privileges_i(&self, allow: bool) {
        self.no_new_privileges.store(!allow, Ordering::Relaxed)
    }

    /// Sets an environment variable to the child process without consuming the `Spawner`.
    pub fn env_i(
        &self,
        key: impl Into<Cow<'a, str>>,
        value: impl Into<Cow<'a, str>>,
    ) -> Result<(), Error> {
        self.env.insert(
            CString::from_str(&key.into()).map_err(Error::Null)?,
            CString::from_str(&value.into()).map_err(Error::Null)?,
        );
        Ok(())
    }

    /// Pass an environment variable to the child process without consuming the `Spawner`.
    pub fn pass_env_i(&self, key: impl Into<Cow<'a, str>>) -> Result<(), Error> {
        let key = key.into();
        let os_key = OsString::from_str(&key).map_err(|_| Error::Environment)?;
        if let Ok(env) = std::env::var(&os_key) {
            self.env.insert(
                CString::from_str(&key).map_err(Error::Null)?,
                CString::from_str(&env).map_err(Error::Null)?,
            );
            Ok(())
        } else {
            Err(Error::Environment)
        }
    }

    /// Set the user mode without consuming the `Spawner`.
    #[cfg(feature = "user")]
    pub fn mode_i(&self, mode: user::Mode) {
        *self.mode.lock() = Some(mode);
    }

    /// Set a *SECCOMP* filter without consuming the `Spawner`.
    #[cfg(feature = "seccomp")]
    pub fn seccomp_i(&self, seccomp: Filter) {
        *self.seccomp.lock() = Some(seccomp)
    }

    /// Move an argument to the `Spawner` in-place.
    pub fn arg_i(&self, arg: impl Into<Cow<'a, str>>) -> Result<(), Error> {
        self.args
            .lock()
            .push(CString::new(arg.into().as_ref()).map_err(Error::Null)?);
        Ok(())
    }

    /// Move a FD to the `Spawner` in-place.
    #[cfg(feature = "fd")]
    pub fn fd_i(&self, fd: impl Into<OwnedFd>) {
        self.fds.lock().push(fd.into());
    }

    /// Move FDs to the `Spawner` in-place, passing it as an argument.
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
    #[cfg(feature = "fd")]
    pub fn fds_i<I, S>(&self, fds: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<OwnedFd>,
    {
        self.fds.lock().extend(fds.into_iter().map(Into::into));
    }

    /// Move an iterator of arguments to the `Spawner` in-place.
    /// Both sequence and order are guaranteed.
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
    /// This function fails if cache_start is called twice without having
    /// first called cache_write.
    ///
    /// ## Examples
    ///
    /// ```rust,ignore
    /// let cache = std::path::PathBuf::from("cmd.cache");
    /// let mut handle = spawn::Spawner::abs("/usr/bin/bash");
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
    pub fn spawn(mut self) -> Result<Handle, Error> {
        // Create our pipes based on whether we need t
        // hem.
        // Because we use these conditionals later on when using them,
        // we can unwrap() with impunity.

        let stdout_mode = self.output.into_inner();
        let stderr_mode = self.error.into_inner();
        let stdin_mode = self.input.into_inner();

        let stdout = cond_pipe(&stdout_mode)?;
        let stderr = cond_pipe(&stderr_mode)?;
        let stdin = cond_pipe(&stdin_mode)?;

        #[cfg(feature = "fd")]
        let fds = self.fds.into_inner();

        let mut cmd_c: Option<CString> = None;
        let mut args_c = Vec::<CString>::new();

        // Launch with pkexec if we're elevated.
        #[cfg(feature = "elevate")]
        if self.elevate.load(Ordering::Relaxed) {
            let polkit = CString::new("/usr/bin/pkexec".to_string()).map_err(Error::Null)?;
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

        // Clear F_SETFD to allow passed FD's to persist after execve
        #[cfg(feature = "fd")]
        for fd in &fds {
            fcntl(fd, FcntlArg::F_SETFD(FdFlag::empty()))
                .map_err(|e| Error::Errno(None, "fnctl fd", e))?;
        }

        if self.preserve_env.load(Ordering::Relaxed) {
            let environment: HashMap<_, _> = std::env::vars_os()
                .filter_map(|(key, value)| {
                    if let Some(key) = key.to_str()
                        && let Some(value) = value.to_str()
                        && let Ok(key) = CString::from_str(key)
                        && let Ok(value) = CString::from_str(value)
                    {
                        if !self.env.contains_key(&key) {
                            Some((key, value))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect();

            self.env.extend(environment)
        }

        let envs: Vec<_> = self
            .env
            .into_iter()
            .filter_map(|(k, v)| {
                // We already know both are valid strings, because we converted them from Str to begin with.
                CString::from_str(&format!("{}={}", k.to_str().unwrap(), v.to_str().unwrap())).ok()
            })
            .collect();

        // Log if desired.
        if log::log_enabled!(log::Level::Trace) {
            let formatted = format_iter(args_c.iter().map(|e| e.to_string_lossy()));
            if !envs.is_empty() {
                let env_formatted = format_iter(envs.iter().map(|e| e.to_string_lossy()));
                trace!("{env_formatted} {formatted}",);
            } else {
                trace!("{formatted}");
            }
        }

        let all = caps::all();
        let set: HashSet<Capability> = self.whitelist.into_iter().collect();
        let diff: CapsHashSet = all.difference(&set).copied().collect();

        #[cfg(feature = "seccomp")]
        let filter = {
            let mut filter = self.seccomp.into_inner();
            if let Some(filter) = &mut filter {
                filter.setup().map_err(Error::Seccomp)?;
            }
            filter
        };

        let fork = unsafe { fork() }.map_err(Error::Fork)?;
        match fork {
            ForkResult::Parent { child } => {
                let name = if let Some(name) = self.unique_name.into_inner() {
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
                    if let StreamMode::Log(log) = stdout_mode {
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
                    if let StreamMode::Log(log) = stderr_mode {
                        let name = name.clone();
                        std::thread::spawn(move || logger(log, read, name));
                        None
                    } else {
                        Some(read)
                    }
                } else {
                    None
                };

                #[cfg(feature = "user")]
                let mode = self.mode.into_inner().unwrap_or(
                    user::current().map_err(|e| Error::Errno(Some(fork), "getresuid", e))?,
                );

                let associated: Vec<Handle> = self.associated.into_iter().map(|(_, v)| v).collect();

                // Return.
                let handle = Handle::new(
                    name,
                    child,
                    #[cfg(feature = "user")]
                    mode,
                    stdin,
                    stdout,
                    stderr,
                    associated,
                );
                Ok(handle)
            }

            ForkResult::Child => {
                if let Some((read, write)) = stdin {
                    let _ = close(write);
                    let _ = dup2_stdin(read);
                } else if let StreamMode::Discard = stdin_mode {
                    let _ = dup2_stdin(dup_null().unwrap());
                }
                #[cfg(feature = "fd")]
                if let StreamMode::Fd(fd) = stdin_mode {
                    let _ = dup2_stdin(fd);
                }

                if let Some((read, write)) = stdout {
                    let _ = close(read);
                    let _ = dup2_stdout(write);
                } else if let StreamMode::Discard = stdout_mode {
                    let _ = dup2_stdout(dup_null().unwrap());
                }
                #[cfg(feature = "fd")]
                if let StreamMode::Fd(fd) = stdout_mode {
                    let _ = dup2_stdout(fd);
                }

                if let Some((read, write)) = stderr {
                    let _ = close(read);
                    let _ = dup2_stderr(write);
                } else if let StreamMode::Discard = stderr_mode {
                    let _ = dup2_stderr(dup_null().unwrap());
                }
                #[cfg(feature = "fd")]
                if let StreamMode::Fd(fd) = stderr_mode {
                    let _ = dup2_stderr(fd);
                }

                let _ = prctl::set_pdeathsig(SIGTERM);

                // Drop modes
                #[cfg(feature = "user")]
                if let Some(mode) = self.mode.into_inner()
                    && let Err(e) = user::drop(mode)
                {
                    warn!("Failed to drop user: {e}")
                }

                clear_capabilities(diff);

                if self.no_new_privileges.load(Ordering::Relaxed)
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

                // Execve
                let _ = execve(&cmd_c, &args_c, &envs);
                exit(-1);
            }
        }
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

        let bytes = handle.output()?.read_bytes(Some(string.len()))?;
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
            .env("USER", user)?
            .output(StreamMode::Pipe)
            .error(StreamMode::Pipe)
            .spawn()?;

        let output = handle.output()?.read_all()?;
        assert!(output.trim() == user);
        Ok(())
    }
}
