//! This file contains an interface to a static DashMap, such that the stored values can be extracted with static lifetimes,
//! and without a Ref Guard.
//!
//! The underlying implementation stores all values within an Arc. When a value is requested, the interior Arc's address is
//! returned directly. This is safe, as:
//!
//! 1.  The static DashMap holds a strong reference to the Arc, so its lifetime is static. There is no risk of a reference
//!     becoming invalid.
//! 2.  Because the DashMap stores an Arc, the interior address of the value is outside the realm of the Map. Therefore,
//!     insertions and deletions will not alter the value within the Arc, even if it modifies the address of the Arc
//!     itself.cache
//! 3.  Typical Rust thread-safety and reference rules still apply. You can take as many immutable references as you want,
//!     but only a single mutable reference. This means, if you need to query the data across multiple threads, you'll
//!     need to wrap the data in a Mutex or similar interior-mutable structure.
//!
//! To use this, you will need to define two static values:
//!
//! 1.  The DashMap itself. Define a new static cache::CacheStatic with the Key and Value types needed.
//! 2.  Instantiate a static of the cache::Cache object within a LazyLock, with its value taking a reference
//!     to the above DashMap.
//!
//! Then, you can use the Cache to retrieve static values, and insert them.

use dashmap::{DashMap, mapref::one::Ref};
use std::{
    borrow::Borrow,
    hash::Hash,
    sync::{Arc, LazyLock},
};

/// The underlying data store. You should not use this besides passing it to the Cache instance.
pub type CacheStatic<K, V> = LazyLock<DashMap<K, Arc<V>, ahash::RandomState>>;

/// The Cache interface. Store within a LazyLock, and pass the corresponding CacheStatic to the LazyLock::new()
pub struct Cache<K: Eq + Hash + 'static, V: 'static> {
    container: &'static CacheStatic<K, V>,
}
impl<K: Eq + Hash + Clone + 'static, V: 'static> Cache<K, V> {
    /// Extract the value from within the Arc.
    /// This function is safe so long as the Arc is held by a static container
    fn get_static(arc: Ref<K, Arc<V>>) -> &'static V {
        // Get the arc from within the reference, bumping the reference count by one.
        let arc = arc.value().clone();

        // Consume the arc to get the raw pointer, decreasing the reference back down.
        let raw_ptr: *const V = Arc::into_raw(arc);

        // Because the Arc is contained in a static container, the lifetime of the interior
        // value is likewise static, so this is safe.
        //
        // Also note that because we are storing in an Arc, we don't need to worry about the
        // DashMap changing the address from underneath us. It stores the Arc, not the
        // ptr we're returning here.
        let static_ref: &'static V = unsafe { &*raw_ptr };
        static_ref
    }

    /// Construct a new Cache. This should be provided to the LazyLock holding this object.
    ///
    /// ## Example
    ///
    /// ```rust
    /// use common::cache::{CacheStatic, Cache};
    /// use dashmap::DashMap;
    /// use std::{
    ///     borrow::Cow,
    ///     sync::LazyLock
    /// };
    ///
    /// static CACHE: CacheStatic<String, Cow<'static, str>> = LazyLock::new(DashMap::default);
    /// pub static MY_CACHE: LazyLock<Cache<String, Cow<'static, str>>> = LazyLock::new(|| Cache::new(&CACHE));
    /// ```
    pub fn new(container: &'static CacheStatic<K, V>) -> Self {
        Self { container }
    }

    /// Get a value from within the Cache, if it exists.
    ///
    /// ## Example
    ///
    /// ```rust
    /// use common::cache::{CacheStatic, Cache};
    /// use dashmap::DashMap;
    /// use std::{
    ///     borrow::Cow,
    ///     sync::LazyLock
    /// };
    ///
    /// static CACHE: CacheStatic<String, Cow<'static, str>> = LazyLock::new(DashMap::default);
    /// pub static MY_CACHE: LazyLock<Cache<String, Cow<'static, str>>> = LazyLock::new(|| Cache::new(&CACHE));
    ///
    /// MY_CACHE.insert("Test".to_string(), Cow::Borrowed("Another"));
    /// let value: &'static str = MY_CACHE.get("Test").unwrap();
    /// assert!(value == "Another");
    /// ```
    pub fn get<R>(&self, key: &R) -> Option<&'static V>
    where
        R: ?Sized + Hash + Eq,
        K: Borrow<R>,
    {
        if let Some(arc) = self.container.get(key) {
            Some(Self::get_static(arc))
        } else {
            None
        }
    }

    /// Insert a new value into the Cache, returning the stored value.
    ///
    /// ## Example
    ///
    /// ```rust
    /// use common::cache::{CacheStatic, Cache};
    /// use dashmap::DashMap;
    /// use std::{
    ///     borrow::Cow,
    ///     sync::LazyLock
    /// };
    ///
    /// static CACHE: CacheStatic<String, Cow<'static, str>> = LazyLock::new(DashMap::default);
    /// pub static MY_CACHE: LazyLock<Cache<String, Cow<'static, str>>> = LazyLock::new(|| Cache::new(&CACHE));
    ///
    /// let interior: &'static str = MY_CACHE.insert("Test".to_string(), Cow::Borrowed("Another"));
    /// assert!(interior == "Another");
    /// ```
    pub fn insert(&self, key: K, value: V) -> &'static V {
        self.container.insert(key.clone(), Arc::new(value));
        self.get(&key).unwrap()
    }
}
