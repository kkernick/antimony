//! The Library Fabricator is most important of all, as it assembles the SOF. It also touches almost every other fabricator, and is
//! the central part of the most important path between bin-library-syscalls. It is also by the far the most complicated, as libraries
//! can encompass files, wildcards, directories, and binaries. They can be sourced from just about anywhere on the system (IE /usr/bin,
//! or /usr/share/application), and it needs to determine which files should be placed in the SOF, and what to do with their dependencies.
//! It relies on LDD to determine ELF dependencies (IE .so files), and Find to scour directories. Everything is aggressively cached, and
//! even more aggressively parallelized.

use crate::{
    fab::{ThreadCache, get_dir, get_libraries, get_wildcards, localize_home},
    shared::{
        Set,
        config::CONFIG_FILE,
        env::{AT_HOME, CACHE_DIR, HOME},
    },
    timer,
};
use ahash::RandomState;
use anyhow::Result;
use dashmap::{DashSet, iter_set::OwningIter};
use log::{debug, error, warn};
use rayon::prelude::*;
use spawn::Spawner;
use std::{
    borrow::Cow,
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::LazyLock,
};
use user::as_effective;

pub static FILES: LazyLock<ThreadCache> = LazyLock::new(DashSet::default);
pub static DIRS: LazyLock<ThreadCache> = LazyLock::new(DashSet::default);
pub static ROOTS: LazyLock<ThreadCache> = LazyLock::new(|| {
    CONFIG_FILE
        .library_roots()
        .par_iter()
        .map(|root| root.to_string())
        .collect()
});

#[inline]
pub fn in_lib(path: &str) -> bool {
    ROOTS.par_iter().any(|r| path.starts_with(r.as_str()))
}

/// Add a file to the SOF.
pub fn add_sof(sof: &Path, library: Cow<'_, str>, cache: &Path) -> Result<()> {
    let sof_path = sof.join(&library.as_ref()[1..]);
    if let Some(parent) = sof_path.parent() {
        if !parent.exists() {
            as_effective!(fs::create_dir_all(parent))??;
        }
        let path = PathBuf::from(library.as_ref());
        let canon = fs::canonicalize(&path)?;

        if !sof_path.exists()
            && let Err(e) = fs::hard_link(&canon, &sof_path)
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
            let shared_path = &cache.join("shared").join(&library.as_ref()[1..]);
            if let Some(parent) = shared_path.parent() {
                as_effective!(Result<()>, {
                    if !parent.exists() {
                        fs::create_dir_all(parent)?;
                    }

                    if !shared_path.exists() {
                        fs::copy(&canon, shared_path)?;
                    }
                    fs::hard_link(shared_path, &sof_path)?;
                    Ok(())
                })??;
            }
        }
    }
    Ok(())
}

pub fn mount_roots(sof: &str, handle: &Spawner) -> Result<()> {
    ROOTS.par_iter().try_for_each(|root| -> Result<()> {
        let path = PathBuf::from(format!("{sof}{}", root.as_str()));
        if path.exists() {
            handle.args_i(["--ro-bind", path.to_str().unwrap(), root.as_ref()])?;
        }
        Ok(())
    })?;

    for root in ["/lib", "/lib64", "/usr/lib64"] {
        if let Ok(link) = fs::read_link(root) {
            let link = if !link.is_absolute() {
                Path::new(root).parent().unwrap().join(link)
            } else {
                link
            }
            .canonicalize()?;

            if let Some(str) = link.to_str()
                && ROOTS.contains(str)
            {
                handle.args_i(["--symlink", str, root])?;
            }
        }
    }

    Ok(())
}

