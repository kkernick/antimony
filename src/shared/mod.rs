pub mod config;
pub mod edit;
pub mod env;
pub mod feature;
pub mod profile;
pub mod syscalls;

use crate::shared::{
    config::CONFIG_FILE,
    env::{AT_HOME, CACHE_DIR, RUNTIME_DIR},
};
use indexmap::{IndexMap, IndexSet};
use log::{Level, Record};
use nix::unistd::getpid;
use notify::{level_name, level_urgency};
use spawn::Spawner;
use std::{fmt::Display, hash::BuildHasher, path::PathBuf};
use user::as_real;

#[derive(Default, Copy, Clone)]
pub struct StaticHash;
impl BuildHasher for StaticHash {
    type Hasher = ahash::AHasher;
    fn build_hasher(&self) -> Self::Hasher {
        ahash::RandomState::with_seeds(0, 0, 0, 0).build_hasher()
    }
}

pub type Set<T> = IndexSet<T, StaticHash>;
pub type Map<K, V> = IndexMap<K, V, StaticHash>;

/// Check that the Real User is privileged. This is used to allow modifying the
/// Antimony system, it does not correlate to actual administrative access (IE sudo/polkit)
pub fn privileged() -> anyhow::Result<bool> {
    if CONFIG_FILE.is_privileged() {
        Ok(true)
    } else {
        Ok(as_real!(anyhow::Result<i32>, {
            Ok(Spawner::abs("/usr/bin/pkcheck")
                .args([
                    "--action-id",
                    "org.freedesktop.policykit.exec",
                    "--allow-user-interaction",
                    "--process",
                    &format!("{}", getpid().as_raw()),
                ])?
                .mode(user::Mode::Real)
                .preserve_env(true)
                .error(spawn::StreamMode::Discard)
                .output(spawn::StreamMode::Discard)
                .spawn()?
                .wait()?)
        })?? == 0)
    }
}

/// Get the path to a utility.
#[inline]
pub fn utility(util: &str) -> String {
    AT_HOME
        .join("utilities")
        .join(format!("antimony-{util}"))
        .to_string_lossy()
        .into_owned()
}

/// Our notify logger implementation. Because Antimony runs SetUID, we have to
/// spawn a separate process to access the user bus.
pub fn logger(record: &Record, level: Level) -> bool {
    let result = || -> anyhow::Result<()> {
        let code = Spawner::abs(utility("notify"))
            .mode(user::Mode::Real)
            .pass_env("DBUS_SESSION_BUS_ADDRESS")?
            .args([
                "--title",
                &format!("{}: {}", level_name(level), record.target()),
                "--body",
                &format!("{}", record.args()),
                "--urgency",
                &format!("{}", level_urgency(level)),
            ])?
            .spawn()?
            .wait_blocking()?;
        if code != 0 {
            Err(anyhow::anyhow!("Failed to notify"))
        } else {
            Ok(())
        }
    }();
    result.is_ok()
}

/// Format an iterator into a string.
pub fn format_iter<T, V>(iter: T) -> String
where
    T: Iterator<Item = V>,
    V: Display,
{
    let mut ret = String::new();
    iter.for_each(|f| ret.push_str(&format!("{f} ")));
    ret
}

/// The user dir is where the instance information is stored.
#[inline]
pub fn user_dir(instance: &str) -> PathBuf {
    PathBuf::from(RUNTIME_DIR.as_path())
        .join("antimony")
        .join(instance)
}

/// Get where direct files should be placed.
#[inline]
pub fn direct_path(file: &str) -> PathBuf {
    CACHE_DIR.join(".direct").join(&file[1..])
}

/// Debug macro to record how long something took, but only in developer builds.
/// On release builds, this does nothing.
#[macro_export]
macro_rules! timer {
    ($name:literal, $body:block) => {{
        #[cfg(debug_assertions)]
        {
            let start = std::time::Instant::now();
            let result = $body;
            log::info!("{}: {}us", $name, start.elapsed().as_micros());
            result
        }

        #[cfg(not(debug_assertions))]
        $body
    }};

    ($name:literal, $expr:expr) => {{
        #[cfg(debug_assertions)]
        {
            log::debug!("Starting {}", $name);
            let start = std::time::Instant::now();
            let result = $expr;
            log::info!("{}: {}us", $name, start.elapsed().as_micros());
            result
        }

        #[cfg(not(debug_assertions))]
        $expr
    }};
}
pub use timer;
