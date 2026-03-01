//! Modifying the datastore

use crate::shared::{
    config::CONFIG_FILE,
    privileged,
    store::{
        self, BackingStore, CACHE_STORE, SYSTEM_STORE, USER_STORE, init_cache, init_system,
        init_user,
    },
};
use anyhow::Result;
use dialoguer::Confirm;
use std::{cell::RefCell, thread::LocalKey};

#[derive(clap::Args, Default)]
pub struct Args {
    /// The new backend to use for the datastore
    new: store::Store,

    /// Perform the operation in place by destructively removing
    /// the existing datastore and digesting it into the new
    /// dataset.
    #[arg(long, default_value_t = false)]
    digest: bool,

    /// Configure the cache datastore rather than than the configuration
    /// datastore.
    #[arg(long, default_value_t = false)]
    cache: bool,

    /// Overwrite existing values without confirmation.
    #[arg(long, default_value_t = false)]
    overwrite: bool,
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

            if current == self.new {
                println!("Already using selected backend");
            } else {
                let digest = |store: &'static LocalKey<RefCell<Box<dyn BackingStore>>>,
                              dest: Box<dyn BackingStore>|
                 -> Result<()> {
                    store.with_borrow(|s| -> Result<()> {
                        for (object, objects) in store::export(s.as_ref()) {
                            for name in objects {
                                if dest.exists(&name, object)
                                    && !self.overwrite
                                    && !Confirm::new()
                                        .with_prompt(format!(
                                            "{name} already exists in new backend (within {object}). Overwrite?",
                                        ))
                                        .interact()?
                                {
                                    continue;
                                } else {
                                    dest.store(&name, object, &s.fetch(&name, object)?)?;
                                    if self.digest {
                                        s.remove(&name, object)?;
                                    }
                                }
                            }
                        }
                        Ok(())
                    })
                };

                let mut update = CONFIG_FILE.clone();
                if self.cache {
                    digest(&CACHE_STORE, init_cache(self.new))?;
                    let mut update = CONFIG_FILE.clone();
                    update.cache_store = Some(self.new);
                } else {
                    digest(&USER_STORE, init_user(self.new))?;
                    digest(&SYSTEM_STORE, init_system(self.new))?;
                    update.config_store = Some(self.new);
                }
                update.update()?;
            }
            Ok(())
        }
    }
}
