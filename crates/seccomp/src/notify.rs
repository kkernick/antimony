//! A wrapper for the SECCOMP Notify interface.
//!
//! ## Implementation
//! This implementation does not make any assumptions about how you get the
//! SECCOMP FD. However, there are some considerations you should take into account:
//! * SECCOMP applies across threads. If you place the monitor in a separate thread,
//!   making sending the FD easy, the monitor will be confined by the policy it's monitoring.
//!   This can cause deadlock where the monitor uses a syscall, which the kernel sends an
//!   event for, to which the monitor cannot handle because its currently waiting for itself
//!   to request it.
//! * FDs can be passed across a socket, but you cannot get the SECCOMP FD until you have
//!   loaded the filter. This means you need to ensure that the syscalls used to send the
//!   FD (`connect`, `sendmsg`, etc) are not sent to the notifier, who does not have
//!   the SECCOMP FD yet. `fd_socket` provides functions to send and receive a FD between
//!   processes. See `antimony-monitor`, and Antimony as a whole, to see how you can
//!   notify safely (Hint: Notify all Syscalls except those needed to send FD, which are
//!   instead logged on Audit, with a separate thread for reading the log).
#![cfg(feature = "notify")]

use super::raw;
use nix::errno::Errno;
use std::{
    os::fd::{AsRawFd, RawFd},
    ptr::null_mut,
};

/// Errors regarding to Notify.
#[derive(Debug)]
pub enum Error {
    /// If the pair cannot be allocated.
    Allocation(Errno),

    /// If a request has become invalid
    InvalidId,

    /// If there was an error receiving a request from the kernel.
    Receive(Errno),

    /// If there was an error sending a response to a request.
    Respond(Errno),
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Allocation(errno) => Some(errno),
            Self::Receive(errno) => Some(errno),
            _ => None,
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Allocation(errno) => write!(f, "Failed to allocate notification pair: {errno}"),
            Self::InvalidId => write!(f, "Received ID is no longer valid"),
            Self::Receive(errno) => write!(f, "Failed to receive event: {errno}"),
            Self::Respond(errno) => write!(f, "Failed to respond to event: {errno}"),
        }
    }
}

/// A Notification Pair.
///
/// ## Examples
///
/// ```rust,ignore
/// let pair = seccomp::notify::Pair::new().unwrap();
/// loop {
///     match pair.recv(raw) {
///         Ok(Some(_)) => pair.reply(raw, |req, resp| {
///             resp.val = 0;
///
///             // Deny syscall 1.
///             resp.error = if req.data.nr == 1 {
///                 EPERM
///             } else {
///                 0
///             };
///
///             // Allow everything else.
///             resp.flags = 1;
///         }).unwrap(),
///         Ok(None) => continue,
///         Err(_) => break
///     }
/// }
/// ```
pub struct Pair {
    /// The structure filled by the kernel on new events.
    req: *mut raw::seccomp_notif,

    /// The constructed response to send back.
    resp: *mut raw::seccomp_notif_resp,
}
impl Pair {
    /// Construct a new Pair.
    pub fn new() -> Result<Self, Error> {
        let (req, resp) = unsafe {
            let mut req: *mut raw::seccomp_notif = null_mut();
            let mut resp: *mut raw::seccomp_notif_resp = null_mut();
            match raw::seccomp_notify_alloc(&mut req, &mut resp) {
                0 => (req, resp),
                e => return Err(Error::Allocation(Errno::from_raw(e))),
            }
        };
        Ok(Self { req, resp })
    }

    /// Receive a new event.
    /// This function fails if the kernel returns an error.
    pub fn recv(&self, fd: RawFd) -> Result<Option<()>, Error> {
        // We need to wipe the structure each time.
        unsafe {
            std::ptr::write_bytes(self.req, 0, 1);
        }
        // Call seccomp_notify_receive
        let ret = unsafe { raw::seccomp_notify_receive(fd, self.req) };
        if ret < 0 {
            match Errno::last() {
                Errno::EINTR | Errno::EAGAIN | Errno::ENOENT => Ok(None),
                err => Err(Error::Receive(err)),
            }
        } else {
            Ok(Some(()))
        }
    }

    /// Reply to the last event.
    ///
    /// ## Handle
    /// Handle offloads the actual decision of the request to your application.
    /// It takes a constant reference to the event from the kernel, and a mutable
    /// reference to the response. Parse the former to populate the latter, and
    /// the Pair will send the response over.
    ///
    /// The request will always be valid, and the ID will already be set.
    ///
    pub fn reply<F>(&self, fd: RawFd, handle: F) -> Result<(), Error>
    where
        F: Fn(&raw::seccomp_notif, &mut raw::seccomp_notif_resp),
    {
        let (req, resp) = unsafe { (&*self.req, &mut *self.resp) };

        // Ensure the request is still valid.
        let valid = unsafe { raw::seccomp_notify_id_valid(fd, req.id) };
        if valid != 0 {
            return Err(Error::InvalidId);
        }

        // Set the ID.
        resp.id = req.id;

        // Delegate the decision to the closure.
        handle(req, resp);

        // Send response
        let respond_ret = unsafe { raw::seccomp_notify_respond(fd.as_raw_fd(), self.resp) };
        if respond_ret < 0 {
            Err(Error::Respond(Errno::from_raw(-respond_ret)))
        } else {
            Ok(())
        }
    }
}
impl Drop for Pair {
    fn drop(&mut self) {
        unsafe { raw::seccomp_notify_free(self.req, self.resp) }
    }
}
// The Notify API is Thread Safe, and we're moving the Pair anyways.
unsafe impl Send for Pair {}