pub fn resolve_wildcards(
    set: Set<String>,
    filter: &'static str,
) -> OwningIter<String, RandomState> {
    let resolved = ThreadCache::default();

    let (wildcards, flat): (HashSet<_>, HashSet<_>) = timer!(
        "::wildcard::partition",
        set.into_par_iter().partition(|e| e.contains('*'))
    );

    timer!("::wildcard::localize", {
        flat.into_par_iter().for_each(|e| {
            if e.starts_with("/") {
                resolved.insert(e.to_string());
            } else if e.starts_with("~") {
                resolved.insert(e.replace("~", HOME.as_str()));
            } else {
                ROOTS.par_iter().for_each(|root| {
                    let path = format!("{}/{e}", root.as_str());
                    if Path::new(&path).exists() {
                        resolved.insert(path);
                    }
                });
            }
        })
    });

    timer!("::wildcard::resolve", {
        wildcards.into_par_iter().for_each(|w| {
            if let Ok(cards) = get_wildcards(&w, true, filter) {
                cards.into_par_iter().for_each(|card| {
                    resolved.insert(card);
                });
            }
        })
    });

    resolved.into_iter()
}

#[inline]
pub fn cache_dir() -> PathBuf {
    CACHE_DIR.join(".lib")
}

#[inline]
pub fn sof_dir(sys_dir: &Path) -> PathBuf {
    sys_dir.join("sof")
}

/// Generate the libraries for a program.
pub fn fabricate(info: &super::FabInfo) -> Result<()> {
    if let Some(libraries) = info.profile.lock().libraries.take() {
        if libraries.no_sof.unwrap_or(false) {
            return mount_roots("", info.handle);
        }

        let cache = cache_dir();
        if !cache.exists() {
            let _ = as_effective!({ fs::create_dir_all(cache.as_path()) });
        }

        let sof = sof_dir(info.sys_dir);
        if !sof.exists() {
            let _ = as_effective!(fs::create_dir(&sof));
        }

        // We do need the cache on disk in case we need to use a shared SOF source.
        if !cache.starts_with(AT_HOME.as_path()) {
            let shared = cache.join("shared");
            if !shared.exists() {
                debug!("Creating shared directory at {}", shared.display());
                let _ = fs::create_dir(&shared);
            }
        }

        timer!("::resolve", {
            rayon::join(
                move || {
                    resolve_wildcards(libraries.directories, "d")
                        .par_bridge()
                        .for_each(|e| {
                            if let Ok(libraries) = get_dir(&e) {
                                libraries.into_par_iter().for_each(|lib| {
                                    let _ = FILES.insert(lib);
                                });
                            }
                            DIRS.insert(e);
                        });
                },
                move || {
                    resolve_wildcards(libraries.files, "f,l")
                        .par_bridge()
                        .for_each(|file| {
                            if let Ok(libraries) = get_libraries(Cow::Borrowed(&file)) {
                                libraries.into_par_iter().for_each(|lib| {
                                    let _ = FILES.insert(lib);
                                });
                            }
                            FILES.insert(file);
                        })
                },
            );
        });

        let sof = sof_dir(info.sys_dir);
        let cache = cache_dir();

        FILES
            .par_iter()
            .filter(|library| {
                if in_lib(library) {
                    true
                } else {
                    let _ = info.handle.args_i([
                        if library.starts_with("/home/") {
                            "--bind"
                        } else {
                            "--ro-bind"
                        },
                        library.as_str(),
                        library.as_str(),
                    ]);
                    false
                }
            })
            // Write the SOF version, as a hard link preferably.
            .for_each(|lib| {
                if let Err(e) = add_sof(&sof, Cow::Borrowed(&lib), &cache) {
                    error!("Failed to add {} to SOF: {e}", lib.as_str())
                }
            });

        let sof_str = sof.to_string_lossy();
        mount_roots(&sof_str, info.handle)?;

        timer!("::mount_directories", {
            DIRS.par_iter().try_for_each(|dir| -> Result<()> {
                if in_lib(dir.as_ref()) {
                    let sof_path = sof.join(&dir[1..]);
                    if !sof_path.exists() {
                        fs::create_dir_all(sof_path)?;
                    }
                }

                info.handle.args_i([
                    if dir.starts_with("/home/") {
                        "--bind"
                    } else {
                        "--ro-bind"
                    },
                    dir.as_ref(),
                    localize_home(dir.as_ref()).as_ref(),
                ])?;
                Ok(())
            })?;
        });
    }
    Ok(())
}
