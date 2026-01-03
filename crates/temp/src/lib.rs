#![doc = include_str!("../README.md")]

use std::{
    env::temp_dir,
    iter::repeat_with,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};

/// Generate a unique object in the provided directory.
fn unique(dir: &Path) -> String {
    let mut rng = fastrand::Rng::new();
    loop {
        let mut instance = String::with_capacity(16);
        repeat_with(|| rng.u8(..))
            .take(8)
            .map(|byte| format!("{byte:02x?}"))
            .for_each(|byte| instance.push_str(&byte));

        if !dir.join(&instance).exists() {
            break instance;
        }
    }
}

/// An object is something that exists in the filesystem.
pub trait Object {
    /// Create the object
    fn create(&self) -> Result<(), std::io::Error>;

    /// Remove the object
    fn remove(&self) -> Result<(), std::io::Error>;

    /// Get the parent of the object
    fn path(&self) -> &Path;

    /// Get the name of the object.
    fn name(&self) -> &str;

    /// Get the full path of the object, IE path + name
    fn full(&self) -> PathBuf;
}

/// A trait for Objects that can be created in the `temp::Builder`
pub trait BuilderCreate {
    fn new(path: PathBuf, name: String) -> Self;
}

/// A temporary file.
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
        let path = self.parent.join(&self.name);
        if path.exists() {
            std::fs::remove_file(path).map(|_| ())?;
        }
        Ok(())
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

/// A temporary directory
pub struct Directory {
    path: PathBuf,
    name: String,
}
impl Object for Directory {
    fn create(&self) -> Result<(), std::io::Error> {
        std::fs::create_dir_all(self.path.join(&self.name)).map(|_| ())
    }

    fn remove(&self) -> Result<(), std::io::Error> {
        let path = self.path.join(&self.name);
        if path.exists() {
            std::fs::remove_dir_all(path).map(|_| ())?;
        }
        Ok(())
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

/// An instance of a Temporary Object. The Object will be deleted
/// when the Object is dropped.
///
/// Additional Temporary Objects can be associated to an instance,
/// such that they will be tied to the main Object's lifetime and
/// dropped together.
///
/// There is no distinction between Files/Directories/other objects
/// in a Temp. The distinction is only made within the Builder, and
/// it is up to the program to know what kind of object is stored.
pub struct Temp {
    object: Box<dyn Object>,
    associated: Vec<Temp>,

    #[cfg(feature = "user")]
    mode: user::Mode,
}
impl Temp {
    /// Associate another Temporary Object to the caller. It will
    /// be tied to the caller's lifetime, and dropped with it.
    pub fn associate(&mut self, temp: Temp) {
        self.associated.push(temp)
    }

    /// Get the name of the temporary object.
    pub fn name(&self) -> &str {
        self.object.name()
    }

    /// Get the path the Temporary Object is located within.
    pub fn path(&self) -> &Path {
        self.object.path()
    }

    /// The full path to the object, including its name.
    pub fn full(&self) -> PathBuf {
        self.object.full()
    }

    /// Make a Symlink from the Temporary Object. This link will be associated
    /// with the caller, such that the Link will be deleted with the Object.
    pub fn link(
        &mut self,
        link: impl Into<PathBuf>,

        #[cfg(feature = "user")] mode: user::Mode,
    ) -> Result<(), std::io::Error> {
        let link = link.into();
        if let Some(parent) = link.parent()
            && let Some(name) = link.file_name()
        {
            #[cfg(feature = "user")]
            user::run_as!(mode, { symlink(self.object.full(), &link) })??;

            #[cfg(not(feature = "user"))]
            symlink(self.object.full(), &link)?;

            self.associated.push(Temp {
                object: Box::new(File {
                    parent: parent.to_path_buf(),
                    name: name.to_string_lossy().into_owned(),
                }),
                associated: Vec::new(),

                #[cfg(feature = "user")]
                mode,
            });
            Ok(())
        } else {
            Err(std::io::ErrorKind::NotFound.into())
        }
    }
}
impl Drop for Temp {
    #[cfg(feature = "user")]
    fn drop(&mut self) {
        let mode = self.mode;
        let _ = user::run_as!(mode, { self.object.remove() });
    }

    #[cfg(not(feature = "user"))]
    fn drop(&mut self) {
        let _ = self.object.remove();
    }
}
unsafe impl Send for Temp {}
unsafe impl Sync for Temp {}

/// Build a new Temporary Object.
///
/// ## Example
///
/// ```rust
/// use std::io::Write;
/// let temp = temp::Builder::new().create::<temp::File>().unwrap();
/// let path = temp.full();
/// let mut file = std::fs::File::open(&path).unwrap();
/// write!(file, "Hello!");
/// drop(temp);
/// assert!(!path.exists());
/// ```
#[derive(Default)]
pub struct Builder {
    name: Option<String>,
    path: Option<PathBuf>,
    extension: Option<String>,

    #[cfg(feature = "user")]
    mode: Option<user::Mode>,
    make: bool,
}
impl Builder {
    /// Create a new Builder.
    pub fn new() -> Self {
        Self {
            make: true,
            ..Default::default()
        }
    }

    #[cfg(feature = "user")]
    /// Set the owner of the Temporary Object.
    pub fn owner(mut self, mode: user::Mode) -> Self {
        self.mode = Some(mode);
        self
    }

    /// The directory the Temporary Object should reside in.
    /// If not set, defaults to /tmp.
    pub fn within(mut self, path: impl Into<PathBuf>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// The name of the Temporary Object. If not set, uses a
    /// randomized, unique string.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set an optional extension to the Object.
    pub fn extension(mut self, extension: impl Into<String>) -> Self {
        self.extension = Some(extension.into());
        self
    }

    /// Whether to create the Object on create(). By default,
    /// the object is created.
    pub fn make(mut self, make_object: bool) -> Self {
        self.make = make_object;
        self
    }

    /// Create the Object, consuming the Builder. The Object is passed as a Template to this
    /// function.
    ///
    /// ## Examples
    ///
    /// Create a new temporary file as /tmp/new_file
    ///
    /// ```rust
    /// let file = temp::Builder::new().name("new_file").create::<temp::File>().unwrap();
    /// assert!(std::path::Path::new("/tmp/new_file").exists());
    /// ```
    ///
    /// Create a new temporary directory in the current directory
    ///
    /// ```rust
    /// temp::Builder::new().within("").create::<temp::Directory>().unwrap();
    /// ```
    pub fn create<T: BuilderCreate + Object + 'static>(self) -> Result<Temp, std::io::Error> {
        let parent = self.path.unwrap_or(temp_dir());
        let mut name = self.name.unwrap_or(unique(&parent));

        #[cfg(feature = "user")]
        let mode = self.mode.unwrap_or(user::current()?);

        if let Some(extension) = &self.extension {
            name.push_str(&format!(".{extension}"));
        }

        let object = T::new(parent, name);
        if self.make {
            #[cfg(feature = "user")]
            user::run_as!(mode, object.create())??;

            #[cfg(not(feature = "user"))]
            object.create()?;
        }
        Ok(Temp {
            object: Box::new(object),
            associated: Vec::new(),

            #[cfg(feature = "user")]
            mode,
        })
    }
}
