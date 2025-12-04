//! A wrapper around a SECCOMP context.
use super::{action::Action, attribute::Attribute, raw, syscall::Syscall};
use nix::errno::Errno;
use std::{
    error, fmt,
    fs::File,
    io,
    os::fd::{IntoRawFd, OwnedFd},
    path::{Path, PathBuf},
};

#[cfg(feature = "notify")]
use std::os::fd::FromRawFd;

/// Errors related to filter generation.
#[derive(Debug)]
pub enum Error {
    /// Failure to initialize the context
    Initialization,

    /// Failed to set attribute.
    SetAttribute(Attribute, Errno),

    /// Failed to add rule.
    AddRule(Action, Syscall, Errno),

    /// Failed to write out as BPF
    Io(PathBuf, io::Error),

    /// Failed to export the SECCOMP to BPF.
    Export(Errno),

    /// Failed to load the policy into the process.
    Load(Errno),

    /// Failed to send the SECCOMP FD to the monitor.
    #[cfg(feature = "notify")]
    Send,
}
impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::SetAttribute(_, errno) => Some(errno),
            Self::AddRule(_, _, errno) => Some(errno),
            Self::Io(_, error) => Some(error),
            Self::Export(errno) => Some(errno),
            Self::Load(errno) => Some(errno),
            _ => None,
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Initialization => write!(f, "Failed to initialization the Filter context"),
            Self::SetAttribute(attr, errno) => write!(f, "Failed to set attribute {attr}: {errno}"),
            Self::AddRule(action, syscall, errno) => {
                write!(f, "Failed to add rule {action} = {syscall}: {errno}")
            }
            Self::Io(path, error) => {
                write!(f, "IO error {}: {error}", path.to_string_lossy())
            }
            Self::Export(errno) => {
                write!(f, "Failed to export to BPF: {errno}",)
            }
            Self::Load(errno) => {
                write!(f, "Failed to load filter: {errno}",)
            }
            #[cfg(feature = "notify")]
            Self::Send => {
                write!(f, "Failed to send notify FD",)
            }
        }
    }
}

/// A trait for transmitting a SECCOMP Notify FD to a Monitor.
///
///  When `Filter::load()` is called, the Filter will execute the following
/// actions in the following order:
///
/// 1. Call `Notifier::exempt()`
/// 2. Call `Notifier::prepare()`
/// 3. Call `seccomp_load()`
/// 4. Call `Notifier::handle()`
///
/// Then, you should call `execve()`.
/// See `antimony::shared::syscalls::Notifier` for a socket implementation.
#[cfg(feature = "notify")]
pub trait Notifier: Send + 'static {
    /// Return the list of syscalls that are used by the Notifier itself
    /// in order to transmit the SECCOMP FD. These syscalls will be used
    /// between `seccomp_load()` and `execve()`. For example, if sending
    /// the FD across a socket, you should pass `sendmsg`.
    ///
    /// The action should NOT be Notify, as that will cause a deadlock.
    /// Instead, either Allow, or Log.
    fn exempt(&self) -> Vec<(Action, Syscall)> {
        Vec::new()
    }

    /// Prepare for `seccomp_load`. This function is the last thing run
    /// before `seccomp_load`, and as such is the last time you will
    /// not be confined by the Filter. This can be used, for example,
    /// to wait for a socket, then connect to it.
    fn prepare(&mut self) {}

    /// Handle the SECCOMP FD. This function runs under the confined
    /// SECCOMP Filter, and should transmit the OwnedFD to the
    /// Notify Monitor. The more you do here, the more syscalls
    /// you will need; consider moving as much as possible to
    /// `prepare()`
    fn handle(&mut self, fd: OwnedFd);
}

