#![doc = include_str!("../README.md")]

use std::{
    env::temp_dir,
    fmt::Write,
    fs, io,
    iter::repeat_with,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("I/O Error: {0}")]
    Io(#[from] io::Error),

    #[cfg(feature = "user")]
    #[error("User Error: {0}")]
    User(#[from] user::Error),
}

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
    ///
    /// ### Errors
    /// If the file cannot be created.
    fn create(&self) -> Result<(), Error>;

    /// Remove the object
    ///
    /// ### Errors
    /// If the file cannot be removed.
    fn remove(&self) -> Result<(), Error>;

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
    /// The path the file is located in.
    parent: PathBuf,

    /// The name of the file
    name: String,
}
impl Object for File {
    fn create(&self) -> Result<(), Error> {
        if !self.parent.exists() {
            fs::create_dir_all(&self.parent)?;
        }
        fs::File::create_new(self.parent.join(&self.name)).map(|_| ())?;
        Ok(())
    }
    fn remove(&self) -> Result<(), Error> {
        fs::remove_file(self.full())?;
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
    /// The path the directory in is.
    path: PathBuf,

    /// The name of the directory
    name: String,
}
impl Object for Directory {
    fn create(&self) -> Result<(), Error> {
        fs::create_dir_all(self.path.join(&self.name))?;
        Ok(())
    }

    fn remove(&self) -> Result<(), Error> {
        fs::remove_dir_all(self.full())?;
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
    /// The temporary object itself.
    object: Box<dyn Object + Send + Sync>,

    /// Any associated objects, so their lifetimes are bound together.
    associated: Vec<Self>,

    #[cfg(feature = "user")]
    /// The mode the file was created with, so it can be properly destroyed
    mode: user::Mode,
}
impl Temp {
    /// Associate another Temporary Object to the caller. It will
    /// be tied to the caller's lifetime, and dropped with it.
    pub fn associate(&mut self, temp: Self) {
        self.associated.push(temp);
    }

    /// Get the name of the temporary object.
    #[must_use]
    pub fn name(&self) -> &str {
        self.object.name()
    }

    /// Get the path the Temporary Object is located within.
    #[must_use]
    pub fn path(&self) -> &Path {
        self.object.path()
    }

    /// The full path to the object, including its name.
    #[must_use]
    pub fn full(&self) -> PathBuf {
        self.object.full()
    }

    /// Make a Symlink from the Temporary Object. This link will be associated
    /// with the caller, such that the Link will be deleted with the Object.
    ///
    /// ## Errors
    /// `Error::Io`: If the object could not be created.
    /// `Error::User`: If we could not change user.
    pub fn link(
        &mut self,
        link: &Path,

        #[cfg(feature = "user")] mode: user::Mode,
    ) -> Result<(), Error> {
        if let Some(parent) = link.parent()
            && let Some(name) = link.file_name()
        {
            #[cfg(feature = "user")]
            user::run_as!(mode, { symlink(self.object.full(), link) })??;

            #[cfg(not(feature = "user"))]
            symlink(self.object.full(), &link)?;

            self.associated.push(Self {
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
            Err(Error::Io(io::ErrorKind::NotFound.into()))
        }
    }
}
impl Drop for Temp {
    #[cfg(feature = "user")]
    fn drop(&mut self) {
        let mode = self.mode;
        if let Err(e) = user::run_as!(mode, { self.object.remove() }) {
            log::error!("Failed to remove temporary file: {e}");
        }
    }

    #[cfg(not(feature = "user"))]
    fn drop(&mut self) {
        log::trace!("Dropping temporary file: {}", self.full().display());
        if let Err(e) = self.object.remove() {
            log::error!("Failed to remove temporary file: {e}");
        }
    }
}

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
    /// The name of the object.
    name: Option<String>,

    /// The path to the object
    path: Option<PathBuf>,

    /// The file extension
    extension: Option<String>,

    #[cfg(feature = "user")]
    /// The mode to create the file with.
    mode: Option<user::Mode>,

    /// Whether to create the file on `create()`
    make: bool,
}
impl Builder {
    /// Create a new Builder.
    #[must_use]
    pub fn new() -> Self {
        Self {
            make: true,
            ..Default::default()
        }
    }

    #[cfg(feature = "user")]
    #[must_use]
    /// Set the owner of the Temporary Object.
    pub const fn owner(mut self, mode: user::Mode) -> Self {
        self.mode = Some(mode);
        self
    }

    /// The directory the Temporary Object should reside in.
    /// If not set, defaults to /tmp.
    #[must_use]
    pub fn within(mut self, path: impl Into<PathBuf>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// The name of the Temporary Object. If not set, uses a
    /// randomized, unique string.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set an optional extension to the Object.
    #[must_use]
    pub fn extension(mut self, extension: impl Into<String>) -> Self {
        self.extension = Some(extension.into());
        self
    }

    /// Whether to create the Object on `create()`. By default,
    /// the object is created.
    #[must_use]
    pub const fn make(mut self, make_object: bool) -> Self {
        self.make = make_object;
        self
    }

    /// Create the Object, consuming the Builder. The Object is passed as a Template to this
    /// function.
    ///
    /// ## Examples
    ///
    /// Create a new temporary file as `/tmp/new_file`
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
    ///
    /// ## Errors
    /// `Error::Io`: If the object could not be created.
    /// `Error::User`: If we could not change user.
    pub fn create<T: BuilderCreate + Object + Send + Sync + 'static>(self) -> Result<Temp, Error> {
        let parent = self.path.unwrap_or_else(temp_dir);
        let mut name = self.name.unwrap_or_else(|| unique(&parent));

        #[cfg(feature = "user")]
        let mode = self.mode.unwrap_or(user::current()?);

        if let Some(extension) = &self.extension {
            let _ = write!(name, ".{extension}");
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
