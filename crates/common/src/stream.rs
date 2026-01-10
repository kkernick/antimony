//! This file contains various utilities dealing with Sockets and Streams.

use nix::{
    errno::Errno,
    poll::{PollFd, PollFlags, PollTimeout},
    sys::socket::{ControlMessageOwned, MsgFlags, recvmsg},
};
use std::{
    io::IoSliceMut,
    os::{
        fd::{AsFd, AsRawFd, FromRawFd, OwnedFd, RawFd},
        unix::net::{UnixListener, UnixStream},
    },
};

/// Poll on Accept, Timing out after timeout.
fn accept_with_timeout(
    listener: &UnixListener,
    timeout: PollTimeout,
) -> Result<Option<UnixStream>, std::io::Error> {
    listener.set_nonblocking(true)?;

    let fd = listener.as_fd();
    let mut fds = [PollFd::new(fd, PollFlags::POLLIN)];
    let res = nix::poll::poll(&mut fds, timeout)?;

    if res == 0 {
        Ok(None)
    } else {
        match listener.accept() {
            Ok((stream, _addr)) => Ok(Some(stream)),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// Receive a file descriptor from a Unix socket as an `OwnedFd`.
pub fn receive_fd(listener: &UnixListener) -> Result<Option<(OwnedFd, String)>, std::io::Error> {
    let stream = accept_with_timeout(listener, PollTimeout::from(1000u16))?;
    if let Some(stream) = stream {
        let mut buf = [0u8; 256];
        let pair = || -> Result<Option<(OwnedFd, usize)>, Errno> {
            let raw_fd = stream.as_raw_fd();

            let mut io = [IoSliceMut::new(&mut buf)];
            let mut msg_space = nix::cmsg_space!([RawFd; 1]);
            let msg = recvmsg::<()>(raw_fd, &mut io, Some(&mut msg_space), MsgFlags::empty())?;
            for cmsg in msg.cmsgs()? {
                if let ControlMessageOwned::ScmRights(fds) = cmsg
                    && let Some(fd) = fds.first()
                {
                    let owned_fd = unsafe { OwnedFd::from_raw_fd(*fd) };
                    return Ok(Some((owned_fd, msg.bytes)));
                }
            }
            Ok(None)
        }()?;

        if let Some((fd, bytes)) = pair {
            let name = String::from_utf8_lossy(&buf[..bytes])
                .trim_end_matches(char::from(0))
                .to_string();
            return Ok(Some((fd, name)));
        }
    }
    Ok(None)
}
