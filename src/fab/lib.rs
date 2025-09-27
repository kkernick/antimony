use crate::{
    aux::{
        env::{AT_HOME, SINGLE_LIB},
        profile::Profile,
    },
    fab::bin::ELF_MAGIC,
};
use anyhow::{Context, Error, Result};
use dashmap::DashSet;
use log::debug;
use once_cell::sync::Lazy;
use rayon::prelude::*;
use spawn::Spawner;
use std::{
    borrow::Cow,
    collections::HashSet,
    fs::File,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

/// Where to store cache data.
static CACHE_DIR: Lazy<PathBuf> = Lazy::new(|| {
    let path = PathBuf::from(AT_HOME.as_path()).join("cache").join(".lib");
    std::fs::create_dir_all(&path).unwrap();
    path
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
    let cache_file = CACHE_DIR.join(name.replace("/", ".").replace("*", "."));
    let mut file = File::create(&cache_file)?;
    for library in &libraries {
        writeln!(file, "{library}")?;
    }
    Ok(libraries)
}

/// LDD a path.
pub fn get_libraries(path: Cow<'_, str>) -> Result<HashSet<String>> {
    if let Some(libraries) = get_cache(&path)? {
        return Ok(libraries);
    }
    let libraries: HashSet<String> = Spawner::new("/usr/bin/ldd")
        .arg(path.as_ref())?
        .output(true)
        .spawn()?
        .output_all()?
        .split_whitespace()
        .filter(|s| s.contains('/'))
        .filter_map(|e| {
            let mut resolved = e.to_string();
            if e.contains("..") {
                resolved = std::fs::canonicalize(e)
                    .map_err(Error::from)
                    .ok()?
                    .to_string_lossy()
                    .into_owned()
            }
            if resolved.starts_with("/lib") {
                resolved.insert_str(0, "/usr");
            }
            Some(resolved)
        })
        .collect();
    write_cache(&path, libraries)
}

/// Get all matches for a wildcard.
pub fn get_wildcards(pattern: &str) -> Result<HashSet<String>> {
    let run = |dir, base| -> Result<HashSet<String>> {
        Ok(Spawner::new("find")
            .args([
                dir,
                "-maxdepth",
                "1",
                "-mindepth",
                "1",
                "-name",
                base,
                "-executable",
            ])?
            .output(true)
            .mode(user::Mode::Real)
            .spawn()?
            .output_all()?
            .lines()
            .map(|e| e.to_string())
            .collect())
    };

    if let Some(libraries) = get_cache(pattern)? {
        return Ok(libraries);
    }
    let libraries = if pattern.starts_with("/") {
        let i = pattern.rfind('/').unwrap();
        run(&pattern[..i], &pattern[i + 1..])?
    } else {
        let mut libraries = run("/usr/lib", pattern)?;
        if !*SINGLE_LIB {
            libraries.extend(run("/usr/lib64", pattern).unwrap_or_default());
        }
        libraries
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
        .output(true)
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
    }
    Ok(dependencies)
}

pub fn sof_path(sof: &Path, library: &str) -> PathBuf {
    PathBuf::from(library.replace("/usr", &sof.to_string_lossy()))
}

pub fn add_sof(sof: &Path, library: String) -> Result<()> {
    let sof_path = sof_path(sof, &library);

    if let Some(parent) = sof_path.parent() {
        std::fs::create_dir_all(parent)?;
        if let Ok(canon) = std::fs::canonicalize(&library) {
            if let Err(e) = std::fs::hard_link(&canon, &sof_path) {
                if e.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(e).with_context(|| {
                        format!("Failed to hardlink {library} -> {}", sof_path.display())
                    });
                }
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
    let saved = user::save()?;

    if let Some(libraries) = &profile.libraries {
        if libraries.contains(&"/usr/lib".to_string()) {
            #[rustfmt::skip]
            handle.args_i([
                "--ro-bind", "/usr/lib", "/usr/lib",
                "--ro-bind", "/usr/lib64", "/usr/lib64",
                "--symlink", "/usr/lib", "/lib",
                "--symlink", "/usr/lib64", "/lib64",
            ])?;
            return Ok(());
        }
    }

    debug!("Creating SOF");
    let sof = sys_dir.join("sof");
    std::fs::create_dir_all(&sof)?;

    // Libraries needed by the program. No Binaries.
    let mut dependencies = HashSet::new();

    // Libraries and Binaries, Wildcard and Directories Resolved
    let mut resolved = HashSet::new();

    // Directories to exclude and attach
    let directories = Arc::from(DashSet::new());

    let mut lib_roots = vec!["lib"];
    if !*SINGLE_LIB {
        lib_roots.push("lib64");
    }

    for lib_root in lib_roots {
        let app_lib = Path::new("/usr").join(lib_root).join(name);
        if app_lib.exists() {
            debug!("Adding program lib folder");
            resolved.insert(Cow::Owned(app_lib.to_string_lossy().into_owned()));
        }
    }

    if let Some(libraries) = profile.libraries.take() {
        // Separate the wildcards from the files/dirs.
        let (wildcards, flat): (HashSet<_>, HashSet<_>) =
            libraries.into_par_iter().partition(|e| e.contains('*'));

        resolved.extend(flat.into_iter().map(Cow::Owned));

        debug!("Resolving wildcards");
        resolved.extend(
            wildcards
                .into_par_iter()
                .filter_map(|e| get_wildcards(&e).ok())
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
                .filter_map(|b| {
                    if !b.contains("lib") {
                        handle.args_i(["--ro-bind", b, b]).ok();
                    }
                    get_libraries(Cow::Borrowed(b)).ok()
                })
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

            if parent == "/usr/lib" || parent == "/usr/lib64" {
                true
            } else {
                !directories
                    .iter()
                    .any(|dir| parent.starts_with(dir.as_str()))
            }
        })
        // Write the SOF version, as a hard link preferably.
        .try_for_each(|lib| add_sof(&sof, lib))?;

    // Overlays are required to mount the library directories.

    let sof_str = sof.to_string_lossy();
    #[rustfmt::skip]
    handle.args_i([
        "--ro-bind-try", &format!("{sof_str}/lib"), "/usr/lib",
        "--ro-bind-try", &format!("{sof_str}/lib64"), "/usr/lib64",
        "--symlink", "/usr/lib", "/lib",
        "--symlink", "/usr/lib64", "/lib64",
    ])?;

    directories.iter().try_for_each(|dir| -> Result<()> {
        if dir.contains("lib") {
            let sof_path = sof_path(&sof, dir.as_str());
            if !sof_path.exists() {
                std::fs::create_dir_all(sof_path)?;
            }
        }
        handle.args_i(["--ro-bind", dir.as_str(), dir.as_str()])?;
        Ok(())
    })?;

    user::restore(saved)?;
    Ok(())
}
