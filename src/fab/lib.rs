use crate::{
    fab::{bin::ELF_MAGIC, localize_home},
    shared::{
        env::{AT_HOME, HOME},
        profile::Profile,
    },
};
use anyhow::{Error, Result, anyhow};
use dashmap::DashSet;
use log::{debug, error, trace, warn};
use once_cell::sync::Lazy;
use rayon::prelude::*;
use spawn::{Spawner, StreamMode};
use std::{
    borrow::Cow,
    collections::HashSet,
    fs::{self, File},
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

/// Where to store cache data.
static CACHE_DIR: Lazy<PathBuf> = Lazy::new(|| {
    let path = crate::shared::env::CACHE_DIR.join(".lib");
    user::run_as!(user::Mode::Effective, fs::create_dir_all(&path).unwrap());
    path
});

/// Whether we have a split-root.
pub static SINGLE_LIB: Lazy<bool> = Lazy::new(|| {
    let single = match fs::read_link("/usr/lib64") {
        Ok(dest) => dest == Path::new("/usr/lib") || dest == Path::new("lib"),
        Err(_) => false,
    };
    debug!("Single Library Folder: {single}");
    single
});

// Get the library roots.
pub static LIB_ROOTS: Lazy<HashSet<String>> = Lazy::new(|| {
    let mut roots: HashSet<String> = Spawner::new("find")
        .args(["/usr/lib", "-maxdepth", "2", "-name", "libsqlite3.so"])
        .expect("Failed to setup process")
        .output(StreamMode::Pipe)
        .spawn()
        .expect("Failed to spawn process")
        .output_all()
        .expect("Failed to get sqlite path")
        .lines()
        .filter_map(|path| {
            PathBuf::from(&path[..path.len() - 1])
                .parent()
                .map(|path| path.to_string_lossy().into_owned())
        })
        .collect();
    debug!("Library root at: {roots:?}");

    roots.insert(String::from("/usr/lib"));
    if !*SINGLE_LIB {
        roots.insert(String::from("/usr/lib64"));
    }
    roots
});

/// Get cached definitions.
pub fn get_cache(name: &str) -> Result<Option<HashSet<String>>> {
    let cache_file = CACHE_DIR.join(name.replace("/", ".").replace("*", "."));
    if let Ok(file) = File::open(&cache_file) {
        let reader = BufReader::new(file);
        return Ok(Some(reader.lines().map_while(|e| e.ok()).collect()));
    }
    Ok(None)
}

/// Write the cache file.
pub fn write_cache(name: &str, libraries: HashSet<String>) -> Result<HashSet<String>> {
    user::sync::try_run_as!(user::Mode::Effective, {
        let cache_file = CACHE_DIR.join(name.replace("/", ".").replace("*", "."));
        let mut file = File::create(&cache_file)?;
        for library in &libraries {
            writeln!(file, "{library}")?;
        }
        Ok(libraries)
    })
}

/// LDD a path.
pub fn get_libraries(path: Cow<'_, str>) -> Result<HashSet<String>> {
    if let Some(libraries) = get_cache(&path)? {
        return Ok(libraries);
    }

    let libraries: HashSet<String> = Spawner::new("/usr/bin/ldd")
        .arg(path.as_ref())?
        .output(StreamMode::Pipe)
        .error(StreamMode::Discard)
        .mode(user::Mode::Real)
        .spawn()?
        .output_all()?
        .split_whitespace()
        .filter(|s| s.contains('/'))
        .filter_map(|e| {
            let mut resolved = e.to_string();
            if e.contains("..") {
                resolved = fs::canonicalize(e)
                    .map_err(Error::from)
                    .ok()?
                    .to_string_lossy()
                    .into_owned()
            }
            if resolved.starts_with("/lib") {
                resolved.insert_str(0, "/usr");
            }

            if *SINGLE_LIB {
                resolved = resolved.replace("/usr/lib64/", "/usr/lib/");
            }

            Some(resolved)
        })
        .collect();
    write_cache(&path, libraries)
}

/// Get all matches for a wildcard.
pub fn get_wildcards(pattern: &str, lib: bool) -> Result<HashSet<String>> {
    let run = |dir: &str, base: &str| -> Result<HashSet<String>> {
        #[rustfmt::skip]
        let out = Spawner::new("find")
            .args([
                dir,
                "-maxdepth", "1",
                "-mindepth", "1",
                "-name", base,
            ])?
            .output(StreamMode::Pipe)
            .mode(user::Mode::Real)
            .spawn()?
            .output_all()?
            .lines()
            .map(|e| e.to_string())
            .collect();
        Ok(out)
    };

    if let Some(libraries) = get_cache(pattern)? {
        return Ok(libraries);
    }
    let libraries = if pattern.starts_with("/") {
        let i = pattern.rfind('/').unwrap();
        run(&pattern[..i], &pattern[i + 1..])?
    } else if lib {
        let mut libraries = HashSet::new();
        for root in LIB_ROOTS.iter() {
            libraries.extend(run(root, pattern)?);
        }

        libraries
    } else {
        run("/usr/bin", pattern)?
    };

    write_cache(pattern, libraries)
}

/// Filter non-elf files.
fn elf_filter(path: &str) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut magic = [0u8; 5];
    file.read_exact(&mut magic).ok()?;
    if magic != ELF_MAGIC {
        return None;
    }
    Some(path.to_string())
}

