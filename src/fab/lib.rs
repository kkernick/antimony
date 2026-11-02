//! The Library Fabricator is most important of all, as it assembles the SOF.
//!
//! It also touches almost every other fabricator, and is
//! the central part of the most important path between bin-library-syscalls. It is also by the far the most complicated, as libraries
//! can encompass files, wildcards, directories, and binaries. They can be sourced from just about anywhere on the system (i.e. /usr/bin,
//! or /usr/share/application), and it needs to determine which files should be placed in the SOF, and what to do with their dependencies.
//! It relies on LDD to determine ELF dependencies (i.e. .so files), and Find to scour directories. Everything is aggressively cached, and
//! even more aggressively parallelized.
#![allow(clippy::missing_errors_doc)]

use crate::shared::landlock::update_policy;
use crate::shared::store::{CACHE_STORE, Object};
use crate::shared::{StaticHash, create_parent, create_path, utility};
use crate::{
    fab::{find_folders, get_libraries, in_lib, localize_home},
    shared::{
        Set, ThreadSet,
        config::CONFIG_FILE,
        env::{AT_HOME, CACHE_DIR, HOME},
        find::{self, WildcardFilter},
    },
    timer,
};
use anyhow::{Result, anyhow};
use caps::Capability;
use dashmap::iter_set::OwningIter;
use heck::ToTitleCase;
use landlock::ruleset::Filesystem;
use log::{Level, error, warn};
use rayon::prelude::*;
use spawn::Spawner;
use std::{
    borrow::Cow,
    fs::{self, File},
    io::{self, ErrorKind, Read},
    path::{Path, PathBuf},
    sync::LazyLock,
};
use user::{Mode, as_effective};

/// Library Files (i.e. .so files)
pub static FILES: LazyLock<ThreadSet<String>> = LazyLock::new(ThreadSet::default);

/// Library Directories (e.g. /usr/lib/qt6)
pub static DIRS: LazyLock<ThreadSet<String>> = LazyLock::new(ThreadSet::default);

/// Roots to search for files and directories
pub static ROOTS: LazyLock<ThreadSet<String>> = LazyLock::new(|| {
    parse_cache("/etc/ld.so.cache").map_or_default(|mut roots| {
        if let Some(cached) = &CONFIG_FILE.library_roots {
            roots.extend(cached.clone());
        }

        roots
            .into_iter()
            .filter(|root| !root.is_empty() && root != "/")
            .map(|root| {
                if root.ends_with('/') {
                    root
                } else {
                    format!("{root}/")
                }
            })
            .collect()
    })
});

/// Parse the host ld.so.cache to extract roots
#[allow(clippy::arithmetic_side_effects)]
fn parse_cache(filepath: &str) -> Result<Set<String>> {
    let roots = if let Ok(bytes) = CACHE_STORE.borrow().bytes("ld.so", Object::Libraries) {
        bitcode::deserialize(bytes.as_slice())?
    } else {
        let mut file = File::open(filepath)?;
        let mut data = Vec::default();
        file.read_to_end(&mut data)?;

        let magic_new = b"glibc-ld.so.cache1.1";
        let h = data
            .windows(magic_new.len())
            .position(|window| window == magic_new)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Modern glibc cache format not found.",
                )
            })?;

        let size = 20 + 4 + 4 + 5 * 4;
        let libs = u32::from_le_bytes([data[h + 20], data[h + 21], data[h + 22], data[h + 23]]);
        let offset = h + size;
        let entry_size = 24;

        let mut roots: Set<String> = (0..libs)
            .into_par_iter()
            .map(|i| offset + (i as usize * entry_size))
            .filter(|c| c + entry_size <= data.len())
            .filter_map(|c| {
                let val_idx =
                    u32::from_le_bytes([data[c + 8], data[c + 9], data[c + 10], data[c + 11]])
                        as usize;
                let ptr = offset + val_idx;
                let end = data[ptr..]
                    .iter()
                    .position(|&b| b == 0)
                    .map_or(data.len(), |pos| ptr + pos);
                if let Ok(path) = str::from_utf8(&data[ptr..end])
                    && path.starts_with('/')
                    && let Some(parent) = Path::new(&path).parent()
                    && let Ok(canon) = parent.canonicalize()
                    && canon.exists()
                {
                    Some(canon.to_string_lossy().to_string())
                } else {
                    None
                }
            })
            .collect();

        for auxiliary in ["/usr/lib64", "/usr/libexec"] {
            let path = Path::new(auxiliary);
            if path.exists()
                && !path.is_symlink()
                && let Ok(canon) = path.canonicalize()
            {
                roots.insert(canon.to_string_lossy().into_owned());
            }
        }
        CACHE_STORE.borrow().dump(
            "ld.so",
            Object::Libraries,
            bitcode::serialize(&roots)?.as_slice(),
        )?;
        roots
    };
    Ok(roots)
}

