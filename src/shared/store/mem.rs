//! The Memory Backend.
//! This backend shouldn't be used outside of internal usage. It is used by the global refresh
//! so that profiles can share a cache entirely in memory, which is then dumped to the
//! actual backing store all in one go.

use crate::shared::CONFIG_FILE;
use crate::shared::store::{CACHE_STORE, OBJECTS, Object, init_cache};
use common::cache::{self, CacheStatic};
use dashmap::DashMap;
use log::debug;
use rayon::prelude::*;
use std::{
    collections::HashMap,
    sync::{LazyLock, OnceLock},
};

type Table = HashMap<String, Vec<u8>>;

/// The cache store.
static CACHE: CacheStatic<String, DashMap<Object, Table>> = LazyLock::new(DashMap::default);

static MEM_STORE: LazyLock<cache::Cache<String, DashMap<Object, Table>>> =
    LazyLock::new(|| cache::Cache::new(&CACHE));

pub static FLUSH_STORE: OnceLock<super::Store> = OnceLock::new();

pub fn setup() -> anyhow::Result<()> {
    let old = CONFIG_FILE
        .cache_store
        .lock()
        .replace(super::Store::Memory)
        .unwrap_or_default();

    if FLUSH_STORE.set(old).is_err() {
        Err(anyhow::anyhow!("Cannot setup mem cache more than once!"))
    } else {
        Ok(())
    }
}

pub fn flush() -> anyhow::Result<()> {
    if let Some(old) = FLUSH_STORE.get() {
        debug!("Flushing cache to disk...");
        let mut out = init_cache(*old);

        OBJECTS
            .into_iter()
            .try_for_each(|object| -> anyhow::Result<()> {
                if let Ok(map) = CACHE_STORE.with_borrow(|s| s.get(object)) {
                    out.bulk(
                        map.into_par_iter()
                            .filter_map(|name| {
                                match CACHE_STORE.with_borrow(|s| s.bytes(&name, object)) {
                                    Ok(content) => Some((name, content)),
                                    Err(_) => None,
                                }
                            })
                            .collect(),
                        object,
                    )?;
                }
                Ok(())
            })
    } else {
        Err(anyhow::anyhow!(
            "Flush store not initalized! Call setup() first!"
        ))
    }
}

/// The File Store
pub struct Store {
    name: String,
}
impl Store {
    /// Construct a new Memory Store
    pub fn new(name: &str) -> Self {
        if MEM_STORE.get(name).is_none() {
            MEM_STORE.insert(name.to_string(), DashMap::default());
            let db = MEM_STORE.get(name).unwrap();
            for object in super::OBJECTS {
                db.insert(object, HashMap::default());
            }
        }

        Self {
            name: name.to_string(),
        }
    }
}
impl super::BackingStore for Store {
    fn resident(&self) -> bool {
        true
    }

    fn fetch(&self, name: &str, object: Object) -> Result<String, super::Error> {
        let bytes = String::from_utf8(self.bytes(name, object)?)?;
        Ok(bytes)
    }

    fn bytes(&self, name: &str, object: Object) -> Result<Vec<u8>, super::Error> {
        if let Some(db) = MEM_STORE.get(&self.name)
            && let Some(value) = db.get(&object).unwrap().get(name)
        {
            Ok(value.clone())
        } else {
            Err(super::Error::Mem("No such value"))
        }
    }

    fn get(&self, object: Object) -> Result<Vec<String>, super::Error> {
        if let Some(db) = MEM_STORE.get(&self.name) {
            Ok(db.get(&object).unwrap().keys().cloned().collect())
        } else {
            Err(super::Error::Mem("No such object"))
        }
    }

    fn store(&self, name: &str, object: Object, content: &str) -> Result<(), super::Error> {
        self.dump(name, object, content.as_bytes())
    }

    fn bulk(
        &mut self,
        entries: HashMap<String, Vec<u8>>,
        object: super::Object,
    ) -> Result<(), super::Error> {
        entries
            .iter()
            .filter(|(name, _)| !self.exists(name, object))
            .try_for_each(|(name, content)| self.dump(name, object, content))
    }

    fn dump(&self, name: &str, object: Object, content: &[u8]) -> Result<(), super::Error> {
        if let Some(db) = MEM_STORE.get(&self.name) {
            db.get_mut(&object)
                .unwrap()
                .insert(name.to_string(), Vec::from(content));
            Ok(())
        } else {
            Err(super::Error::Mem("No such value"))
        }
    }

    fn exists(&self, name: &str, object: Object) -> bool {
        if let Some(db) = MEM_STORE.get(&self.name)
            && let Some(table) = db.get_mut(&object)
            && table.contains_key(name)
        {
            true
        } else {
            false
        }
    }

    fn remove(&self, name: &str, object: Object) -> Result<(), super::Error> {
        if let Some(db) = MEM_STORE.get(&self.name) {
            db.get_mut(&object).unwrap().remove(name);
            Ok(())
        } else {
            Err(super::Error::Mem("No such value"))
        }
    }
}
