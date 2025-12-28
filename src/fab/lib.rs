use crate::{
    debug_timer,
    fab::{
        LIB_ROOTS, SINGLE_LIB, elf_filter, get_dir, get_libraries, get_wildcards, localize_home,
    },
    shared::{
        Set,
        env::{AT_HOME, HOME},
        profile::Profile,
    },
};
use ahash::HashSetExt;
use anyhow::{Result, anyhow};
use dashmap::DashSet;
use log::{debug, error, warn};
use rayon::prelude::*;
use spawn::Spawner;
use std::{
    borrow::Cow,
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

pub fn in_lib(path: &str) -> bool {
    path.starts_with("/usr/lib") || (!*SINGLE_LIB && path.starts_with("/usr/lib64"))
}

/// Determine dependencies for directories.
fn dir_resolve(
    library: Cow<'_, str>,
    directories: Arc<DashSet<String>>,
    cache: &Path,
) -> Result<Vec<String>> {
    let mut dependencies = Vec::new();
    let path = Path::new(library.as_ref());

    // Resolve directories.
    if path.is_dir() {
        dependencies.extend(get_dir(&library, Some(cache))?);
        directories.insert(library.to_string());
    } else if let Some(library) = elf_filter(&library) {
        dependencies.push(library);
    }
    Ok(dependencies)
}

fn get_sof_path(sof: &Path, library: &str) -> PathBuf {
    PathBuf::from(library.replace("/usr", &sof.to_string_lossy()))
}

pub fn add_sof(sof: &Path, library: Cow<'_, str>, cache: &Path) -> Result<()> {
    let sof_path = get_sof_path(sof, &library);

    if let Some(parent) = sof_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
        let path = PathBuf::from(library.as_ref());
        let canon = fs::canonicalize(&path)?;

        if let Err(e) = fs::hard_link(&canon, &sof_path)
            && e.kind() != io::ErrorKind::AlreadyExists
        {
            // If we cannot hard-link directly, then we created a shared source
            // of library copies within the CACHE_DIR, then hard-link from that.
            //
            // This reduces redundancy between profiles, and since shared exists
            // in the CACHE_DIR, hard links will work.

            let shared_path = get_sof_path(&cache.join("shared"), &library);
            if let Some(parent) = shared_path.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(&canon, &shared_path)?;
                fs::hard_link(&shared_path, &sof_path)?;
            }
        }
    }
    Ok(())
}

