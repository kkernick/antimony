//! The Antimony Package is a self-contained profile that can run on any device.

use crate::{cli::run, setup::setup};
use crate::{
    fab::lib::ROOTS,
    shared::{Map, env::CACHE_DIR, profile::Profile},
    timer,
};
use anyhow::Result;
use anyhow::anyhow;
use bilrost::{Message, OwnedMessage};
use bstr::{BStr, BString};
use log::{debug, error, info};
use path_clean::clean;
use serde::{Deserialize, Serialize};
use spawn::{Spawner, StreamMode};
use std::ffi::OsStr;
use std::{
    borrow::Cow,
    fs::{self, File},
    io::{Read, Seek},
    os::unix::fs::PermissionsExt,
    path::Path,
};
use user::Mode;

// Package marker
pub static PACKAGE_MARKER: [u8; 7] = *b"\0PKG\0\0\0";

#[derive(Deserialize, Serialize, Default, Debug, Message)]
pub struct Package {
    pub name: String,
    pub profile: Profile,

    /// Files can be anywhere is the sandbox (Resource Files)
    /// They are mounted onto root via on overlay.
    pub files: Map<String, BString>,

    /// Libraries are localized to a single root and attached to /usr/lib. Lib roots
    /// Are then symlinked.
    pub libraries: Map<String, BString>,

    /// Binaries are dumped to /usr/bin, and symlinked to bin, sbin, and /usr/sbin
    pub binaries: Map<String, BString>,

    /// System binaries are binaries used by antimony (i.e find, proxy). They are
    /// added to `PATH`
    pub system_binaries: Map<String, BString>,

    /// Libraries needed by the system binaries. Added to `LD_LIBRARY_PATH`.
    pub system_libraries: Map<String, BString>,
}
impl Package {
    /// Adds a binary to the package.
    ///
    /// ## Errors
    /// If the binary could not be found, or it could not be read.
    pub fn add(&mut self, name: &str, dest: &str) -> Result<()> {
        Self::add_path(name, dest, &mut self.files)
    }

    pub fn add_binary(&mut self, name: &str, dest: &str) -> Result<()> {
        let dest = if let Some(i) = dest.rfind('/')
            && let Some(i) = i.checked_add(1)
        {
            &dest[i..]
        } else {
            dest
        };

        Self::add_path(name, dest, &mut self.binaries)
    }

    pub fn add_system(&mut self, name: &str, dest: &str) -> Result<()> {
        let dest = if let Some(i) = dest.rfind('/')
            && let Some(i) = i.checked_add(1)
        {
            &dest[i..]
        } else {
            dest
        };

        Self::add_path(name, dest, &mut self.system_binaries)
    }

    pub fn add_depend(&mut self, name: &str, mut dest: &str) -> Result<()> {
        for root in ROOTS.iter() {
            dest = dest.strip_prefix(root.as_ref()).unwrap_or(dest);
        }
        dest = dest.strip_prefix('/').unwrap_or(dest);
        Self::add_path(name, dest, &mut self.system_libraries)
    }

    pub fn add_library(&mut self, name: &str, mut dest: &str) -> Result<()> {
        for root in ROOTS.iter() {
            dest = dest.strip_prefix(root.as_ref()).unwrap_or(dest);
        }
        dest = dest.strip_prefix('/').unwrap_or(dest);
        Self::add_path(name, dest, &mut self.libraries)
    }

    /// Add a binary to the package and the specified path.
    ///
    /// ## Errors
    /// If the binary could not be found, or it could not be read
    pub fn add_path(name: &str, dest: &str, to: &mut Map<String, BString>) -> Result<()> {
        let mut path = Path::new(name);
        if !path.exists() {
            path = Path::new(which::which(name)?);
        }

        let mut add_file = |path: &Path, dest: &str| -> Result<()> {
            debug!("Adding {} to package (at {dest})", path.display());
            let mut buffer = Vec::new();
            File::open(path)?.read_to_end(&mut buffer)?;

            to.insert(dest.to_owned(), BString::from(buffer));
            Ok(())
        };

        if path.is_file() {
            add_file(path, dest)?;
        } else {
            let files = Spawner::new("find")?
                .args([&path.to_string_lossy(), "-type", "f"])
                .mode(Mode::Real)
                .output(StreamMode::Pipe)
                .spawn()?
                .output_all()?;

            for file in files.lines() {
                let dest = if let Some(i) = file.find(dest)
                    && let Some(i) = i.checked_add(dest.len())
                {
                    Cow::Owned(format!("{dest}{}", &file[i..]))
                } else {
                    Cow::Borrowed(file)
                };
                add_file(Path::new(file), &dest)?;
            }
        }

        Ok(())
    }

