use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
};
use user::as_effective;

use crate::shared::store::Object;

pub struct Store {
    path: String,
    extension: &'static str,
}
impl Store {
    pub fn new(path: &str, extension: &'static str) -> Self {
        Self {
            path: path.to_owned(),
            extension,
        }
    }

    fn path(&self, name: &str, object: Object) -> PathBuf {
        PathBuf::from(format!("{}/{}/{}", self.path, object.name(), name))
            .with_extension(self.extension)
    }

    fn root_path(&self, object: Object) -> PathBuf {
        PathBuf::from(format!("{}/{}", self.path, object.name()))
    }
}
impl super::BackingStore for Store {
    fn fetch(&self, name: &str, object: Object) -> Result<String, super::Error> {
        Ok(fs::read_to_string(self.path(name, object))?)
    }

    fn bytes(&self, name: &str, object: Object) -> Result<Vec<u8>, super::Error> {
        Ok(fs::read(self.path(name, object))?)
    }

    fn get(&self, object: Object) -> Result<Vec<String>, super::Error> {
        Ok(fs::read_dir(self.root_path(object))?
            .filter_map(|file| file.ok())
            .filter_map(|file| {
                file.path()
                    .file_stem()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .collect())
    }

    fn store(&self, name: &str, object: Object, content: &str) -> Result<(), super::Error> {
        self.dump(name, object, content.as_bytes())
    }

    fn dump(&self, name: &str, object: Object, content: &[u8]) -> Result<(), super::Error> {
        let path = self.path(name, object);
        as_effective!(Result<(), super::Error>, {
            if let Some(parent) = path.parent()
                && !parent.exists()
            {
                fs::create_dir_all(parent)?;
            }
            File::create(path)?.write_all(content)?;
            Ok(())
        })?
    }

    fn exists(&self, name: &str, object: Object) -> bool {
        self.path(name, object).exists()
    }

    fn remove(&self, name: &str, object: Object) -> Result<(), super::Error> {
        as_effective!({ fs::remove_file(self.path(name, object)) })??;
        Ok(())
    }
}