/// Add a file to the SOF.
/// This function must be run underneath an effective UID
///
/// ## Errors
/// If the path cannot be resolved, or the file cannot be added to the SOF.
#[inline]
pub fn add_sof(sof: &Path, library: &str, cache: &Path) -> Result<()> {
    let sof_path = sof.join(library.strip_prefix("/").unwrap_or(library));
    create_parent(sof_path.as_path())?;

    let path: Cow<'_, Path> = Cow::Owned(PathBuf::from(library));
    let canon = if path.is_symlink() {
        Cow::Owned(fs::canonicalize(path.as_ref())?)
    } else {
        Cow::Borrowed(path.as_ref())
    };

    if let Err(e) = fs::hard_link(canon.as_ref(), &sof_path)
        && e.kind() != ErrorKind::AlreadyExists
    {
        warn!(
            "Failed to hardlink {} => {}: {e}",
            canon.display(),
            sof_path.display()
        );

        // If we cannot hard-link directly, then we created a shared source
        // of library copies within the CACHE_DIR, then hard-link from that.
        //
        // This reduces redundancy between profiles, and since shared exists
        // in the CACHE_DIR, hard links will work.
        let shared_path = &cache
            .join("shared")
            .join(library.strip_prefix('/').unwrap_or(library));

        // Copy will always overwrite, so we need to check if it exists
        if !shared_path.exists()
            && let Err(e) = fs::copy(canon.as_ref(), shared_path)
        {
            // Only create the parent if we actually need to.
            if e.kind() == ErrorKind::NotFound {
                create_parent(shared_path.as_path())?;
                fs::copy(canon.as_ref(), shared_path)?;
            } else {
                return Err(e.into());
            }
        }
        fs::hard_link(shared_path, &sof_path)?;
    }

    Ok(())
}

/// Mount the roots created in the SOF into the sandbox.
#[inline]
pub fn mount_roots(sof: &str, handle: &Spawner) -> Result<()> {
    ROOTS.iter().try_for_each(|root| -> Result<()> {
        let path = PathBuf::from(format!("{sof}{}", root.as_str()));
        if path.exists() {
            if sof.is_empty() {
                handle.args_i(["--ro-bind", &path.display().to_string(), root.as_ref()]);
            } else {
                handle.args_i([
                    "--overlay-src",
                    &path.display().to_string(),
                    "--tmp-overlay",
                    root.as_ref(),
                ]);
            }
        }
        Ok(())
    })?;

    for root in [
        ("/usr/lib64/", "/usr/lib"),
        ("/lib/", "/usr/lib"),
        ("/lib64/", "/usr/lib64"),
    ] {
        if !ROOTS.contains(root.0) {
            handle.args_i([
                "--symlink",
                root.1,
                root.0.strip_suffix('/').unwrap_or(root.0),
            ]);
        }
    }

    Ok(())
}

/// Resolve any wildcards in the given set.
#[inline]
fn resolve_wildcards(set: Set<String>, filter: WildcardFilter) -> OwningIter<String, StaticHash> {
    let resolved = ThreadSet::<String>::default();

    let (wildcards, flat): (Set<_>, Set<_>) = timer!(
        "::wildcard::partition",
        set.into_par_iter().partition(|e| e.contains('*'))
    );

    timer!("::wildcard::localize", {
        flat.into_par_iter().for_each(|e| {
            if e.starts_with('/') {
                resolved.insert(e);
            } else if e.starts_with('~') {
                resolved.insert(e.replacen('~', HOME.as_str(), 1));
            } else {
                for root in ROOTS.iter() {
                    let path = format!("{}/{e}", root.as_str());
                    if Path::new(&path).exists() {
                        resolved.insert(path);
                        if filter == WildcardFilter::Files {
                            break;
                        }
                    }
                }
            }
        });
    });

    timer!("::wildcard::resolve", {
        wildcards.into_par_iter().for_each(|w| {
            if let Ok(cards) = find::wildcards(&w, true, filter) {
                cards.into_par_iter().for_each(|card| {
                    resolved.insert(card);
                });
            }
        });
    });

    resolved.into_iter()
}

#[inline]
pub fn cache_dir() -> PathBuf {
    CACHE_DIR.join(".lib")
}

#[inline]
#[must_use]
pub fn sof_dir(sys_dir: &Path) -> PathBuf {
    sys_dir.join("sof")
}