    pub fn unpack(&self, package_dir: &Path) -> Result<()> {
        fs::create_dir_all(package_dir)?;
        fs::write(
            package_dir.join(format!("{}.toml", self.name)),
            toml::to_string(&self.profile)?,
        )?;

        let unpack = |root: &Path, path: &str, content: &BStr, executable: bool| -> Result<()> {
            let pkg_path = clean(root.join(path.strip_prefix('/').unwrap_or(path)));

            if !pkg_path.starts_with(package_dir) {
                error!("File outside package directory: {}", pkg_path.display());
            }
            if let Some(parent) = pkg_path.parent()
                && !parent.exists()
            {
                fs::create_dir_all(parent)?;
            }
            debug!("Unpacking {path} to {}...", pkg_path.display());
            fs::write(&pkg_path, content)?;
            if executable {
                let metadata = fs::metadata(&pkg_path)?;
                let mut perms = metadata.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(pkg_path, perms)?;
            }
            Ok(())
        };

        if !self.files.is_empty() {
            let root = package_dir.join("root");
            fs::create_dir(&root)?;
            for (path, content) in &self.files {
                unpack(&root, path, content.as_ref(), false)?;
            }
        }
        if !self.binaries.is_empty() {
            let root = package_dir.join("bin");
            fs::create_dir(&root)?;
            for (path, content) in &self.binaries {
                unpack(&root, path, content.as_ref(), true)?;
            }
        }
        if !self.libraries.is_empty() {
            let root = package_dir.join("lib");
            fs::create_dir(&root)?;
            for (path, content) in &self.libraries {
                unpack(&root, path, content.as_ref(), true)?;
            }
        }
        if !self.system_binaries.is_empty() {
            let root = package_dir.join("system").join("bin");
            fs::create_dir_all(&root)?;
            for (path, content) in &self.system_binaries {
                unpack(&root, path, content.as_ref(), true)?;
            }
        }
        if !self.system_libraries.is_empty() {
            let root = package_dir.join("system").join("lib");
            fs::create_dir_all(&root)?;
            for (path, content) in &self.system_libraries {
                unpack(&root, path, content.as_ref(), true)?;
            }
        }

        Ok(())
    }
}

pub fn execute_package(current: &Path, mut file: File, name: &OsStr) -> Result<()> {
    let root_path = Path::new("/pkg");
    if root_path.exists() {
        info!("In package namespace. Executing packaging...");
        let result = || -> Result<()> {
            let info = timer!(
                "::setup",
                setup(
                    root_path.to_string_lossy(),
                    &mut run::Args::default(),
                    false,
                    Some((Package::default(), true))
                )
            )?;
            timer!("::run", run::run(info, &mut run::Args::default()))?;
            Ok(())
        }();

        if let Err(e) = &result {
            error!("{e}");
        }
        result
    } else {
        debug!("Package marker found");
        let path = CACHE_DIR.join("packages").join(name);
        file.rewind()?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;

        // We might hit multiple marker (At least one, as it's used in the ELF header), but
        // the data shouldn't have it, so it should be the last one.
        debug!("Searching for payload...");
        let mut package = None;
        for (position, window) in bytes.windows(PACKAGE_MARKER.len()).enumerate() {
            if window == PACKAGE_MARKER
                && let Some(index) = position.checked_add(PACKAGE_MARKER.len())
                && let Ok(pkg) = zstd::decode_all(&bytes[index..])
                && let Ok(pkg) = Package::decode(pkg.as_slice())
            {
                package = Some(pkg);
                break;
            }
        }
        if let Some(package) = package {
            package.unpack(&path)?;
            let home = CACHE_DIR.to_string_lossy();

            #[rustfmt::skip]
                let handle = Spawner::new("bwrap")?.mode(Mode::Real).preserve_env(true).args([
                    "--new-session", "--die-with-parent",
                    "--proc", "/proc",

                    "--dev", "/dev",
                    "--dev-bind", "/dev/dri", "/dev/dri",

                    "--bind", "/tmp", "/tmp",
                    "--bind", "/sys", "/sys",
                    "--bind", "/run", "/run",
                    "--bind", "/etc", "/etc",
                    "--bind", "/var", "/var",
                    "--bind", "/usr/share", "/usr/share",

                    "--bind", &path.to_string_lossy(), "/pkg",
                    "--bind", &home, "/usr/share/antimony",
                    "--setenv", "HOME", "/home/antimony",
                    "--dir", "/home/antimony",
                ]);

            let bin = path.join("system").join("bin");
            if bin.exists() {
                #[rustfmt::skip]
                    handle.args_i([
                        "--bind", &bin.to_string_lossy(), "/usr/bin",
                        "--symlink", "/usr/bin", "/bin",
                        "--symlink", "/usr/bin", "/sbin",
                        "--symlink", "/usr/bin", "/usr/sbin"
                    ]);
            }
            let lib = path.join("system").join("lib");
            if lib.exists() {
                #[rustfmt::skip]
                    handle.args_i([
                        "--bind", &lib.to_string_lossy(), "/usr/lib",
                        "--symlink", "/usr/lib", "/lib",
                        "--symlink", "/usr/lib", "/lib64",
                        "--symlink", "/usr/lib", "/usr/lib64"
                    ]);
            }

            let current_str = current.to_string_lossy();
            #[rustfmt::skip]
                handle.args([
                    "--ro-bind", &current_str, "/pkg.sb",
                    "--", "/pkg.sb"
                ]).spawn()?.wait()?;
            Ok(())
        } else {
            Err(anyhow!("Data Header Missing! Corrupted Package!"))
        }
    }
}
