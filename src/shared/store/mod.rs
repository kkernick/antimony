#![allow(
    clippy::absolute_paths,
    clippy::missing_errors_doc,
    clippy::missing_docs_in_private_items
)]
//! Antimony can use different backends for its file and cache store.
//! By defining a common interface, they can be swapped out relatively easily,
//! and migrating from one to the other.

pub mod file;
pub mod mem;

use crate::shared::{
    Map, Set,
    config::CONFIG_FILE,
    env::{AT_CONFIG, CACHE_DIR, USER_NAME},
};
use log::info;
use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use std::{any::Any, error, fmt, fs, io, path::PathBuf, string, sync::LazyLock};
use thiserror::Error;

pub static CACHE: Mutex<Option<bool>> = Mutex::new(None);

pub struct Store {
    /// The underlying store we're pointing to.
    backing: Box<dyn BackingStore + Send + Sync>,
}
impl Store {
    pub fn init(t: StoreType) -> Self {
        let backing: Box<dyn BackingStore + Send + Sync> = match t {
            StoreType::System => Box::new(file::Store::new(
                &format!("{}", AT_CONFIG.display()),
                "toml",
            )),

            StoreType::User => Box::new(file::Store::new(
                &format!("{}/{}", AT_CONFIG.display(), USER_NAME.as_str()),
                "toml",
            )),
            StoreType::Cache => {
                let cache: Box<dyn BackingStore + Send + Sync> = Box::new(file::Store::new(
                    &format!("{}", CACHE_DIR.display()),
                    "cache",
                ));

                let value = *CACHE.lock();
                if let Some(read) = value {
                    info!("{t:?} in Memory");
                    let name = format!("{t:?}_cache");
                    Box::new(mem::Store::new(&name, cache, read))
                } else {
                    cache
                }
            }
        };
        Self { backing }
    }

    #[allow(clippy::should_implement_trait)]
    #[must_use]
    pub fn borrow(&self) -> &dyn BackingStore {
        self.backing.as_ref()
    }
}

pub static SYSTEM_STORE: LazyLock<Store> = LazyLock::new(|| Store::init(StoreType::System));
pub static USER_STORE: LazyLock<Store> = LazyLock::new(|| Store::init(StoreType::User));
pub static CACHE_STORE: LazyLock<Store> = LazyLock::new(|| Store::init(StoreType::Cache));

#[derive(PartialEq, Eq, Copy, Clone, Hash, Debug)]
pub enum StoreType {
    System,
    User,
    Cache,
}

/// Store errors
#[derive(Debug, Error)]
pub enum Error {
    #[error("Failed to initialize {0} store")]
    Init(&'static str),

    #[error("Memory Backend Error: {0}")]
    Mem(&'static str),

    #[error("I/O Error: {0}")]
    Io(#[from] io::Error),

    #[error("Database Error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Failed to change user: {0}")]
    User(#[from] user::Error),

    #[error("Pulling raw bytes from this backend cannot be returned as a string: {0}")]
    UTF(#[from] string::FromUtf8Error),
}

/// Each Object, for iteration.
pub static OBJECTS: [Object; 6] = [
    Object::Profile,
    Object::Feature,
    Object::Directories,
    Object::Wildcards,
    Object::Libraries,
    Object::Binaries,
];

/// The kinds of things a backend can store.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum Object {
    Profile,
    Feature,
    Directories,
    Wildcards,
    Libraries,
    Binaries,
}
impl Object {
    const fn name(self) -> &'static str {
        match self {
            Self::Profile => "profiles",
            Self::Feature => "features",
            Self::Directories => "directories",
            Self::Wildcards => "wildcards",
            Self::Libraries => "libraries",
            Self::Binaries => "binaries",
        }
    }
}
impl fmt::Display for Object {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// The abstraction layer that each backend must implement. This simply defines
/// an interface that Antimony can use to read/write to a backend.
pub trait BackingStore {
    /// Resolve the Store as an Any for down-casting
    fn as_any(&self) -> &dyn Any;

    /// Whether the store is resident to the instance, or is non-memory backed.
    fn resident(&self) -> bool;

    /// Fetch an object as a string.
    fn fetch(&self, name: &str, object: Object) -> Result<String, Error>;

    /// Fetch an object as raw bytes.
    fn bytes(&self, name: &str, object: Object) -> Result<Vec<u8>, Error>;

    /// Get all objects of a certain type
    fn get(&self, object: Object) -> Result<Set<String>, Error>;

    /// Check if an object exists.
    fn exists(&self, name: &str, object: Object) -> bool;

    /// Store a string into the data-store with the given name.
    fn store(&self, name: &str, object: Object, content: &str) -> Result<(), Error>;

    /// Store bytes into the data-store with the given name.
    fn dump(&self, name: &str, object: Object, content: &[u8]) -> Result<(), Error>;

    fn bulk(&self, entries: Map<String, Vec<u8>>, object: Object) -> Result<(), Error>;

    /// Remove an object from the data-store
    fn remove(&self, name: &str, object: Object) -> Result<(), Error>;
}

/// Load an object from the database.
pub fn load<
    T: DeserializeOwned + Default,
    E: error::Error + From<toml::de::Error> + From<Error> + From<io::Error>,
>(
    name: &str,
    object: Object,
    def: bool,
) -> Result<T, E> {
    if def && name == "default" && CONFIG_FILE.system_mode() {
        log::trace!("Default not allowed");
        return Ok(T::default());
    }

    // Try and load a file absolutely if the file is given.
    if std::path::Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
    {
        let path = PathBuf::from(name);
        if path.exists() {
            return Ok(toml::from_str(&fs::read_to_string(path)?)?);
        }
    }

    if !CONFIG_FILE.system_mode()
        && let Ok(str) = USER_STORE.borrow().fetch(name, object)
    {
        return Ok(toml::from_str(&str)?);
    }

    let str = SYSTEM_STORE.borrow().fetch(name, object)?;
    Ok(toml::from_str(&str)?)
}

/// Export the entire store into memory.
#[inline]
pub fn export(store: &dyn BackingStore) -> Map<Object, Set<String>> {
    let mut map = Map::default();
    for object in OBJECTS {
        if let Ok(objects) = store.get(object) {
            map.insert(object, objects);
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use crate::shared::{
        env::{AT_CONFIG, AT_HOME, PWD},
        store::{BackingStore, Object, file},
    };

    #[allow(clippy::needless_pass_by_value)]
    fn backend_test(store: Box<dyn BackingStore>) {
        let object = Object::Profile;
        for profile in store.get(object).expect("Failed to get profiles") {
            store
                .fetch(&profile, object)
                .expect("Failed to get profile");
        }

        let content = "This is a test!";
        store
            .store("test", object, content)
            .expect("Failed to store profile");

        assert!(store.exists("test", object));
        assert!(store.fetch("test", object).expect("Failed to get profile") == content);

        store
            .remove("test", object)
            .expect("Failed to delete profile");
        assert!(!store.exists("test", object));
    }

    #[test]
    fn file_backend() {
        if AT_HOME.as_path() == PWD.as_path() && PWD.join("config").exists() {
            let store = file::Store::new(&format!("{}", AT_CONFIG.display()), "toml");
            backend_test(Box::new(store));
        }
    }
}
