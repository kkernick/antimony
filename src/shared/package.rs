//! The Antimony Package is a self-contained profile that can run on any device.

use crate::{
    cli::{
        integrate::{self, integrate_vec},
        run, run_vec,
    },
    fab::lib::ROOTS,
    setup::setup,
    shared::{
        Map, Set,
        env::{CACHE_DIR, DATA_HOME},
        find::{DirType, recursive_crawl},
        profile::Profile,
    },
    timer,
};
use anyhow::{Result, anyhow};
use bilrost::{Message, OwnedMessage};
use bstr::{BStr, BString};
use clap::Parser;
use log::{error, info};
use path_clean::clean;
use serde::{Deserialize, Serialize};
use spawn::Spawner;
use std::{
    env,
    ffi::OsStr,
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    sync::LazyLock,
};
use user::Mode;

pub static IS_PACKAGE: LazyLock<Option<PathBuf>> = LazyLock::new(|| {
    // We modify a reserved section of the ELF header, as it's a predictable location that is unused.
    // The MARKER starts and ends with `\0`, and it doesn't trip any systems I've tested on, but this
    // is a hack.
    if let Ok(current) = env::current_exe()
        && let Ok(mut file) = File::open(&current)
        && file.seek(SeekFrom::Start(0x09)).is_ok()
    {
        let mut marker = [0u8; 7];
        let _ = file.read_exact(&mut marker);
        if marker == PACKAGE_MARKER {
            return Some(current);
        }
    }
    None
});

// Package marker
pub static PACKAGE_MARKER: [u8; 7] = *b"\0PKG\0\0\0";

/// Command line arguments specifically for when running as a package
#[derive(clap::Parser, Default)]
#[command(name = "Antimony (Packaged)")]
#[command(version)]
#[command(about = "Sandbox Applications (Self-Contained Package)")]
#[command(before_help = r#"⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣰⣦⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⣴⠟⠹⣧⡀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⣷⣦⣄⣠⣿⠃⢠⣄⠈⢻⣆⣠⣴⡞⡆⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⢀⣀⣀⣿⠀⠈⢻⣇⢀⣾⢟⡄⣸⡿⠋⠀⡇⣇⣀⣀⠀⠀⠀⠀⠀
⠀⣤⣤⣤⣀⣱⢻⠚⠻⣧⣀⠀⢹⡿⠃⠈⢻⣟⠀⢀⣤⠧⠓⣹⣟⣀⣤⣤⣤⡀
⠀⠈⠻⣧⠉⠛⣽⠀⠀⠀⠙⣷⡿⠁⠀⠀⠀⢻⣶⠛⠁⠀⠀⡟⠟⠉⣵⡟⠁⠀
⠀⠀⠀⠹⣧⡀⠏⡇⠀⠀⠀⣿⠁⠀⠀⠀⠀⠀⣿⡄⠀⠀⢠⢷⠀⣼⡟⠀⠀⠀
⠀⠀⠀⠀⠙⣟⢼⡹⡄⠀⠀⣿⡄⠀⠀⠀⠀⢀⣿⡇⠀⢀⣞⣦⢾⠟⠀⠀⠀⠀
⠀⠠⢶⣿⣛⠛⢒⣭⢻⣶⣤⣹⣿⣤⣀⣀⣠⣾⣟⣠⣔⡛⢫⣐⠛⢛⣻⣶⠆⠀
⠀⠀⠀⠉⣻⡽⠛⠉⠁⠀⠉⢙⣿⠖⠒⠛⠻⣿⡋⠉⠁⠈⠉⠙⢿⣿⠉⠀⠀⠀
⠀⠀⠀⠸⠿⠷⠒⣦⣤⣴⣶⢿⣿⡀⠀⠀⠀⣽⡿⢷⣦⠤⢤⡖⠶⠿⠧⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠛⢿⣦⣴⡾⠟⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠙⠟⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀"#)]
pub struct Args {
    /// Ignore existing unpacked directory and unpack again.
    #[arg(long)]
    refresh: bool,

    /// Integrate the package into the system environment.
    #[arg(long)]
    integrate: bool,

    /// Run arguments
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub passthrough: Option<Vec<String>>,
}

#[derive(Deserialize, Serialize, Default, Message)]
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

    /// Symlinks constructed by the package SRC -> DEST. DEST must be provided.
    pub symlinks: Map<String, String>,

    /// System binaries are binaries used by antimony (i.e find, proxy). They are
    /// added to `PATH`
    pub system_binaries: Map<String, BString>,

    /// Libraries needed by the system binaries. Added to `LD_LIBRARY_PATH`.
    pub system_libraries: Map<String, BString>,

    /// Misc system files, like desktop files and icons.
    pub system_files: Map<String, BString>,
}
impl Package {
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

    pub fn add_misc(&mut self, name: &str, dest: &str) -> Result<()> {
        Self::add_path(name, dest, &mut self.system_files)
    }

