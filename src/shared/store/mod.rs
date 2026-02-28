pub mod file;

use crate::shared::{
    config::CONFIG_FILE,
    env::{AT_CONFIG, USER_NAME},
};
use nix::errno;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{cell::RefCell, error, fs, io, path::PathBuf};
use thiserror::Error;

thread_local! {
    pub static SYSTEM_STORE: RefCell<file::FileStore> = RefCell::new(
        file::FileStore::new(&format!("{}", AT_CONFIG.display()), "toml")
    );
    pub static USER_STORE: RefCell<file::FileStore> = RefCell::new(
        file::FileStore::new(&format!("{}/{}", AT_CONFIG.display(), USER_NAME.as_str()), "toml")
    );
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Failed to initialize {0} store")]
    Init(&'static str),

    #[error("I/O Error: {0}")]
    Io(#[from] io::Error),

    #[error("Failed to change user: {0}")]
    Errno(#[from] errno::Errno),
}

#[derive(Deserialize, Serialize, Default, Debug, Copy, Clone)]
pub enum Store {
    #[default]
    File,

    SQLite,
}

#[derive(Copy, Clone)]
pub enum Object {
    Profile,
    Feature,
}
impl Object {
    fn name(self) -> &'static str {
        match self {
            Self::Profile => "profiles",
            Self::Feature => "features",
        }
    }
}

pub trait BackingStore {
    fn fetch(&self, name: &str, object: Object) -> Result<String, Error>;

    fn get(&self, object: Object) -> Result<Vec<String>, Error>;

    fn exists(&self, name: &str, object: Object) -> bool;

    fn store(&self, name: &str, object: Object, content: &str) -> Result<(), Error>;

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
