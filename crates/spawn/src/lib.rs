//! Process Spawning supporting asynchronous input/output/error capturing,
//! FD passthrough, SetUID mode dropping, SECCOMP filters, and privileged
//! launching.

mod handle;
mod spawn;

pub use handle::Error as HandleError;
pub use handle::Handle;
pub use handle::Stream;
pub use spawn::Error as SpawnError;
pub use spawn::Spawner;
pub use spawn::StreamMode;

fn format_iter<T, V>(iter: T) -> String
where
    T: Iterator<Item = V>,
    V: std::fmt::Display,
{
    let mut ret = String::new();
    iter.for_each(|f| ret.push_str(&format!("{f} ")));
    ret
}
