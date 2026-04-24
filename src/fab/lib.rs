//! The Library Fabricator is most important of all, as it assembles the SOF. It also touches almost every other fabricator, and is
//! the central part of the most important path between bin-library-syscalls. It is also by the far the most complicated, as libraries
//! can encompass files, wildcards, directories, and binaries. They can be sourced from just about anywhere on the system (IE /usr/bin,
//! or /usr/share/application), and it needs to determine which files should be placed in the SOF, and what to do with their dependencies.
//! It relies on LDD to determine ELF dependencies (IE .so files), and Find to scour directories. Everything is aggressively cached, and
//! even more aggressively parallelized.

use crate::{
    fab::{get_dir, get_libraries, get_wildcards, in_lib, localize_home},
    shared::{
        Set, StaticHash, ThreadSet,
        config::CONFIG_FILE,
        env::{AT_HOME, CACHE_DIR, HOME},
    },
    timer,
};
use anyhow::Result;
use dashmap::iter_set::OwningIter;
use log::{debug, error, warn};
use rayon::prelude::*;
use spawn::Spawner;
use std::{
    borrow::Cow,
    fs,
    path::{Path, PathBuf},
    sync::LazyLock,
};

static FILES: LazyLock<ThreadSet<String>> = LazyLock::new(ThreadSet::default);
pub static DIRS: LazyLock<ThreadSet<String>> = LazyLock::new(ThreadSet::default);
pub static ROOTS: LazyLock<ThreadSet<String>> = LazyLock::new(|| {
    CONFIG_FILE
        .library_roots()
        .par_iter()
        .map(|root| root.to_string())
        .collect()
});

/// Add a file to the SOF.
/// This function must be run underneath an effective UID
#[inline]
pub fn add_sof(sof: &Path, library: Cow<'_, str>, cache: &Path) -> Result<()> {
    let sof_path = sof.join(&library.as_ref()[1..]);
    if let Some(parent) = sof_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
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
                if !parent.exists() {
                    fs::create_dir_all(parent)?;
                }

                if !shared_path.exists() {
                    fs::copy(&canon, shared_path)?;
                }
                fs::hard_link(shared_path, &sof_path)?;
            }
        }
    }
    Ok(())
}

#[inline]
pub fn mount_roots(sof: &str, handle: &Spawner) -> Result<()> {
    ROOTS.par_iter().try_for_each(|root| -> Result<()> {
        let path = PathBuf::from(format!("{sof}{}", root.as_str()));
        if path.exists() {
            if sof.is_empty() {
                handle.args_i(["--ro-bind", path.to_str().unwrap(), root.as_ref()]);
            } else {
                handle.args_i([
                    "--overlay-src",
                    path.to_str().unwrap(),
                    "--tmp-overlay",
                    root.as_ref(),
                ]);
            }
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
                handle.args_i(["--symlink", str, root]);
            }
        }
    }

    Ok(())
}

#[derive(Copy, Clone, PartialEq)]
pub enum WildcardFilter {
    Files,
    Directories,
}
impl WildcardFilter {
    pub fn find_filter(&self) -> &'static str {
        match self {
            WildcardFilter::Files => "f,l",
            WildcardFilter::Directories => "d",
        }
    }
}

#[inline(always)]
fn resolve_wildcards(set: Set<String>, filter: WildcardFilter) -> OwningIter<String, StaticHash> {
    let resolved = ThreadSet::default();

    let (wildcards, flat): (Set<_>, Set<_>) = timer!(
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
pub fn fabricate(info: &mut super::FabInfo) -> Result<()> {
    let no_sof = if let Some(libraries) = &info.profile.libraries {
        libraries.no_sof.unwrap_or(false)
    } else {
        false
    };

    // Each is sent to the library fabricator, in case they contain anything,
    // and are then mounted directly.
    [
        Path::new("/etc"),
        Path::new("/usr/share"),
        Path::new("/opt"),
    ]
    .into_iter()
    .filter_map(|path| {
        let path = path.join(info.name);
        if path.exists() {
            Some(path.to_string_lossy().into_owned())
        } else {
            None
        }
    })
    .for_each(|path| {
        if no_sof {
            info.handle.args_i(["--ro-bind", &path, &path]);
        } else {
            info.profile
                .libraries
                .get_or_insert_default()
                .directories
                .insert(path);
        }
    });

    if no_sof {
        return mount_roots("", info.handle);
    }

    ROOTS.iter().for_each(|lib_root| {
        let name = format!("{}/{}", lib_root.as_str(), info.name);
        if Path::new(&name).exists() {
            let _ = info
                .profile
                .libraries
                .get_or_insert_default()
                .directories
                .insert(name);
        }
    });

    timer!("::binaries", {
        info.profile.binaries.par_iter().for_each(|binary| {
            if let Ok(libraries) = get_libraries(binary) {
                libraries.into_iter().for_each(|lib| {
                    let _ = FILES.insert(lib);
                });
            }
        });
    });

    if let Some(libraries) = info.profile.libraries.take() {
        timer!("::resolve", {
            rayon::join(
                move || {
                    timer!(
                        "::directories",
                        resolve_wildcards(libraries.directories, WildcardFilter::Directories)
                            .par_bridge()
                            .for_each(|e| {
                                if let Ok(libraries) = get_dir(&e) {
                                    libraries.into_par_iter().for_each(|lib| {
                                        let _ = FILES.insert(lib);
                                    });
                                }
                                DIRS.insert(e);
                            })
                    );
                },
                move || {
                    timer!(
                        "::files",
                        resolve_wildcards(libraries.files, WildcardFilter::Files)
                            .par_bridge()
                            .for_each(|file| {
                                if let Ok(libraries) = get_libraries(&file) {
                                    libraries.into_par_iter().for_each(|lib| {
                                        let _ = FILES.insert(lib);
                                    });
                                }
                                FILES.insert(file);
                            })
                    )
                },
            );
        });
    }

    let sof = sof_dir(info.sys_dir);
    let cache = timer!("::setup", {
        let cache = cache_dir();
        if !cache.exists() {
            fs::create_dir_all(cache.as_path())?;
        }
        if !sof.exists() {
            fs::create_dir(&sof)?;
        }

        // We do need the cache on disk in case we need to use a shared SOF source.
        if !cache.starts_with(AT_HOME.as_path()) {
            let shared = cache.join("shared");
            if !shared.exists() {
                debug!("Creating shared directory at {}", shared.display());
                let _ = fs::create_dir(&shared);
            }
        }
        cache
    });

    if !FILES.is_empty() {
        timer!(
            "::write_files",
            FILES
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
                })
        );

        let sof_str = sof.to_string_lossy();
        timer!("::mount_roots", mount_roots(&sof_str, info.handle))?;
    }

    if !DIRS.is_empty() {
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
                ]);
                Ok(())
            })?;
        });
    }
    Ok(())
}
