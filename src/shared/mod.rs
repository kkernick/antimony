pub mod config;
pub mod db;
pub mod edit;
pub mod env;
pub mod feature;
pub mod path;
pub mod profile;
pub mod syscalls;

pub type Set<T> = std::collections::HashSet<T, ahash::RandomState>;
pub type Map<K, V> = std::collections::HashMap<K, V, ahash::RandomState>;

pub type ISet<T> = IndexSet<T, ahash::RandomState>;
pub type IMap<K, V> = IndexMap<K, V, ahash::RandomState>;

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

pub fn utility(util: &str) -> String {
    AT_HOME
        .join("utilities")
        .join(format!("antimony-{util}"))
        .to_string_lossy()
        .into_owned()
}

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
            .wait()?;
        if code != 0 {
            Err(anyhow::anyhow!("Failed to notify"))
        } else {
            Ok(())
        }
    }();
    result.is_ok()
}

pub fn format_iter<T, V>(iter: T) -> String
where
    T: Iterator<Item = V>,
    V: Display,
{
    let mut ret = String::new();
    iter.for_each(|f| ret.push_str(&format!("{f} ")));
    ret
}

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
use std::fmt::Display;

use indexmap::{IndexMap, IndexSet};
use log::{Level, Record};
use nix::unistd::getpid;
use notify::{level_name, level_urgency};
use spawn::Spawner;
pub use timer;
use user::as_real;

use crate::shared::{config::CONFIG_FILE, env::AT_HOME};