    pub fn add_symlink(&mut self, name: &str, dest: &str) {
        self.symlinks.insert(name.to_owned(), dest.to_owned());
    }

    /// Add a file to a package field at the desired location.
    pub fn add_path(name: &str, dest: &str, to: &mut Map<String, BString>) -> Result<()> {
        let mut path = Path::new(name);
        if !path.exists() {
            path = Path::new(which::which(name)?);
        }

        let mut add_file = |path: &Path, dest: &str| -> Result<()> {
            info!("Adding {} to package (at {dest})", path.display());
            let mut buffer = Vec::new();
            File::open(path)?.read_to_end(&mut buffer)?;

            to.insert(dest.to_owned(), BString::from(buffer));
            Ok(())
        };

        if path.is_file() {
            add_file(path, dest)?;
        } else {
            let mut crawled = recursive_crawl(&path.to_string_lossy(), None)?;
            if let Some(files) = crawled.remove(&DirType::File) {
                for file in files {
                    if let Some(stripped) = file.strip_prefix("/usr/lib/") {
                        add_file(Path::new(&file), stripped)?;
                    } else {
                        add_file(Path::new(&file), &file)?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Unpack the package
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
            info!("Unpacking {path} to {}...", pkg_path.display());
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
        if !self.symlinks.is_empty() {
            let root = package_dir.join("links");
            fs::create_dir(&root)?;
            for (src, dest) in &self.symlinks {
                symlink(dest, root.join(src.replace('/', "-")))?;
            }
        }

        let root = package_dir.join("system").join("bin");
        fs::create_dir_all(&root)?;
        for (path, content) in &self.system_binaries {
            unpack(&root, path, content.as_ref(), true)?;
        }

        let root = package_dir.join("system").join("lib");
        fs::create_dir_all(&root)?;
        for (path, content) in &self.system_libraries {
            unpack(&root, path, content.as_ref(), true)?;
        }

        let root = package_dir.join("system").join("misc");
        fs::create_dir_all(&root)?;
        for (path, content) in &self.system_files {
            unpack(&root, path, content.as_ref(), false)?;
        }

        Ok(())
    }
}

/// Grab the profile from a package directory.
pub fn get_profile(path: &Path) -> Result<(Profile, PathBuf)> {
    let mut profile_path = None;

    for file in path.read_dir()?.filter_map(Result::ok) {
        if let Some(extension) = file.path().extension()
            && extension == "toml"
        {
            profile_path = Some(file.path());
            break;
        }
    }

    let Some(profile_path) = profile_path else {
        return Err(anyhow!("Could not find package profile"));
    };
    Ok((
        toml::from_str(&fs::read_to_string(&profile_path)?)?,
        profile_path,
    ))
}

#[allow(
    clippy::useless_let_if_seq,
    reason = "It's not useless. Searching the binary may fail."
)]
#[allow(clippy::too_many_lines)]
pub fn execute_package(current: &Path, mut file: File, name: &OsStr) -> Result<()> {
    let root_path = Path::new("/pkg");
    let cli = Args::parse();

    if root_path.exists() {
        info!("In package namespace. Executing packaging...");
        let mut args = cli
            .passthrough
            .map_or_else(run::Args::default, |passthrough| {
                run_vec(&root_path.to_string_lossy(), passthrough)
            });

        let result = || -> Result<()> {
            let info = timer!(
                "::setup",
                setup(
                    root_path.to_string_lossy(),
                    &mut args,
                    false,
                    Some((Package::default(), true))
                )
            )?;
            timer!("::run", run::run(info, &mut args))?;
            Ok(())
        }();

        if let Err(e) = &result {
            error!("{e}");
        }
        result
    } else {
        let path = CACHE_DIR.join("packages").join(name);
        if cli.refresh && path.exists() {
            fs::remove_dir_all(&path)?;
        }

        if !path.exists() {
            let mut package = None;
            file.rewind()?;
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;

            // We might hit multiple marker (At least one, as it's used in the ELF header), but
            // the data shouldn't have it, so it should be the last one.
            for (position, window) in bytes.windows(PACKAGE_MARKER.len()).enumerate() {
                if window == PACKAGE_MARKER
                    && let Some(index) = position.checked_add(PACKAGE_MARKER.len())
                    && let Ok(bytes) = zstd::decode_all(&bytes[index..])
                    && let Ok(pkg) = Package::decode(bytes.as_slice())
                {
                    package = Some(pkg);
                    break;
                }
            }

            if let Some(package) = package {
                package.unpack(&path)?;
            } else {
                return Err(anyhow!("Data Header Missing! Corrupted Package!"));
            }
        }

        if cli.integrate {
            let desktop: Set<_> = path
                .join("system/misc/usr/share/applications")
                .read_dir()
                .map_or_else(
                    |_| Set::default(),
                    |dir| dir.filter_map(Result::ok).map(|p| p.path()).collect(),
                );

            if desktop.is_empty() {
                return Err(anyhow!("This package has no desktop files to integrate"));
            }

            let pkg_path = DATA_HOME.join("antimony").join("packages");
            if !pkg_path.exists() {
                fs::create_dir_all(&pkg_path)?;
            }
            let (mut profile, _) = get_profile(&path)?;
            let cmd = cli
                .passthrough
                .map_or_else(integrate::Args::default, |passthrough| {
                    integrate_vec(&root_path.to_string_lossy(), passthrough)
                });

            // This is all guaranteed.
            if let Some(current) = IS_PACKAGE.as_ref()
                && let Some(filename) = current.file_name()
            {
                let path = pkg_path.join(filename);
                if cmd.remove && path.exists() {
                    fs::remove_file(&path)?;
                } else {
                    fs::copy(current, &path)?;
                }

                for file in desktop {
                    if cmd.remove
                        && let Some(name) = file.file_name()
                    {
                        let path = DATA_HOME.join("applications").join(name);
                        if path.exists() {
                            fs::remove_file(path)?;
                        }
                    } else if let Some(stem) = file.file_stem() {
                        integrate::format_desktop(
                            &cmd,
                            &mut profile,
                            &stem.to_string_lossy(),
                            &file,
                            &path.to_string_lossy(),
                            &DATA_HOME.join("applications"),
                            true,
                        )?;
                    }
                }
            }

            let prefix = format!("{}/system/misc/usr/share", path.display());
            let package_icons = format!("{prefix}/icons");
            if Path::new(&package_icons).exists() {
                recursive_crawl(&package_icons, None)?
                    .remove(&DirType::File)
                    .unwrap_or_default()
                    .into_iter()
                    .try_for_each(|path| -> Result<()> {
                        let out = path.replacen(&prefix, &DATA_HOME.to_string_lossy(), 1);
                        if cmd.remove {
                            let path = Path::new(&out);
                            if path.exists() {
                                // Remove the icon
                                fs::remove_file(path)?;
                                let mut parent = path.parent();

                                // While we have a parent
                                while let Some(dir) = parent

                                // And we haven't reached the icon root
                                && dir != DATA_HOME.join("icons")

                                // And there are files
                                && let Ok(iter) = fs::read_dir(dir)
                                && let contents = iter.collect::<Vec<_>>()

                                // And the only entry is either the previous directory (== 1)
                                // Or the original parent (== 0)
                                && contents.len() <= 1
                                {
                                    // Remove that as well, and bubble up.
                                    fs::remove_dir_all(dir)?;
                                    parent = dir.parent();
                                }
                            }
                        } else {
                            if let Some(parent) = Path::new(&out).parent()
                                && !parent.exists()
                            {
                                fs::create_dir_all(parent)?;
                            }
                            fs::copy(path, out)?;
                        }
                        Ok(())
                    })?;
            }

            return Ok(());
        }

        let home = CACHE_DIR.to_string_lossy();

        #[rustfmt::skip]
        let handle = Spawner::new("bwrap")?.mode(Mode::Real).preserve_env(true).args([
            "--new-session", "--die-with-parent",
            "--proc", "/proc",
            "--dev", "/dev",
            "--bind", "/tmp", "/tmp",
            "--bind", "/sys", "/sys",
            "--bind", "/run", "/run",
            "--bind", "/etc", "/etc",
            "--bind", "/var", "/var",
            "--bind", "/usr/share", "/usr/share",
            "--bind", "/home", "/home",
            "--bind", &home, &home,
            "--bind", &path.to_string_lossy(), "/pkg",
        ]);

        let bin = path.join("system").join("bin");
        #[rustfmt::skip]
        handle.args_i([
            "--overlay-src", "/usr/bin",
            "--overlay-src", &bin.to_string_lossy(),
            "--ro-overlay", "/usr/bin",
            "--symlink", "/usr/bin", "/bin",
            "--symlink", "/usr/bin", "/sbin",
            "--symlink", "/usr/bin", "/usr/sbin"
        ]);

        let lib = path.join("system").join("lib");
        for root in ["/usr/lib", "/usr/lib64", "/usr/lib32"] {
            let path = Path::new(&root);
            if path.exists() && !path.is_symlink() {
                handle.args_i(["--overlay-src", root]);
            }
        }

        #[rustfmt::skip]
        handle.args_i([
            "--overlay-src", &lib.to_string_lossy(),
            "--ro-overlay", "/usr/lib",
            "--symlink", "/usr/lib", "/lib",
            "--symlink", "/usr/lib", "/lib64",
            "--symlink", "/usr/lib", "/usr/lib64"
        ]);

        let current_str = current.to_string_lossy();
        #[rustfmt::skip]
        handle.args([
            "--ro-bind", &current_str, "/pkg.sb",
            "--", "/pkg.sb"
        ]).args(env::args().skip(1)).spawn()?.wait()?;
        Ok(())
    }
}
