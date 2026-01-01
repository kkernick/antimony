use dashmap::{DashMap, mapref::one::Ref};
use std::{
    borrow::Borrow,
    hash::Hash,
    sync::{Arc, LazyLock},
};

pub type CacheStatic<K, V> = LazyLock<DashMap<K, Arc<V>, ahash::RandomState>>;

pub struct Cache<K: Eq + Hash + 'static, V: 'static> {
    container: &'static CacheStatic<K, V>,
}
impl<K: Eq + Hash + Clone + 'static, V: 'static> Cache<K, V> {
    fn get_static(arc: Ref<K, Arc<V>>) -> &'static V {
        let arc = arc.value().clone();
        let raw_ptr: *const V = Arc::into_raw(arc);
        let static_ref: &'static V = unsafe { &*raw_ptr };
        static_ref
    }

    pub fn new(container: &'static CacheStatic<K, V>) -> Self {
        Self { container }
    }

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

    pub fn insert(&self, key: K, value: V) -> &'static V {
        self.container.insert(key.clone(), Arc::new(value));
        self.get(&key).unwrap()
    }
}
