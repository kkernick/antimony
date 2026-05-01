#![doc = include_str!("../README.md")]

mod handle;
mod spawn;

use caps::{CapSet, CapsHashSet};
use log::warn;
use nix::unistd::{dup, pipe};
use std::fmt::{self, Write};
use std::fs::File;
use std::os::fd::AsFd;
use std::{os::fd::OwnedFd, sync::LazyLock};

pub use handle::Error as HandleError;
pub use handle::Handle;
pub use handle::Stream;
pub use spawn::Error as SpawnError;
pub use spawn::Spawner;
pub use spawn::StreamMode;

/// An `OwnedFd` pointing to /dev/null, duplicated for
/// `StreamMode::Discard`.
static NULL: LazyLock<OwnedFd> = LazyLock::new(|| {
    File::open("/dev/null")
        .expect("Failed to open /dev/null")
        .into()
});

/// Format an iterator into a string.
fn format_iter<T, V>(iter: T) -> String
where
    T: Iterator<Item = V>,
    V: fmt::Display,
{
    let mut ret = String::new();
    iter.for_each(|f| {
        let _ = write!(ret, "{f} ");
    });
    ret
}

/// Clears the capabilities of the current thread.
fn clear_capabilities(diff: &CapsHashSet) {
    for set in [
        CapSet::Ambient,
        CapSet::Ambient,
        CapSet::Effective,
        CapSet::Inheritable,
        CapSet::Permitted,
    ] {
        for cap in diff {
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
fn logger(level: log::Level, fd: OwnedFd, name: &str) {
    let stream = Stream::new(fd);
    while let Some(line) = stream.read_line() {
        log::log!(level, "{name}: {line}");
    }
}

#[cfg(test)]
mod tests {
    use crate::{Spawner, StreamMode};
    use anyhow::Result;
    use std::io::Write;

    #[test]
    fn bash() -> Result<()> {
        let string = "Hello, World!";
        let mut handle = Spawner::new("bash")?
            .args(["-c", &format!("echo '{string}'")])
            .output(StreamMode::Pipe)
            .error(StreamMode::Pipe)
            .spawn()?;

        let output = handle.output()?.read_blocking()?;
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

        let output = handle.output()?.read_blocking()?;
        assert!(output.trim() == string);
        Ok(())
    }

    #[test]
    fn read() -> Result<()> {
        let string = "Hello!";
        let mut handle = Spawner::new("echo")?
            .arg(string)
            .output(StreamMode::Pipe)
            .spawn()?;

        let bytes = handle.output()?.read_bytes(Some(string.len()));
        let output = String::from_utf8_lossy(&bytes);
        assert!(output.trim() == string);
        Ok(())
    }

    #[test]
    fn clear_env() -> Result<()> {
        let mut handle = Spawner::new("bash")?
            .args(["-c", "echo $USER"])
            .output(StreamMode::Pipe)
            .error(StreamMode::Pipe)
            .spawn()?;

        let output = handle.output()?.read_blocking()?;
        assert!(output.trim().is_empty());
        Ok(())
    }

    #[test]
    fn preserve_env() -> Result<()> {
        let user = "Test";
        let mut handle = Spawner::new("bash")?
            .args(["-c", "echo $USER"])
            .env("USER", user)
            .output(StreamMode::Pipe)
            .error(StreamMode::Pipe)
            .spawn()?;

        let output = handle.output()?.read_blocking()?;
        assert!(output.trim() == user);
        Ok(())
    }
}