/// Generate the libraries for a program.
pub fn fabricate(
    profile: &mut Profile,
    name: &str,
    sys_dir: &Path,
    handle: &Spawner,
) -> Result<()> {
    if let Some(libraries) = &profile.libraries
        && libraries.contains("/usr/lib")
    {
        #[rustfmt::skip]
            handle.args_i([
                "--ro-bind", "/usr/lib", "/usr/lib",
                "--ro-bind", "/usr/lib64", "/usr/lib64",
                "--symlink", "/usr/lib", "/lib",
                "--symlink", "/usr/lib64", "/lib64",
            ])?;
        return Ok(());
    }

    let cache = Arc::new(crate::shared::env::CACHE_DIR.join(".lib"));
    if !cache.exists() {
        user::run_as!(user::Mode::Effective, {
            fs::create_dir_all(cache.as_path()).unwrap()
        });
    }

    debug!("Creating SOF");
    let sof = sys_dir.join("sof");
    if !sof.exists() {
        fs::create_dir(&sof)?;
    }

    // Libraries needed by the program. No Binaries.
    let dependencies: Arc<DashSet<String, ahash::RandomState>> = Arc::default();

    // We use get_libraries to initialize the library roots, but need to pull one of the binaries
    // off to do it before the roots are needed.
    //
    // I cannot think of any situation in which profile.binaries will be None, or empty. Profiles are
    // either elf binaries themselves, or they use an interpreter that is. We fallback to the current
    // program in emergencies.
    if let Some(binaries) = &profile.binaries {
        debug_timer!("::binaries", {
            binaries.par_iter().for_each(|binary| {
                let dep = dependencies.clone();
                if let Ok(libraries) =
                    get_libraries(Cow::Owned(binary.clone()), Some(&cache.clone()))
                {
                    for library in libraries {
                        dep.insert(library);
                    }
                }
            })
        });
    };

    if !cache.starts_with(AT_HOME.as_path()) {
        let shared = cache.join("shared");
        if !shared.exists() {
            debug!("Creating shared directory at {shared:?}");
            fs::create_dir(&shared)?;
        }
    }

    // Libraries and Binaries, Wildcard and Directories Resolved
    let mut resolved = Set::new();

    // Directories to exclude and attach
    let directories = Arc::from(DashSet::new());

    for lib_root in LIB_ROOTS.get().unwrap().iter() {
        let app_lib = format!("{lib_root}/{name}");
        if Path::new(&app_lib).exists() {
            debug!("Adding program lib folder");
            resolved.insert(Cow::Owned(app_lib));
        }
    }

    debug_timer!("::wildcards", {
        if let Some(libraries) = profile.libraries.take() {
            // Separate the wildcards from the files/dirs.
            let (wildcards, flat): (Set<_>, Set<_>) =
                libraries.into_par_iter().partition(|e| e.contains('*'));

            debug!("Formatting flat");
            resolved.extend(flat.into_iter().filter_map(|e| {
                if e.starts_with("/") {
                    Some(Cow::Owned(e))
                } else if e.starts_with("~") {
                    Some(Cow::Owned(e.replace("~", HOME.as_str())))
                } else {
                    for root in LIB_ROOTS.get().unwrap().iter() {
                        let path = format!("{root}/{e}");
                        if Path::new(&path).exists() {
                            return Some(Cow::Owned(path));
                        }
                    }
                    warn!("Failed to find library: {e}");
                    None
                }
            }));

            debug!("Resolving wildcards");
            resolved.extend(
                wildcards
                    .into_par_iter()
                    .filter_map(|e| get_wildcards(&e, true, Some(&cache)).ok())
                    .collect::<Vec<_>>()
                    .into_iter()
                    .flatten()
                    .map(Cow::Owned)
                    .collect::<Set<_>>(),
            );
        }
    });

    let files = debug_timer!("::directories", {
        resolved
            .into_par_iter()
            .filter_map(|e| dir_resolve(e, directories.clone(), &cache).ok())
            .collect::<Vec<_>>()
            .into_iter()
            .flatten()
            .collect::<Set<_>>()
    });

    debug_timer!("::resolve", {
        files.into_par_iter().for_each(|file| {
            let dep = dependencies.clone();
            dep.insert(file.clone());
            if let Ok(libraries) = get_libraries(Cow::Owned(file), Some(&cache.clone())) {
                for library in libraries {
                    dep.insert(library);
                }
            }
        });
    });

    let dependencies = Arc::into_inner(dependencies).unwrap();

    debug_timer!("::writing", {
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

                if parent.contains("lib") && LIB_ROOTS.get().unwrap().iter().any(|r| parent == r) {
                    true
                } else {
                    !directories
                        .iter()
                        .any(|dir| parent.starts_with(dir.as_str()))
                }
            })
            // Write the SOF version, as a hard link preferably.
            .for_each(|lib| {
                if let Err(e) = add_sof(&sof, Cow::Borrowed(&lib), &cache) {
                    error!("Failed to add {lib} to SOF: {e}")
                }
            });
    });

    let sof_str = sof.to_string_lossy();
    handle.args_i(["--ro-bind-try", &format!("{sof_str}/lib"), "/usr/lib"])?;

    let path = &format!("{sof_str}/lib64");
    if Path::new(path).exists() {
        handle.args_i(["--ro-bind-try", path, "/usr/lib64"])?;
    } else {
        handle.args_i(["--symlink", "/usr/lib", "/usr/lib64"])?;
    }

    #[rustfmt::skip]
    handle.args_i([
        "--symlink", "/usr/lib", "/lib",
        "--symlink", "/usr/lib64", "/lib64",
    ])?;

    debug_timer!("::mount_directories", {
        directories.par_iter().try_for_each(|dir| -> Result<()> {
            if let Some(roots) = LIB_ROOTS.get() {
                if roots.iter().any(|r| dir.starts_with(r)) {
                    let sof_path = get_sof_path(&sof, dir.as_str());
                    if !sof_path.exists() {
                        fs::create_dir_all(sof_path)?;
                    }
                }
            } else {
                return Err(anyhow!("Roots not initalized!"));
            }

            handle.args_i([
                "--ro-bind",
                dir.as_str(),
                localize_home(dir.as_str()).as_ref(),
            ])?;
            Ok(())
        })?;
    });

    profile.libraries = Some(
        Arc::try_unwrap(directories)
            .map_err(|_| anyhow!("Deadlock collecting binary dependencies!"))?
            .into_iter()
            .collect(),
    );
    Ok(())
}
