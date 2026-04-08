//! The Memory Backend.
//! This backend shouldn't be used outside of internal usage. It is used by the global refresh
//! so that profiles can share a cache entirely in memory, which is then dumped to the
//! actual backing store all in one go.

use crate::shared::{
    Map, ThreadMap,
    store::{BackingStore, CACHE_STORE, OBJECTS, Object},
};
use common::cache::{self, CacheStatic};
use log::debug;
use std::{any::Any, sync::LazyLock};

type Table = Map<String, Vec<u8>>;

#[inline]
pub fn flush() {
    CACHE_STORE.with_borrow(|s| {
        if let Some(cache) = s.as_any().downcast_ref::<Store>() {
            let _ = cache.flush();
        }
    })
}

/// The cache store.
static CACHE: CacheStatic<String, ThreadMap<Object, Table>> = LazyLock::new(ThreadMap::default);

static MEM_STORE: LazyLock<cache::Cache<String, ThreadMap<Object, Table>>> =
    LazyLock::new(|| cache::Cache::new(&CACHE));

/// The File Store
pub struct Store {
    name: String,
    backend: Box<dyn BackingStore>,
    read: bool,
}
impl Store {
    /// Construct a new Memory Store
    ///
    /// The Memory Store always acts as as write-cache. dump/store will write into
    /// memory, with an explicit call to Store::flush() required to write to the
    /// underlying disk.
    ///
    /// It can *also* act as a read-cache. Though this depends on correctly setting
    /// the read argument. When read is true, get/fetch will query the cache, and
    /// if it's empty will fetch from disk and load that data into memory. If read
    /// is false, only memory is checked.
    pub fn new(name: &str, backend: Box<dyn BackingStore>, read: bool) -> Self {
        if MEM_STORE.get(name).is_none() {
            MEM_STORE.insert(name.to_string(), ThreadMap::default());
            let db = MEM_STORE.get(name).unwrap();
            for object in super::OBJECTS {
                db.insert(object, Map::default());
            }
        }

        Self {
            name: name.to_string(),
            backend,
            read,
        }
    }

    pub fn flush(&self) -> Result<(), super::Error> {
        debug!("Flushing to disk...");
        OBJECTS
            .into_iter()
            .try_for_each(|object| -> Result<(), super::Error> {
                if let Ok(map) = self.get(object) {
                    self.backend.bulk(
                        map.into_iter()
                            .filter_map(|name| match self.bytes(&name, object) {
                                Ok(content) => Some((name, content)),
                                Err(_) => None,
                            })
                            .collect(),
                        object,
                    )?;
                }
                Ok(())
            })
    }
}
impl super::BackingStore for Store {
    #[inline]
    fn as_any(&self) -> &dyn Any {
        self
    }

    #[inline]
    fn resident(&self) -> bool {
        true
    }

    #[inline]
    fn fetch(&self, name: &str, object: Object) -> Result<String, super::Error> {
        let bytes = String::from_utf8(self.bytes(name, object)?)?;
        Ok(bytes)
    }

    fn bytes(&self, name: &str, object: Object) -> Result<Vec<u8>, super::Error> {
        if let Some(db) = MEM_STORE.get(&self.name)
            && let Some(value) = db.get(&object).unwrap().get(name)
        {
            Ok(value.clone())
        } else if self.read
            && let Ok(bytes) = self.backend.bytes(name, object)
        {
            self.dump(name, object, &bytes)?;
            Ok(bytes)
        } else {
            Err(super::Error::Mem("No such value"))
        }
    }

    fn get(&self, object: Object) -> Result<Vec<String>, super::Error> {
        if let Some(db) = MEM_STORE.get(&self.name) {
            Ok(db.get(&object).unwrap().keys().cloned().collect())
        } else if self.read
            && let Ok(disk) = self.backend.get(object)
        {
            Ok(disk)
        } else {
            Err(super::Error::Mem("No such object"))
        }
    }

    #[inline]
    fn store(&self, name: &str, object: Object, content: &str) -> Result<(), super::Error> {
        self.dump(name, object, content.as_bytes())
    }

    fn bulk(
        &self,
        entries: Map<String, Vec<u8>>,
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
        } else if !self.read {
            self.backend.exists(name, object)
        } else {
            false
        }
    }

    #[inline]
    fn remove(&self, name: &str, object: Object) -> Result<(), super::Error> {
        if let Some(db) = MEM_STORE.get(&self.name) {
            db.get_mut(&object).unwrap().remove(name);
            Ok(())
        } else {
            Err(super::Error::Mem("No such value"))
        }
    }
}
