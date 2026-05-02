//! The Memory Backend.
//!
//! This is not a backend in-and-of-itself. Instead, it wraps an existing backend and caches requests
//! to the disk. This has three large advantages:
//!
//! 1. On refresh, we can set read to false, and the profile will need to recreate all records from
//!    scratch. This avoids the prior clumsy approach of simply deleting all cached records, even
//!    those the profile doesn't recreate. It sees an empty store, regardless of what is present on
//!    disk, and then flushes the update records (And ONLY the updated records)
//! 2. On global refresh, the cache can be built cooperatively, then flushed in one operation. Rather
//!    than each profile reading/writing to disk, and using costly disk reads to use the work of a
//!    prior profile, everything is cached in memory, and the entire cache is dumped at the very end.
//!    When you run `antimony refresh`, everything is done in memory until the final flush.
//! 3. On regular runs, profiles can fetch cached records from disk, and subsequent reads will be
//!    cached--only a single disk fetch is needed for any given record.
//!
//! If you haven't already gathered, disk IO is one of the most expensive parts of both the hot and
//! cold paths, so doing as little as possible is hugely advantageous. The abstract nature of this
//! backend (And Backends in general) made slightly more sense when there was more than just File
//! for the principal backends, but more Backends may be created in the future, so the dynamic nature
//! of the Memory Cache will remain until I see fit to change it :)

use crate::shared::{
    Map, Set,
    store::{BackingStore, CACHE_STORE, OBJECTS, Object},
};
use common::cache::{self, CacheStatic};
use dashmap::DashMap;
use log::debug;
use std::{any::Any, sync::LazyLock};

/// A mapping of names to bytes
type Table = Map<String, Vec<u8>>;

#[inline]
pub fn flush() {
    let store = CACHE_STORE.borrow();
    if let Some(cache) = store.as_any().downcast_ref::<Store>() {
        let _ = cache.flush();
    }
}

/// The cache store.
static CACHE: CacheStatic<String, DashMap<Object, Table>> = LazyLock::new(DashMap::default);

/// The front end to the cache.
static MEM_STORE: LazyLock<cache::Cache<String, DashMap<Object, Table>>> =
    LazyLock::new(|| cache::Cache::new(&CACHE));

/// The Memory Store
pub struct Store {
    /// The name of the store we are caching.
    name: String,

    /// The actual store
    backend: Box<dyn BackingStore + Send + Sync>,

    /// Whether we act as a read cache on top of a write cache.
    ///
    /// Effectively, the bool controls whether profiles are allowed
    /// to read from disk. For refreshing, we want to recreate all
    /// definitions, so disallow reading to disk by setting this to
    /// false.
    ///
    /// For regular profile running, we want to fetch from disk if
    /// the record isn't in the cache, but then store it in memory
    /// and use the cached copy on subsequent reads.
    read: bool,
}
impl Store {
    /// Construct a new Memory Store
    ///
    /// The Memory Store always acts as as write-cache. dump/store will write into
    /// memory, with an explicit call to `Store::flush()` required to write to the
    /// underlying disk.
    ///
    /// It can *also* act as a read-cache. Though this depends on correctly setting
    /// the read argument. When read is true, get/fetch will query the cache, and
    /// if it's empty will fetch from disk and load that data into memory. If read
    /// is false, only memory is checked.
    #[must_use]
    pub fn new(name: &str, backend: Box<dyn BackingStore + Send + Sync>, read: bool) -> Self {
        if MEM_STORE.get(name).is_none() {
            let db = MEM_STORE.insert(name.to_owned(), DashMap::default());
            for object in super::OBJECTS {
                db.insert(object, Map::default());
            }
        }
        Self {
            name: name.to_owned(),
            backend,
            read,
        }
    }

    /// Flush the memory store
    ///
    /// ## Errors
    /// If any record could not be flushed to the underlying store.
    pub fn flush(&self) -> Result<(), super::Error> {
        debug!("Flushing to disk...");
        OBJECTS
            .into_iter()
            .try_for_each(|object| -> Result<(), super::Error> {
                if let Ok(map) = self.get(object) {
                    self.backend.bulk(
                        map.into_iter()
                            .filter_map(|name| {
                                self.bytes(&name, object)
                                    .map_or(None, |content| Some((name, content)))
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
            && let Some(value) = db.entry(object).or_default().get(name)
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

    fn get(&self, object: Object) -> Result<Set<String>, super::Error> {
        MEM_STORE.get(&self.name).map_or_else(
            || {
                if self.read
                    && let Ok(disk) = self.backend.get(object)
                {
                    Ok(disk)
                } else {
                    Err(super::Error::Mem("No such object"))
                }
            },
            |db| Ok(db.entry(object).or_default().keys().cloned().collect()),
        )
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
        MEM_STORE.get(&self.name).map_or_else(
            || Err(super::Error::Mem("No such value")),
            |db| {
                db.entry(object)
                    .or_default()
                    .insert(name.to_owned(), Vec::from(content));
                Ok(())
            },
        )
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
        MEM_STORE.get(&self.name).map_or_else(
            || Err(super::Error::Mem("No such value")),
            |db| {
                db.entry(object).or_default().remove(name);
                Ok(())
            },
        )
    }
}
unsafe impl Sync for Store {}
unsafe impl Send for Store {}
