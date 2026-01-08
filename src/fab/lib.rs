//! The Library Fabricator is most important of all, as it assembles the SOF. It also touches almost every other fabricator, and is
//! the central part of the most important path between bin-library-syscalls. It is also by the far the most complicated, as libraries
//! can encompass files, wildcards, directories, and binaries. They can be sourced from just about anywhere on the system (IE /usr/bin,
//! or /usr/share/application), and it needs to determine which files should be placed in the SOF, and what to do with their dependencies.
//! It relies on LDD to determine ELF dependencies (IE .so files), and Find to scour directories. Everything is aggressively cached, and
//! even more aggressively parallelized.

use crate::{
    fab::{
        LIB_ROOTS, SINGLE_LIB, elf_filter, get_dir, get_libraries, get_wildcards, localize_home,
    },
    shared::{
        Set,
        env::{AT_HOME, HOME},
    },
    timer,
};
use anyhow::{Result, anyhow};
use dashmap::DashSet;
use log::{debug, error, trace, warn};
use rayon::prelude::*;
use std::{
    borrow::Cow,
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};
use user::as_effective;

#[inline]
pub fn in_lib(path: &str) -> bool {
    path.starts_with("/usr/lib") || (!*SINGLE_LIB && path.starts_with("/usr/lib64"))
}

/// Determine dependencies for directories.
#[inline]
fn dir_resolve(library: Cow<'_, str>, directories: Arc<DashSet<String>>) -> Result<Vec<String>> {
    let mut dependencies = Vec::new();
    let path = Path::new(library.as_ref());

    // Resolve directories.
    if path.is_dir() {
        dependencies.extend(get_dir(&library)?);
        directories.insert(library.to_string());
    } else if let Some(library) = elf_filter(&library) {
        dependencies.push(library);
    }

    Ok(dependencies)
}

/// Resolve a regular path to its path in the SOF.
#[inline]
pub fn get_sof_path(sof: &Path, library: &str, prefix: &str) -> PathBuf {
    PathBuf::from(library.replace(prefix, &sof.to_string_lossy()))
}

/// Add a file to the SOF.
pub fn add_sof(sof: &Path, library: Cow<'_, str>, cache: &Path, prefix: &str) -> Result<()> {
    let sof_path = get_sof_path(sof, &library, prefix);

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
            let shared_path = get_sof_path(&cache.join("shared"), &library, prefix);
            if let Some(parent) = shared_path.parent() {
                as_effective!(Result<()>, {
                    if !parent.exists() {
                        fs::create_dir_all(parent)?;
                    }

                    if !shared_path.exists() {
                        fs::copy(&canon, &shared_path)?;
                    }
                    fs::hard_link(&shared_path, &sof_path)?;
                    Ok(())
                })??;
            }
        }
    }
    Ok(())
}