/// The Filter is a wrapper around a SECCOMP Context.
///
/// This implementation has first-class support for the SECCOMP Notify
/// API, but a lot of the logic needs to be implemented in the
/// application. Firstly, implement the `Notifier` trait for
/// the calling process (The one that loads the filter). Then,
/// use a `notify::Pair` on the monitoring process. A working
/// implementation of both exist in Antimony.
///
/// ## Examples
///
/// Load a basic rule that logs everything but `read`.
/// ```rust
/// use seccomp::{filter::Filter, action::Action, attribute::Attribute, syscall::Syscall};
/// let mut filter = Filter::new(Action::Log).unwrap();
/// filter.set_attribute(Attribute::NoNewPrivileges(true)).unwrap();
/// filter.add_rule(Action::Allow, Syscall::from_name("read").unwrap()).unwrap();
/// filter.load();
/// ```
///
pub struct Filter {
    ctx: raw::scmp_filter_ctx,

    #[cfg(feature = "notify")]
    notifier: Option<Box<dyn Notifier>>,
}
impl Filter {
    /// Construct a new filter with a default action.
    ///
    /// If you plan to use Notify, use `new_notify` instead
    pub fn new(def_action: Action) -> Result<Self, Error> {
        let ctx = unsafe { raw::seccomp_init(def_action.into()) };
        if ctx.is_null() {
            Err(Error::Initialization)
        } else {
            #[cfg(feature = "notify")]
            return Ok(Self {
                ctx,
                notifier: None,
            });

            #[cfg(not(feature = "notify"))]
            return Ok(Self { ctx });
        }
    }

    /// Set a notifier monitor process. See the Notifier trait for more information.
    #[cfg(feature = "notify")]
    pub fn set_notifier(&mut self, f: impl Notifier) {
        self.notifier = Some(Box::new(f))
    }

    /// Set an attribute.
    pub fn set_attribute(&mut self, attr: Attribute) -> Result<(), Error> {
        match unsafe { raw::seccomp_attr_set(self.ctx, attr.name(), attr.value()) } {
            0 => Ok(()),
            e => Err(Error::SetAttribute(attr, Errno::from_raw(e))),
        }
    }

    /// Add a rule. Complex rules are not supported.
    pub fn add_rule(&mut self, action: Action, syscall: Syscall) -> Result<(), Error> {
        match unsafe { raw::seccomp_rule_add(self.ctx, action.into(), syscall.into(), 0) } {
            0 => Ok(()),
            e => Err(Error::AddRule(action, syscall, Errno::from_raw(e))),
        }
    }

    /// Consumes and Write the filter to a new file with the BPF format of the filter.
    pub fn write(&self, path: &Path) -> Result<OwnedFd, Error> {
        let file = File::create(path).map_err(|e| Error::Io(path.to_path_buf(), e))?;
        match unsafe { raw::seccomp_export_bpf(self.ctx, file.into_raw_fd()) } {
            0 => Ok(File::open(path)
                .map_err(|e| Error::Io(path.to_path_buf(), e))?
                .into()),
            e => Err(Error::Export(Errno::from_raw(e))),
        }
    }

    /// Loads the policy, optionally executing a Notifier function.
    pub fn load(mut self) -> Result<(), Error> {
        #[cfg(feature = "notify")]
        if let Some(mut notifier) = self.notifier.take() {
            // Add exempt calls.
            for (action, call) in notifier.exempt() {
                self.add_rule(action, call)?
            }

            // Prepare
            notifier.prepare();

            // Load
            return match unsafe { raw::seccomp_load(self.ctx) } {
                0 => {
                    // Handle
                    let fd = unsafe { OwnedFd::from_raw_fd(raw::seccomp_notify_fd(self.ctx)) };
                    notifier.handle(fd);
                    Ok(())
                }
                errno => Err(Error::Load(Errno::from_raw(errno))),
            };
        }

        match unsafe { raw::seccomp_load(self.ctx) } {
            0 => Ok(()),
            err => Err(Error::Load(Errno::from_raw(err))),
        }
    }
}
impl Drop for Filter {
    fn drop(&mut self) {
        unsafe { raw::seccomp_release(self.ctx) }
    }
}

// The filter can be shared across threads, but it
// cannot be modified simultaneously
unsafe impl Sync for Filter {}
