#![allow(clippy::missing_errors_doc)]

pub mod config;
pub mod edit;
pub mod env;
pub mod feature;
pub mod profile;
pub mod store;
pub mod syscalls;

use crate::shared::{
    config::CONFIG_FILE,
    env::{AT_HOME, CACHE_DIR},
};
use dashmap::{DashMap, DashSet};
use log::{Level, Record};
use nix::unistd::getpid;
use notify::{level_name, level_urgency};
use spawn::Spawner;
use std::{
    collections::{HashMap, HashSet},
    fs::metadata,
    hash::BuildHasher,
    os::unix::fs::MetadataExt,
    path::PathBuf,
};
use user::{USER, as_real};

#[derive(Default, Copy, Clone)]
pub struct StaticHash;
impl BuildHasher for StaticHash {
    type Hasher = ahash::AHasher;
    fn build_hasher(&self) -> Self::Hasher {
        ahash::RandomState::with_seeds(0, 0, 0, 0).build_hasher()
    }
}

pub type Set<T> = HashSet<T, StaticHash>;
pub type ThreadSet<T> = DashSet<T, StaticHash>;
pub type Map<K, V> = HashMap<K, V, StaticHash>;
pub type ThreadMap<K, V> = DashMap<K, V, StaticHash>;

/// Check that the Real User is privileged. This is used to allow modifying the
/// Antimony system, it does not correlate to actual administrative access (i.e. sudo/polkit)
pub fn privileged() -> anyhow::Result<bool> {
    if CONFIG_FILE.is_privileged() {
        Ok(true)

    // If the running user owns AT_HOME, they don't need permission checks.
    } else if let Ok(meta) = metadata(AT_HOME.as_path())
        && meta.uid() == USER.real.as_raw()
    {
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
                ])
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

/// Our notify logger implementation. Because Antimony runs `SetUID`, we have to
/// spawn a separate process to access the user bus.
#[must_use]
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
            ])
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

/// Get where direct files should be placed.
#[inline]
pub fn direct_path(file: &str) -> PathBuf {
    CACHE_DIR.join(".direct").join(&file[1..])
}

#[cfg(debug_assertions)]
#[allow(clippy::absolute_paths)]
pub static TIME_MAP: std::sync::LazyLock<ThreadMap<&'static str, std::num::Saturating<u128>>> =
    std::sync::LazyLock::new(ThreadMap::default);

/// Debug macro to record how long something took, but only in developer builds.
/// On release builds, this does nothing.
#[macro_export]
macro_rules! timer {
    ($name:literal, $body:block) => {{
        #[cfg(debug_assertions)]
        {
            use std::ops::AddAssign;
            let start = std::time::Instant::now();
            let result = $body;
            let elapsed = start.elapsed().as_micros();
            $crate::shared::TIME_MAP
                .entry($name)
                .or_default()
                .value_mut()
                .add_assign(elapsed);
            result
        }

        #[cfg(not(debug_assertions))]
        $body
    }};

    ($name:literal, $expr:expr) => {{
        #[cfg(debug_assertions)]
        {
            use std::ops::AddAssign;
            let start = std::time::Instant::now();
            let result = $expr;
            let elapsed = start.elapsed().as_micros();
            $crate::shared::TIME_MAP
                .entry($name)
                .or_default()
                .value_mut()
                .add_assign(elapsed);
            result
        }

        #[cfg(not(debug_assertions))]
        $expr
    }};
}
pub use timer;
