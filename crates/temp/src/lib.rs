use log::warn;
use rand::{RngCore, SeedableRng, rngs::SmallRng};
use std::{
    env::temp_dir,
    marker::PhantomData,
    path::{Path, PathBuf},
};

fn unique(dir: &Path) -> String {
    let mut rng = SmallRng::from_os_rng();
    loop {
        let mut bytes = [0; 8];
        rng.fill_bytes(&mut bytes);
        let instance = bytes
            .iter()
            .map(|byte| format!("{byte:02x?}"))
            .collect::<Vec<String>>()
            .join("");
        if !dir.join(&instance).exists() {
            break instance;
        }
    }
}

pub type TempFile = Temp<File>;
pub type TempDir = Temp<Dir>;

#[derive(Default)]
pub struct File;

#[derive(Default)]
pub struct Dir;

pub trait Object {
    fn create(path: &Path, name: &str) -> Result<(), std::io::Error>;
    fn remove(path: &Path, name: &str) -> Result<(), std::io::Error>;
}

impl Object for File {
    fn create(path: &Path, name: &str) -> Result<(), std::io::Error> {
        if !path.exists() {
            std::fs::create_dir_all(path)?;
        }
        std::fs::File::create_new(path.join(name)).map(|_| ())
    }

    fn remove(path: &Path, name: &str) -> Result<(), std::io::Error> {
        std::fs::remove_file(path.join(name)).map(|_| ())
    }
}

impl Object for Dir {
    fn create(path: &Path, name: &str) -> Result<(), std::io::Error> {
        std::fs::create_dir_all(path.join(name)).map(|_| ())
    }

    fn remove(path: &Path, name: &str) -> Result<(), std::io::Error> {
        std::fs::remove_dir_all(path.join(name)).map(|_| ())
    }
}

pub struct InstantiatedTemp<T: Object + Default> {
    name: String,
    path: PathBuf,

    #[cfg(feature = "user")]
    mode: user::Mode,

    phantom: PhantomData<T>,
}
impl<T: Object + Default> InstantiatedTemp<T> {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
impl<T: Object + Default> Drop for InstantiatedTemp<T> {
    fn drop(&mut self) {
        #[cfg(feature = "user")]
        let result = {
            let mode = self.mode;
            user::sync::run_as!(mode, { T::remove(&self.path, &self.name) })
        };

        #[cfg(not(feature = "user"))]
        let result = T::remove(&self.path, &self.name);

        if result.is_err() {
            warn!("Failed to remove temporary file: {}", self.name);
        }
    }
}

#[derive(Default)]
pub struct Temp<T: Object + Default> {
    name: Option<String>,
    path: Option<PathBuf>,

    #[cfg(feature = "user")]
    mode: Option<user::Mode>,

    /// Use a phantom because the template just dictates the create/remove functions.
    phantom: PhantomData<T>,
}
impl<T: Object + Default> Temp<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create_now() -> Result<InstantiatedTemp<T>, std::io::Error> {
        Self::default().create()
    }

    #[cfg(feature = "user")]
    pub fn mode(mut self, mode: user::Mode) -> Self {
        self.mode = Some(mode);
        self
    }

    pub fn within(mut self, path: impl Into<PathBuf>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn create(self) -> Result<InstantiatedTemp<T>, std::io::Error> {
        let path = self.path.unwrap_or(temp_dir());
        let name = self.name.unwrap_or(unique(&path));

        #[cfg(feature = "user")]
        {
            let mode = self.mode.unwrap_or(user::current()?);
            user::run_as!(mode, T::create(&path, &name))?;
            Ok(InstantiatedTemp {
                name,
                path,
                mode,
                phantom: self.phantom,
            })
        }

        #[cfg(not(feature = "user"))]
        {
            T::create(&path, &name)?;
            Ok(InstantiatedTemp {
                name,
                path,
                phantom: self.phantom,
            })
        }
    }
}
