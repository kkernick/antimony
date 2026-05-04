#![allow(unused_crate_dependencies)]

use antimony::{
    fab::{get_libraries, get_wildcards, lib::WildcardFilter, resolve},
    shared::{
        Set, ThreadMap,
        feature::Feature,
        profile::files::FileMode,
        store::{Object, SYSTEM_STORE, USER_STORE},
        utility,
    },
};
use rayon::prelude::*;
use signal_hook::{consts, flag};
use std::{
    borrow::Cow,
    io::{self, Write, stdin},
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

#[allow(clippy::too_many_lines)]
fn main() -> anyhow::Result<()> {
    let term = Arc::new(AtomicBool::new(false));
    flag::register(consts::SIGINT, Arc::clone(&term))?;

    let mut err = Vec::new();
    let stdin = stdin();

    // We need to populate the library roots ;)
    get_libraries(&utility("tracer"))?;

    loop {
        let mut line = String::new();
        match stdin.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                print!("{line}");
                err.push(line);
            }
        }
    }

    println!("Generating Report...");
    // Get the files.
    let not_found: Set<String> = err
        .par_iter()
        .filter(|e| e.contains("ENOENT"))
        .filter_map(|e| {
            let l = e.find('"').unwrap_or(0);
            let r = e.rfind('"').unwrap_or(e.len());
            l.checked_add(1).map(|i| e[i..r].trim().to_owned())
        })
        .filter(|e| {
            let path = Path::new(e);
            path.exists() && !e.starts_with("/home/antimony")
        })
        .collect();

    if not_found.is_empty() {
        println!("Nothing to report!");
    } else {
        println!("Generating Report...");

        // Get all features on the system.
        let feature_database: ThreadMap<String, Feature> = ThreadMap::default();

        if let Ok(features) = SYSTEM_STORE.borrow().get(Object::Feature) {
            for name in features {
                let feature = SYSTEM_STORE.borrow().fetch(&name, Object::Feature)?;
                feature_database.insert(name, toml::from_str(&feature)?);
            }
        }

        // Replace user override
        if let Ok(features) = USER_STORE.borrow().get(Object::Feature) {
            for name in features {
                let feature = USER_STORE.borrow().fetch(&name, Object::Feature)?;
                feature_database.insert(name, toml::from_str(&feature)?);
            }
        }

        let arc = Arc::new(feature_database);

        println!("============== FILES ==============");
        not_found.into_par_iter().try_for_each(|file| {
            let database = Arc::clone(&arc);
            let mut features = Set::default();

            // For each file, try and see if any part of the file path
            // is provided:
            //
            // For example, /usr/lib/mylib would check:
            //  1. /usr/lib/mylib
            //  2. /usr/lib
            //  3. /usr
            database.iter().for_each(|pair| {
                let name = pair.key();
                let feature = pair.value();
                let mut file = file.clone();

                let mut matches = |mode: &FileMode, d_name: &String, file: &String| -> Option<()> {
                    let d_name = resolve(Cow::Borrowed(d_name));

                    let found = if file.is_empty() {
                        false
                    } else if d_name.contains('*') {
                        get_wildcards(&d_name, true, WildcardFilter::Files)
                            .unwrap_or_default()
                            .contains(file.as_str())
                    } else {
                        *d_name == *file
                    };

                    if found {
                        features.insert((name.clone(), d_name.into_owned(), *mode));
                        Some(())
                    } else {
                        None
                    }
                };

                // Digest the path member by member, checking if any relevant
                // field within the feature matches.
                'feature_loop: loop {
                    if file.is_empty() {
                        break;
                    }

                    if let Some(files) = &feature.files {
                        let direct = &files.direct;
                        for (mode, entry) in direct {
                            for d_name in entry.keys() {
                                if matches(mode, d_name, &file).is_some() {
                                    break 'feature_loop;
                                }
                            }
                        }

                        let user = &files.user;
                        for (mode, entry) in user {
                            for d_name in entry {
                                if matches(mode, d_name, &file).is_some() {
                                    break 'feature_loop;
                                }
                            }
                        }

                        let system = &files.platform;
                        for (mode, entry) in system {
                            for d_name in entry {
                                if matches(mode, d_name, &file).is_some() {
                                    break 'feature_loop;
                                }
                            }
                        }

                        let system = &files.resources;
                        for (mode, entry) in system {
                            for d_name in entry {
                                if matches(mode, d_name, &file).is_some() {
                                    break 'feature_loop;
                                }
                            }
                        }
                    }
                    if let Some(binaries) = &feature.binaries {
                        for d_name in binaries {
                            if matches(&FileMode::Executable, d_name, &file).is_some() {
                                break 'feature_loop;
                            }
                        }
                    }
                    if let Some(libraries) = &feature.libraries {
                        for d_name in &libraries.files {
                            if matches(&FileMode::Executable, d_name, &file).is_some() {
                                break 'feature_loop;
                            }
                        }
                        for d_name in &libraries.directories {
                            if matches(&FileMode::Executable, d_name, &file).is_some() {
                                break 'feature_loop;
                            }
                        }
                    }

                    if let Some(devices) = &feature.devices {
                        for d_name in devices {
                            if matches(&FileMode::ReadWrite, d_name, &file).is_some() {
                                break 'feature_loop;
                            }
                        }
                    }

                    if let Some(i) = file.rfind('/') {
                        file = file[..i].to_owned();
                    }
                }
            });

            let io = io::stdout();
            let mut out = io.lock();
            if features.is_empty() {
                writeln!(out, "{file}")?;
            } else {
                writeln!(out, "{file} can be provided with the following features")?;
                for (feature, path, mode) in features {
                    println!("\t- {feature} (via {path}) as {mode}");
                }
            }
            Ok::<(), anyhow::Error>(())
        })?;
    }

    Ok(())
}