/// Get all executable files in a directory.
pub fn get_dir(dir: &str) -> Result<HashSet<String>> {
    if let Some(libraries) = get_cache(dir)? {
        return Ok(libraries);
    }

    let libraries: HashSet<String> = Spawner::new("/usr/bin/find")
        .args([dir, "-executable", "-type", "f"])?
        .output(StreamMode::Pipe)
        .mode(user::Mode::Real)
        .spawn()?
        .output_all()?
        .lines()
        .filter_map(elf_filter)
        .collect();
    write_cache(dir, libraries)
}

/// Determine dependencies for directories.
fn dir_resolve(
    library: Cow<'_, str>,
    directories: Arc<DashSet<String>>,
) -> Result<HashSet<String>> {
    let mut dependencies = HashSet::new();
    let path = Path::new(library.as_ref());

    // Resolve directories.
    if path.is_dir() {
        dependencies.extend(get_dir(&library)?);
        directories.insert(library.to_string());
    } else if let Some(library) = elf_filter(&library) {
        dependencies.insert(library);
    } else {
        warn!("Invalid file: {path:?}");
    }
    Ok(dependencies)
}

pub fn get_sof_path(sof: &Path, library: &str) -> PathBuf {
    PathBuf::from(library.replace("/usr", &sof.to_string_lossy()))
}

