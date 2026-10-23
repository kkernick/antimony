#![allow(clippy::missing_errors_doc)]

pub mod cache;
pub mod config;
pub mod edit;
pub mod env;
pub mod feature;
pub mod find;
pub mod landlock;
pub mod profile;
pub mod store;
pub mod stream;
pub mod syscalls;
pub mod which;

use crate::shared::{
    config::CONFIG_FILE,
    env::{AT_HOME, CACHE_DIR, SESSION_BUS},
};
use console::Term;
use dashmap::{DashMap, DashSet};
use foldhash::fast::{FixedState, FoldHasher};
use log::{Level, Record};
use notify::{level_name, level_urgency};
use spawn::Spawner;
use std::{
    collections::{HashMap, HashSet},
    fs::{self, metadata},
    hash::BuildHasher,
    io::{self, ErrorKind},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};
use user::USER;

#[derive(Default, Copy, Clone)]
pub struct StaticHash;
impl BuildHasher for StaticHash {
    type Hasher = FoldHasher<'static>;
    fn build_hasher(&self) -> Self::Hasher {
        FixedState::with_seed(0).build_hasher()
    }
}

pub type Set<T> = HashSet<T, StaticHash>;
pub type ThreadSet<T> = DashSet<T, StaticHash>;
pub type Map<K, V> = HashMap<K, V, StaticHash>;
pub type ThreadMap<K, V> = DashMap<K, V, StaticHash>;

/// Create the parent directory of a path
#[inline]
pub fn create_path<'a>(path: impl Into<&'a Path>) -> Result<bool, io::Error> {
    match fs::create_dir_all(path.into()) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(e),
    }
}

/// Create the parent directory of a path
#[inline]
pub fn create_parent<'a>(path: impl Into<&'a Path>) -> Result<bool, io::Error> {
    path.into().parent().map_or_else(
        || Err(ErrorKind::NotFound.into()),
        |parent| match fs::create_dir_all(parent) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(false),
            Err(e) => Err(e),
        },
    )
}

/// Remove a file
#[inline]
pub fn remove_file<'a>(path: impl Into<&'a Path>) -> Result<bool, io::Error> {
    match fs::remove_file(path.into()) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// Remove a directory
#[inline]
pub fn remove_directory<'a>(path: impl Into<&'a Path>) -> Result<bool, io::Error> {
    match fs::remove_dir_all(path.into()) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// Confirm a simple yes/no question
#[inline]
pub fn confirm(question: &str) -> Result<bool, io::Error> {
    let term = Term::stdout();
    term.write_line(&format!("{question}? (y/N): "))?;
    let response = loop {
        let response = term.read_char()?.to_ascii_lowercase();
        if response == 'y' {
            break true;
        } else if response == 'n' {
            break false;
        }
    };
    Ok(response)
}

#[macro_export]
macro_rules! confirm {
    ($($arg:tt)*) => {
        $crate::shared::confirm(&format!($($arg)*))
    };
}

pub fn extend_option<A, T: Extend<A> + IntoIterator<Item = A>>(
    lhs: &mut Option<T>,
    rhs: Option<T>,
) {
    match lhs {
        Some(env) => {
            if let Some(val) = rhs {
                env.extend(val);
            }
        }
        None => *lhs = rhs,
    }
}

/// Extend two
pub fn extend_cmdline<A, X>(lhs: &mut Option<X>, rhs: Option<Vec<A>>)
where
    X: Extend<A> + IntoIterator<Item = A> + FromIterator<A>,
{
    match lhs {
        Some(env) => {
            if let Some(val) = rhs {
                env.extend(val);
            }
        }
        None if let Some(r) = rhs
            && !r.is_empty() =>
        {
            *lhs = Some(r.into_iter().collect());
        }
        _ => {}
    }
}

/// Check that the Real User is privileged. This is used to allow modifying the
/// Antimony system, it does not correlate to actual administrative access.
///
/// Privileged users are specified in the /etc/antimony.{toml/d}
pub fn privileged() -> bool {
    if CONFIG_FILE.is_privileged() {
        true
    } else if let Ok(meta) = metadata(AT_HOME.as_path()) {
        meta.uid() == USER.real.as_raw()
    } else {
        false
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
            .env("DBUS_SESSION_BUS_ADDRESS", SESSION_BUS.as_str())
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
