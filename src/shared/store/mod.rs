//! Antimony can use different backends for its file and cache store.
//! Currently, the available options are loose files, and a SQLite database.
//! By defining a common interface, they can be swapped out relatively easily,
//! and migrating from one to the other.

pub mod db;
pub mod file;
pub mod mem;

use crate::shared::{
    Map,
    config::CONFIG_FILE,
    env::{AT_CONFIG, CACHE_DIR, USER_NAME},
};
use clap::ValueEnum;
use log::info;
use nix::errno;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{any::Any, cell::RefCell, error, fmt, fs, io, path::PathBuf};
use thiserror::Error;

pub static CACHE: Mutex<Option<bool>> = Mutex::new(None);

/// Initialize the system store based on the defined configuration.
pub fn init(t: StoreType, store: Store) -> Box<dyn BackingStore> {
    info!("Using {store:?} for {t:?}");

    match t {
        StoreType::System => match store {
            Store::Database => Box::new(
                db::Store::new(db::Database::System).expect("Failed to initialize Database Store"),
            ),
            Store::File => Box::new(file::Store::new(
                &format!("{}", AT_CONFIG.display()),
                "toml",
            )),
        },
        StoreType::User => match store {
            Store::Database => Box::new(
                db::Store::new(db::Database::User).expect("Failed to initialize Database Store"),
            ),
            Store::File => Box::new(file::Store::new(
                &format!("{}/{}", AT_CONFIG.display(), USER_NAME.as_str()),
                "toml",
            )),
        },
        StoreType::Cache => {
            let cache: Box<dyn BackingStore> = match store {
                Store::Database => Box::new(
                    db::Store::new(db::Database::Cache)
                        .expect("Failed to initialize Database Store"),
                ),
                Store::File => Box::new(file::Store::new(
                    &format!("{}", CACHE_DIR.display()),
                    "cache",
                )),
            };
            if let Some(read) = *CACHE.lock() {
                info!("{t:?} in Memory");
                let name = format!("{t:?}_cache");
                Box::new(mem::Store::new(&name, cache, read))
            } else {
                cache
            }
        }
    }
}

// Each thread gets its own. Useful for databases, but does nothing
// for files.
thread_local! {
    pub static SYSTEM_STORE: RefCell<Box<dyn BackingStore>> =
        RefCell::new(init(StoreType::System, CONFIG_FILE.config_store()));

    pub static USER_STORE: RefCell<Box<dyn BackingStore>> =
        RefCell::new(init(StoreType::User, CONFIG_FILE.config_store()));

    pub static CACHE_STORE: RefCell<Box<dyn BackingStore>> =
        RefCell::new(init(StoreType::Cache, CONFIG_FILE.cache_store()));
}

#[derive(PartialEq, Eq, Copy, Clone, Hash, Debug)]
pub enum StoreType {
    System,
    User,
    Cache,
}

/// Which store is in use for a given backend.
#[derive(Serialize, Deserialize, Debug, PartialEq, Copy, Clone, ValueEnum, Default)]
pub enum Store {
    #[default]
    File,
    Database,
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
    Errno(#[from] errno::Errno),

    #[error("Pulling raw bytes from this backend cannot be returned as a string: {0}")]
    UTF(#[from] std::string::FromUtf8Error),
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
    fn name(self) -> &'static str {
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
    fn get(&self, object: Object) -> Result<Vec<String>, Error>;

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
    T: DeserializeOwned,
    E: error::Error + From<toml::de::Error> + From<Error> + From<io::Error>,
>(
    name: &str,
    object: Object,
    def: bool,
) -> Result<T, E> {
    if def && name == "default" {
        if !CONFIG_FILE.system_mode()
            && let Ok(str) = USER_STORE.with_borrow(|s| s.fetch(name, object))
        {
            return Ok(toml::from_str::<T>(&str)?);
        } else {
            let str = SYSTEM_STORE.with_borrow(|s| s.fetch(name, object))?;
            USER_STORE.with_borrow(|s| s.store(name, object, &str))?;
            return Ok(toml::from_str::<T>(&str)?);
        }
    }

    // Try and load a file absolutely if the file is given.
    if name.ends_with(".toml") {
        let path = PathBuf::from(name);
        if path.exists() {
            return Ok(toml::from_str(&fs::read_to_string(path)?)?);
        }
    }

    if !CONFIG_FILE.system_mode()
        && let Ok(str) = USER_STORE.with_borrow(|s| s.fetch(name, object))
    {
        return Ok(toml::from_str(&str)?);
    }

    let str = SYSTEM_STORE.with_borrow(|s| s.fetch(name, object))?;
    Ok(toml::from_str(&str)?)
}

/// Export the entire store into memory.
#[inline]
pub fn export(store: &dyn BackingStore) -> Map<Object, Vec<String>> {
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
        store::{BackingStore, Object, db, file},
    };

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
    fn db_backend() {
        if AT_HOME.as_path() == PWD.as_path() && PWD.join("db").exists() {
            let store =
                db::Store::new(db::Database::System).expect("Failed to initialize Database Store");
            backend_test(Box::new(store));
        }
    }

    #[test]
    fn file_backend() {
        if AT_HOME.as_path() == PWD.as_path() && PWD.join("config").exists() {
            let store = file::Store::new(&format!("{}", AT_CONFIG.display()), "toml");
            backend_test(Box::new(store));
        }
    }
}