pub fn add_sof(sof: &Path, library: Cow<'_, str>) -> Result<()> {
    let sof_path = get_sof_path(sof, &library);

    if let Some(parent) = sof_path.parent() {
        fs::create_dir_all(parent)?;
        let path = PathBuf::from(library.as_ref());
        let canon = fs::canonicalize(&path)?;

        trace!("Creating SOF file: {canon:?} => {sof_path:?}");
        if let Err(e) = fs::hard_link(&canon, &sof_path)
            && e.kind() != io::ErrorKind::AlreadyExists
        {
            // If we cannot hard-link directly, then we created a shared source
            // of library copies within the CACHE_DIR, then hard-link from that.
            //
            // This reduces redundancy between profiles, and since shared exists
            // in the CACHE_DIR, hard links will work.

            let shared_path = get_sof_path(&CACHE_DIR.join("shared"), &library);
            if let Some(parent) = shared_path.parent() {
                fs::create_dir_all(parent)?;

                trace!("Creating shared copy via {canon:?} => {shared_path:?} => {sof_path:?}");
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

    debug!("Creating SOF");
    let sof = sys_dir.join("sof");
    fs::create_dir_all(&sof)?;

    if !CACHE_DIR.starts_with(AT_HOME.as_path()) {
        let shared = CACHE_DIR.join("shared");
        if !shared.exists() {
            debug!("Creating shared directory at {shared:?}");
            fs::create_dir_all(&shared)?;
        }
    }

    // Libraries needed by the program. No Binaries.
    let mut dependencies = HashSet::new();

    // Libraries and Binaries, Wildcard and Directories Resolved
    let mut resolved = HashSet::new();

    // Directories to exclude and attach
    let directories = Arc::from(DashSet::new());

    for lib_root in LIB_ROOTS.iter() {
        let app_lib = format!("{lib_root}/{name}");
        if Path::new(&app_lib).exists() {
            debug!("Adding program lib folder");
            resolved.insert(Cow::Owned(app_lib));
        }
    }

    if let Some(libraries) = profile.libraries.take() {
        // Separate the wildcards from the files/dirs.
        let (wildcards, flat): (HashSet<_>, HashSet<_>) =
            libraries.into_par_iter().partition(|e| e.contains('*'));

        resolved.extend(flat.into_iter().filter_map(|e| {
            if e.starts_with("/") {
                Some(Cow::Owned(e))
            } else if e.starts_with("~") {
                Some(Cow::Owned(e.replace("~", HOME.as_str())))
            } else {
                for root in LIB_ROOTS.iter() {
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
                .filter_map(|e| get_wildcards(&e, true).ok())
                .collect::<Vec<_>>()
                .into_iter()
                .flatten()
                .map(Cow::Owned)
                .collect::<HashSet<_>>(),
        );
    }

    debug!("Resolving directories");
    let files = resolved
        .into_par_iter()
        .filter_map(|e| dir_resolve(e, directories.clone()).ok())
        .collect::<Vec<_>>()
        .into_iter()
        .flatten()
        .collect::<HashSet<_>>();

    // The files themselves are direct dependencies.
    dependencies.extend(files.clone());

    debug!("Resolving libraries");
    dependencies.extend(
        files
            .into_par_iter()
            .filter_map(|e| get_libraries(Cow::Owned(e)).ok())
            .collect::<Vec<_>>()
            .into_iter()
            .flatten()
            .collect::<HashSet<_>>(),
    );

    // Grab the binaries; they are still needed for SECCOMP, however.
    if let Some(binaries) = &profile.binaries {
        debug!("Resolving binaries");
        dependencies.extend(
            binaries
                .into_par_iter()
                .filter_map(|b| get_libraries(Cow::Borrowed(b)).ok())
                .flatten()
                .collect::<HashSet<_>>(),
        );
    }

    debug!("Writing libraries");

    dependencies
        .into_par_iter()
        // Filter things that aren't in /usr/lib
        .filter(|library| {
            let parent = if let Some(i) = library.rfind('/') {
                &library[..i]
            } else {
                library.as_str()
            };

            if LIB_ROOTS.iter().any(|r| parent == r) {
                true
            } else {
                !directories
                    .iter()
                    .any(|dir| parent.starts_with(dir.as_str()))
            }
        })
        // Write the SOF version, as a hard link preferably.
        .for_each(|lib| {
            if let Err(e) = add_sof(&sof, Cow::Borrowed(&lib)) {
                error!("Failed to add {lib} to SOF: {e}")
            }
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

    directories.par_iter().try_for_each(|dir| -> Result<()> {
        if LIB_ROOTS.iter().any(|r| dir.starts_with(r)) {
            let sof_path = get_sof_path(&sof, dir.as_str());
            if !sof_path.exists() {
                fs::create_dir_all(sof_path)?;
            }
        }
        handle.args_i([
            "--ro-bind",
            dir.as_str(),
            localize_home(dir.as_str()).as_ref(),
        ])?;
        Ok(())
    })?;

    profile.libraries = Some(
        Arc::try_unwrap(directories)
            .map_err(|_| anyhow!("Deadlock collecting binary dependencies!"))?
            .into_iter()
            .collect(),
    );
    Ok(())
}
