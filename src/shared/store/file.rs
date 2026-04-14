//! The File Backend.
//! This is the default Backend for Antimony, where Configurations are stored as files
//! in $AT_HOME/config, and Caches are stored in $AT_HOME/cache.
//! This Backend is best for fast disks.

use crate::shared::{Map, Set, store::Object};
use rayon::prelude::*;
use std::{
    any::Any,
    fs::{self, File},
    io::Write,
    path::PathBuf,
};

/// The File Store
pub struct Store {
    path: String,
    extension: &'static str,
}
impl Store {
    /// Construct a new File Store
    pub fn new(path: &str, extension: &'static str) -> Self {
        Self {
            path: path.to_owned(),
            extension,
        }
    }

    /// Get the path to an object
    #[inline]
    fn path(&self, name: &str, object: Object) -> PathBuf {
        PathBuf::from(format!("{}/{}/{}", self.path, object.name(), name))
            .with_extension(self.extension)
    }

    /// Get the path of a category of object
    #[inline]
    fn root_path(&self, object: Object) -> PathBuf {
        PathBuf::from(format!("{}/{}", self.path, object.name()))
    }
}
impl super::BackingStore for Store {
    #[inline]
    fn as_any(&self) -> &dyn Any {
        self
    }

    #[inline]
    fn resident(&self) -> bool {
        false
    }

    #[inline]
    fn fetch(&self, name: &str, object: Object) -> Result<String, super::Error> {
        Ok(fs::read_to_string(self.path(name, object))?)
    }

    #[inline]
    fn bytes(&self, name: &str, object: Object) -> Result<Vec<u8>, super::Error> {
        Ok(fs::read(self.path(name, object))?)
    }

    #[inline]
    fn get(&self, object: Object) -> Result<Set<String>, super::Error> {
        Ok(fs::read_dir(self.root_path(object))?
            .filter_map(|file| file.ok())
            .filter_map(|file| {
                file.path()
                    .file_stem()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .collect())
    }

    #[inline]
    fn store(&self, name: &str, object: Object, content: &str) -> Result<(), super::Error> {
        self.dump(name, object, content.as_bytes())
    }

    #[inline]
    fn bulk(
        &self,
        entries: Map<String, Vec<u8>>,
        object: super::Object,
    ) -> Result<(), super::Error> {
        entries
            .into_par_iter()
            .try_for_each(|(name, content)| self.dump(&name, object, &content))
    }

    #[inline]
    fn dump(&self, name: &str, object: Object, content: &[u8]) -> Result<(), super::Error> {
        let path = self.path(name, object);
        if let Some(parent) = path.parent()
            && !parent.exists()
        {
            fs::create_dir_all(parent)?;
        }
        File::create(path)?.write_all(content)?;
        Ok(())
    }

    #[inline]
    fn exists(&self, name: &str, object: Object) -> bool {
        self.path(name, object).exists()
    }

    #[inline]
    fn remove(&self, name: &str, object: Object) -> Result<(), super::Error> {
        fs::remove_file(self.path(name, object))?;
        Ok(())
    }
}