/// Generate the libraries for a program.
pub fn fabricate(info: &super::FabInfo) -> Result<()> {
    {
        let profile_libraries = &info.profile.lock().libraries;
        if profile_libraries.contains("/usr/lib") {
            #[rustfmt::skip]
        info.handle.args_i([
            "--ro-bind", "/usr/lib", "/usr/lib",
            "--ro-bind", "/usr/lib64", "/usr/lib64",
            "--symlink", "/usr/lib", "/lib",
            "--symlink", "/usr/lib64", "/lib64",
        ])?;
            return Ok(());
        }
    }

    let cache = Arc::new(crate::shared::env::CACHE_DIR.join(".lib"));
    if !cache.exists() {
        as_effective!({ fs::create_dir_all(cache.as_path()) })??;
    }

    debug!("Creating SOF");
    let sof = info.sys_dir.join("sof");
    if !sof.exists() {
        fs::create_dir(&sof)?;
    }

    // Libraries needed by the program. No Binaries.
    let dependencies: Arc<DashSet<String, ahash::RandomState>> = Arc::default();

    // Scope the lock. We do binaries first, since we piggy-back off the LDD call
    // to find library roots.
    {
        let binaries = &info.profile.lock().binaries;
        timer!("::binaries", {
            binaries.par_iter().for_each(|binary| {
                let dep = dependencies.clone();
                if let Ok(libraries) = get_libraries(Cow::Owned(binary.clone())) {
                    for library in libraries {
                        dep.insert(library);
                    }
                }
            })
        });
    }

    // We do need the cache on disk in case we need to use a shared SOF source.
    if !cache.starts_with(AT_HOME.as_path()) {
        let shared = cache.join("shared");
        if !shared.exists() {
            debug!("Creating shared directory at {}", shared.display());
            fs::create_dir(&shared)?;
        }
    }

    // Libraries and Binaries, Wildcard and Directories Resolved
    let resolved = Arc::new(DashSet::new());

    // Directories to exclude and attach
    let directories = Arc::from(DashSet::new());

    if let Some(roots) = LIB_ROOTS.get() {
        for lib_root in roots.iter() {
            let app_lib = format!("{lib_root}/{}", info.name);
            trace!("Checking {app_lib}");
            if Path::new(&app_lib).exists() {
                debug!("Adding program lib folder");
                for exe in dir_resolve(Cow::Owned(app_lib), directories.clone())? {
                    resolved.insert(Cow::Owned(exe));
                }
            }
        }
    }

    timer!("::wildcards", {
        let profile_libraries = &info.profile.lock().libraries;
        // Separate the wildcards from the files/dirs.
        let (wildcards, flat): (Set<_>, Set<_>) = profile_libraries
            .into_par_iter()
            .partition(|e| e.contains('*'));

        debug!("Formatting flat");
        flat.into_par_iter().for_each(|e| {
            if e.starts_with("/") {
                resolved.insert(Cow::Owned(e.to_string()));
            } else if e.starts_with("~") {
                resolved.insert(Cow::Owned(e.replace("~", HOME.as_str())));
            } else if let Some(roots) = LIB_ROOTS.get() {
                for root in roots.iter() {
                    let path = format!("{root}/{e}");
                    if Path::new(&path).exists() {
                        resolved.insert(Cow::Owned(path));
                        return;
                    }
                }
                warn!("Failed to find library: {e}");
            }
        });

        debug!("Resolving wildcards");
        wildcards.into_par_iter().for_each(|w| {
            if let Ok(cards) = get_wildcards(w, true) {
                cards.into_par_iter().for_each(|card| {
                    resolved.insert(Cow::Owned(card.clone()));
                });
            }
        });
    });

    let files = timer!("::directories", {
        Arc::into_inner(resolved)
            .unwrap()
            .into_par_iter()
            .filter_map(|e| dir_resolve(e, directories.clone()).ok())
            .flatten()
            .collect::<Set<_>>()
    });

    timer!("::resolve", {
        files.into_par_iter().for_each(|file| {
            let dep = dependencies.clone();
            dep.insert(file.clone());
            if let Ok(libraries) = get_libraries(Cow::Owned(file)) {
                for library in libraries {
                    dep.insert(library);
                }
            }
        });
    });

    let dependencies = Arc::into_inner(dependencies).unwrap();

    timer!("::writing", {
        dependencies
            .into_par_iter()
            .map(|library| {
                let mut resolved = library;
                if resolved.starts_with("/lib") {
                    resolved = format!("/usr{resolved}");
                }
                if *SINGLE_LIB {
                    resolved = resolved.replace("/lib64/", "/lib/");
                }
                resolved
            })
            // Filter things that aren't in /usr/lib
            .filter(|library| {
                let parent = if let Some(i) = library.rfind('/') {
                    &library[..i]
                } else {
                    library.as_str()
                };

                if parent.contains("lib")
                    && let Some(roots) = LIB_ROOTS.get()
                    && roots.iter().any(|r| parent == r)
                {
                    true
                } else {
                    !directories
                        .iter()
                        .any(|dir| parent.starts_with(dir.as_str()))
                }
            })
            // Write the SOF version, as a hard link preferably.
            .for_each(|lib| {
                if let Err(e) = add_sof(&sof, Cow::Borrowed(&lib), &cache, "/usr") {
                    error!("Failed to add {lib} to SOF: {e}")
                }
            });
    });

    let sof_str = sof.to_string_lossy();
    info.handle
        .args_i(["--ro-bind-try", &format!("{sof_str}/lib"), "/usr/lib"])?;

    let path = &format!("{sof_str}/lib64");
    if Path::new(path).exists() {
        info.handle.args_i(["--ro-bind-try", path, "/usr/lib64"])?;
    } else {
        info.handle
            .args_i(["--symlink", "/usr/lib", "/usr/lib64"])?;
    }

    #[rustfmt::skip]
    info.handle.args_i([
        "--symlink", "/usr/lib", "/lib",
        "--symlink", "/usr/lib64", "/lib64",
    ])?;

    timer!("::mount_directories", {
        directories.par_iter().try_for_each(|dir| -> Result<()> {
            if let Some(roots) = LIB_ROOTS.get() {
                if roots.iter().any(|r| dir.starts_with(r)) {
                    let sof_path = get_sof_path(&sof, dir.as_str(), "/usr");
                    if !sof_path.exists() {
                        fs::create_dir_all(sof_path)?;
                    }
                }
            } else {
                return Err(anyhow!("Roots not initialized!"));
            }

            info.handle.args_i([
                "--ro-bind",
                dir.as_str(),
                localize_home(dir.as_str()).as_ref(),
            ])?;
            Ok(())
        })?;
    });

    Ok(())
}
