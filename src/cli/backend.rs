//! Modifying the datastore

use crate::{
    cli::refresh::installed_profiles,
    shared::{
        config::CONFIG_FILE,
        privileged,
        store::{self, BackingStore, SYSTEM_STORE, Store, StoreType, USER_STORE, init},
    },
};
use anyhow::Result;
use dialoguer::{Confirm, console::style};
use spawn::Spawner;
use std::{
    cell::RefCell,
    thread::LocalKey,
    time::{Duration, Instant},
};

fn cache_bench(state: Store) -> Result<Duration> {
    let now = Instant::now();
    let handle = Spawner::new("antimony")?.args(["refresh"])?;

    if let Store::Database = state {
        handle.env_i("AT_CACHE_DB", "1")?;
    }
    handle
        .preserve_env(true)
        .new_privileges(true)
        .mode(user::Mode::Original)
        .spawn()?
        .wait()?;
    Ok(now.elapsed())
}

fn config_bench(state: Store) -> Result<Duration> {
    let now = Instant::now();
    for profile in installed_profiles()? {
        let handle = Spawner::new("antimony")?;
        if let Store::Database = state {
            handle.env_i("AT_CONFIG_DB", "1")?;
        }
        handle
            .args(["run", &profile, "--dry"])?
            .preserve_env(true)
            .new_privileges(true)
            .mode(user::Mode::Original)
            .spawn()?
            .wait()?;
    }
    Ok(now.elapsed())
}

#[derive(clap::Args, Default)]
pub struct Args {
    /// The new backend to use for the datastore
    pub new: Option<store::Store>,

    /// Perform the operation in place by destructively removing
    /// the existing datastore and digesting it into the new
    /// dataset.
    #[arg(long, default_value_t = false)]
    pub digest: bool,

    /// Configure the cache datastore rather than than the configuration
    /// datastore.
    #[arg(long, default_value_t = false)]
    pub cache: bool,

    /// Overwrite existing values without confirmation.
    #[arg(long, default_value_t = false)]
    pub overwrite: bool,

    /// Replicate the existing backend to new, but don't update the config.
    #[arg(long, default_value_t = false)]
    pub dry: bool,
}

impl super::Run for Args {
    fn run(self) -> Result<()> {
        if !privileged()? {
            Err(anyhow::anyhow!(
                "Modifying the datastore backend is a privileged operation."
            ))
        } else {
            let current = if self.cache {
                CONFIG_FILE.cache_store()
            } else {
                CONFIG_FILE.config_store()
            };

            match self.new {
                Some(new) => {
                    let current = if self.cache {
                        CONFIG_FILE.cache_store()
                    } else {
                        CONFIG_FILE.config_store()
                    };

                    if current == new {
                        println!("Already using selected backend");
                    } else {
                        let digest = |store: &'static LocalKey<RefCell<Box<dyn BackingStore>>>,
                                      dest: Box<dyn BackingStore>|
                         -> Result<()> {
                            store.with_borrow(|s| -> Result<()> {
                            for (object, objects) in store::export(s.as_ref()) {
                                for name in objects {
                                    if dest.exists(&name, object) && !self.overwrite
                                        && (dest.fetch(&name, object)? == s.fetch(&name, object)? || !Confirm::new()
                                            .with_prompt(format!(
                                                "{name} already exists in new backend (within {object}). Overwrite?",
                                            ))
                                            .interact()?) {
                                            continue
                                        }

                                    dest.store(&name, object, &s.fetch(&name, object)?)?;
                                    if self.digest {
                                        s.remove(&name, object)?;
                                    }
                                }
                            }
                            Ok(())
                        })
                        };

                        let update = CONFIG_FILE.clone();

                        if self.cache {
                            update.cache_store.lock().replace(new);
                        } else {
                            digest(&USER_STORE, init(StoreType::User, new))?;
                            digest(&SYSTEM_STORE, init(StoreType::System, new))?;
                            update.config_store.lock().replace(new);
                        }

                        if !self.dry {
                            update.update()?;
                        }
                    }
                }
                None => {
                    let alt = match current {
                        Store::File => Store::Database,
                        Store::Database => Store::File,
                    };

                    // Update the alternate cache_store
                    let update = Args {
                        new: Some(alt),
                        cache: self.cache,
                        dry: true,
                        ..Default::default()
                    };
                    update.run()?;

                    let results = |baseline: Duration, compare: Duration, what: &'static str| {
                        if baseline < compare {
                            println!(
                                "{}",
                                style(format!(
                                    "{what}: {current:?} ({}ms) is faster than {alt:?} ({}ms). Already optimized!", baseline.as_millis(), compare.as_millis()
                                ))
                                .green()
                            );
                        } else {
                            println!(
                                "{}",
                                style(format!(
                                    "{what}: {current:?} ({}ms) is slower than {alt:?} ({}ms). Consider changing backend.",
                                    baseline.as_millis(), compare.as_millis()
                                ))
                                .red()
                            );
                        }
                    };

                    println!("Benchmarking cache...");
                    let baseline = cache_bench(current)?;
                    let compare = cache_bench(alt)?;
                    results(baseline, compare, "cache");

                    println!("Benchmarking config...");
                    let baseline = config_bench(current)?;
                    let compare = config_bench(alt)?;
                    results(baseline, compare, "config");
                }
            }

            Ok(())
        }
    }
}
