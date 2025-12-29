use log::warn;
use rand::{RngCore, SeedableRng, rngs::SmallRng};
use std::{
    env::temp_dir,
    os::unix::fs::symlink,
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

pub trait Object {
    fn create(&self) -> Result<(), std::io::Error>;
    fn remove(&self) -> Result<(), std::io::Error>;
    fn path(&self) -> &Path;
    fn name(&self) -> &str;
    fn full(&self) -> PathBuf;
}

pub trait BuilderCreate {
    fn new(path: PathBuf, name: String) -> Self;
}

pub struct File {
    parent: PathBuf,
    name: String,
}
impl Object for File {
    fn create(&self) -> Result<(), std::io::Error> {
        if !self.parent.exists() {
            std::fs::create_dir_all(&self.parent)?;
        }
        std::fs::File::create_new(self.parent.join(&self.name)).map(|_| ())
    }
    fn remove(&self) -> Result<(), std::io::Error> {
        std::fs::remove_file(self.parent.join(&self.name)).map(|_| ())
    }

    fn path(&self) -> &Path {
        &self.parent
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn full(&self) -> PathBuf {
        self.parent.join(&self.name)
    }
}
impl BuilderCreate for File {
    fn new(path: PathBuf, name: String) -> Self {
        Self { parent: path, name }
    }
}

pub struct Directory {
    path: PathBuf,
    name: String,
}
impl Object for Directory {
    fn create(&self) -> Result<(), std::io::Error> {
        std::fs::create_dir_all(self.path.join(&self.name)).map(|_| ())
    }

    fn remove(&self) -> Result<(), std::io::Error> {
        std::fs::remove_dir_all(self.path.join(&self.name)).map(|_| ())
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn full(&self) -> PathBuf {
        self.path.join(&self.name)
    }
}
impl BuilderCreate for Directory {
    fn new(path: PathBuf, name: String) -> Self {
        Self { path, name }
    }
}

pub struct Temp {
    object: Box<dyn Object>,
    associated: Vec<Temp>,
    mode: user::Mode,
}
impl Temp {
    pub fn associate(&mut self, temp: Temp) {
        self.associated.push(temp)
    }

    pub fn name(&self) -> &str {
        self.object.name()
    }

    pub fn path(&self) -> &Path {
        self.object.path()
    }

    pub fn full(&self) -> PathBuf {
        self.object.full()
    }

    pub fn link(
        &mut self,
        link: impl Into<PathBuf>,
        mode: user::Mode,
    ) -> Result<(), std::io::Error> {
        let link = link.into();
        if let Some(parent) = link.parent()
            && let Some(name) = link.file_name()
        {
            user::try_run_as!(mode, { symlink(self.object.full(), &link) })?;
            self.associated.push(Temp {
                object: Box::new(File {
                    parent: parent.to_path_buf(),
                    name: name.to_string_lossy().into_owned(),
                }),
                associated: Vec::new(),
                mode,
            });
            Ok(())
        } else {
            Err(std::io::ErrorKind::NotFound.into())
        }
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let mode = self.mode;
        let result = user::sync::run_as!(mode, { self.object.remove() });
        if let Err(e) = result
            && e.kind() != std::io::ErrorKind::NotFound
        {
            warn!("Failed to remove temporary file: {e}");
        }
    }
}
unsafe impl Send for Temp {}
unsafe impl Sync for Temp {}

#[derive(Default)]
pub struct Builder {
    name: Option<String>,
    path: Option<PathBuf>,
    mode: Option<user::Mode>,
}
impl Builder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn owner(mut self, mode: user::Mode) -> Self {
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

    pub fn create<T: BuilderCreate + Object + 'static>(self) -> Result<Temp, std::io::Error> {
        let parent = self.path.unwrap_or(temp_dir());
        let name = self.name.unwrap_or(unique(&parent));
        let mode = self.mode.unwrap_or(user::current()?);

        let object = T::new(parent, name);
        user::run_as!(mode, object.create())?;
        Ok(Temp {
            object: Box::new(object),
            associated: Vec::new(),
            mode,
        })
    }
}