/// Generate the libraries for a program.
#[allow(clippy::too_many_lines)]
pub fn fabricate(info: &mut super::FabInfo) -> Result<()> {
    let no_sof = info
        .profile
        .libraries
        .as_ref()
        .is_some_and(|libraries| libraries.no_sof.unwrap_or(false));

    ROOTS.iter().for_each(|lib_root| {
        for norm in [
            Cow::Borrowed(info.name),
            Cow::Owned(info.name.to_title_case()),
            Cow::Owned(info.name.to_lowercase()),
        ] {
            let name = format!("{}/{}", lib_root.as_str(), norm);
            if Path::new(&name).exists() {
                if no_sof {
                    info.handle.args_i(["--ro-bind", &name, &name]);
                } else if Path::new(&name).is_dir() {
                    info.profile
                        .libraries
                        .get_or_insert_default()
                        .directories
                        .get_or_insert_default()
                        .insert(name);
                } else {
                    info.profile
                        .libraries
                        .get_or_insert_default()
                        .files
                        .get_or_insert_default()
                        .insert(name);
                }
            }
        }
    });

    // Each is sent to the library fabricator, in case they contain anything,
    // and are then mounted directly.
    for path in find_folders(info.name) {
        if no_sof {
            info.handle.args_i(["--ro-bind", &path, &path]);
        } else {
            info.profile
                .libraries
                .get_or_insert_default()
                .directories
                .get_or_insert_default()
                .insert(path);
        }
    }

    let allowed: Set<_> = [
        Filesystem::ReadFile,
        Filesystem::ReadDir,
        Filesystem::Execute,
    ]
    .into_iter()
    .collect();

    if no_sof {
        mount_roots("", info.handle)?;
    } else {
        if let Some(binaries) = &info.profile.binaries {
            timer!("::binaries", {
                binaries
                    .par_iter()
                    .for_each(|binary| match get_libraries(binary) {
                        Ok(libraries) => {
                            for lib in libraries {
                                {
                                    let _ = FILES.insert(lib);
                                }
                            }
                        }
                        Err(e) => warn!("Could not get libraries for {binary}: {e}"),
                    });
            });
        }

        if let Some(mut libraries) = info.profile.libraries.take() {
            if let Some(directories) = libraries.directories.take() {
                timer!(
                    "::lib::directories",
                    resolve_wildcards(directories, WildcardFilter::Directories)
                        .par_bridge()
                        .for_each(|e| {
                            if let Ok(libraries) = find::dir(&e) {
                                libraries.into_par_iter().for_each(|lib| {
                                    let _ = FILES.insert(lib);
                                });
                            }
                            DIRS.insert(e);
                        })
                );
            }

            if let Some(files) = libraries.files.take() {
                timer!(
                    "::lib::files",
                    resolve_wildcards(files, WildcardFilter::Files)
                        .par_bridge()
                        .for_each(|file| {
                            if let Ok(libraries) = get_libraries(&file) {
                                libraries.into_par_iter().for_each(|lib| {
                                    let _ = FILES.insert(lib);
                                });
                            }
                            FILES.insert(file);
                        })
                );
            }
        }

        let sof = sof_dir(info.sys_dir);
        let cache = timer!("::setup", {
            as_effective!(Result<PathBuf>, {
                let cache = cache_dir();
                create_path(cache.as_path())?;
                create_path(sof.as_path())?;

                // We do need the cache on disk in case we need to use a shared SOF source.
                if !cache.starts_with(AT_HOME.as_path()) {
                    let shared = cache.join("shared");
                    create_path(shared.as_path())?;
                }
                Ok(cache)
            })?
        });

        if !FILES.is_empty() {
            timer!("::write_files", {
                let libraries: Set<_> = FILES
                    .par_iter()
                    .filter(|library| {
                        if in_lib(library) {
                            true
                        } else {
                            info.handle.args_i([
                                if library.starts_with("/home/") {
                                    "--bind"
                                } else {
                                    "--ro-bind"
                                },
                                library,
                                library,
                            ]);
                            false
                        }
                    })
                    .map(|lib| lib.to_owned())
                    .collect();

                if *user::SETUID {
                    let code = Spawner::abs(utility("writer"))
                        .new_privileges(true)
                        .cap(Capability::CAP_FOWNER)
                        .mode(Mode::Effective)
                        .error(spawn::StreamMode::Log(Level::Warn))
                        .args([sof.to_string_lossy(), cache.to_string_lossy()])
                        .args(libraries)
                        .spawn()?
                        .wait()?;

                    if code != 0 {
                        return Err(anyhow!("Failed to create SOF via writer: {code}"));
                    }
                } else {
                    libraries.par_iter().for_each(|lib| {
                        if let Err(e) = add_sof(&sof, lib, &cache) {
                            error!("Failed to add {} to SOF: {e}", lib.as_str());
                        }
                    });
                }
            });

            let sof_str = sof.to_string_lossy();
            timer!("::mount_roots", mount_roots(&sof_str, info.handle))?;
        }

        if !DIRS.is_empty() {
            timer!("::mount_directories", {
                for dir in DIRS.iter() {
                    let local = localize_home(dir.as_ref());
                    let home = dir.starts_with("/home/");

                    if in_lib(dir.as_ref()) {
                        let sof_path = sof.join(dir.strip_prefix('/').unwrap_or(&dir));
                        as_effective!({ create_path(sof_path.as_path()) })?;
                    } else if !home && let Some(policy) = info.policy {
                        update_policy(dir.as_str(), policy, allowed.clone());
                    }

                    info.handle
                        .args_i([if home { "--bind" } else { "--ro-bind" }, &dir, &local]);
                }
            });
        }
    }
    if let Some(policy) = info.policy {
        for root in ROOTS.iter() {
            update_policy(root.as_str(), policy, allowed.clone());
        }
    }

    Ok(())
}
