#![doc = include_str!("../README.md")]

mod handle;
mod spawn;

use caps::{CapSet, CapsHashSet};
use log::warn;
use nix::unistd::{dup, pipe};
use std::os::fd::AsFd;
use std::{os::fd::OwnedFd, sync::LazyLock};

pub use handle::Error as HandleError;
pub use handle::Handle;
pub use handle::Stream;
pub use spawn::Error as SpawnError;
pub use spawn::Spawner;
pub use spawn::StreamMode;

#[cfg(feature = "fork")]
pub use fork::Error as ForkError;

#[cfg(feature = "fork")]
pub use fork::Fork;

#[cfg(feature = "fork")]
mod fork;

/// An OwnedFd pointing to /dev/null, duplicated for
/// StreamMode::Discard.
static NULL: LazyLock<OwnedFd> = LazyLock::new(|| {
    std::fs::File::open("/dev/null")
        .expect("Failed to open /dev/null")
        .into()
});

/// Format an iterator into a string.
fn format_iter<T, V>(iter: T) -> String
where
    T: Iterator<Item = V>,
    V: std::fmt::Display,
{
    let mut ret = String::new();
    iter.for_each(|f| ret.push_str(&format!("{f} ")));
    ret
}

/// Clears the capabilities of the current thread.
fn clear_capabilities(diff: CapsHashSet) {
    for set in [
        CapSet::Ambient,
        CapSet::Ambient,
        CapSet::Effective,
        CapSet::Inheritable,
        CapSet::Permitted,
    ] {
        for cap in &diff {
            if let Err(e) = caps::drop(None, set, *cap) {
                warn!("Could not drop {cap}: {e}");
            }
        }
    }
}

/// Create a duplicate FD pointing to /dev/null
fn dup_null() -> Result<OwnedFd, SpawnError> {
    dup(NULL.as_fd()).map_err(|e| SpawnError::Errno(None, "dup", e))
}

/// Conditionally create a pipe.
/// Returns either a set of `None`, or the result of `pipe()`
fn cond_pipe(cond: &StreamMode) -> Result<Option<(OwnedFd, OwnedFd)>, SpawnError> {
    match cond {
        StreamMode::Pipe | StreamMode::Log(_) => match pipe() {
            Ok((r, w)) => Ok(Some((r, w))),
            Err(e) => Err(SpawnError::Errno(None, "pipe", e)),
        },
        _ => Ok(None),
    }
}

/// Log all activity from the child at the desired level.
fn logger(level: log::Level, fd: OwnedFd, name: String) {
    let stream = Stream::new(fd);
    while let Some(line) = stream.read_line() {
        log::log!(level, "{name}: {line}");
    }
}
