use ahash::HashSetExt;
use antimony::{
    fab::{get_libraries, get_wildcards, resolve},
    shared::{Set, env::AT_HOME, feature::Feature, profile::FileMode, utility},
};
use dashmap::DashMap;
use rayon::prelude::*;
use std::{
    borrow::Cow,
    fs,
    io::{self, Write, stdin},
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    thread,
};

fn main() -> anyhow::Result<()> {
    let term = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term))?;

    let mut err = Vec::<String>::new();
    let stdin = stdin();

    // We need to populate the library roots ;)
    let root_thread = thread::spawn(|| get_libraries(Cow::Owned(utility("tracer"))));

    loop {
        let mut line = String::new();
        match stdin.read_line(&mut line)? {
            0 => break,
            _ => {
                print!("{line}");
                err.push(line);
            }
        }
    }

    root_thread
        .join()
        .map_err(|_| anyhow::anyhow!("Failed to join root thread!"))??;

    println!("Generating Report...");
    // Get the files.
    let not_found: Set<String> = err
        .par_iter()
        .filter(|e| e.contains("ENOENT"))
        .map(|e| {
            let l = e.find('"').unwrap_or(0);
            let r = e.rfind('"').unwrap_or(e.len());
            e[l + 1..r].trim().to_string()
        })
        .filter(|e| Path::new(e).exists())
        .collect();

    if !not_found.is_empty() {
        println!("Generating Report...");

        // Get all features on the system.
        let feature_database: DashMap<String, Feature> = DashMap::new();
        let feature_dir = Path::new(AT_HOME.as_path()).join("features");
        for path in fs::read_dir(feature_dir)?.filter_map(|e| e.ok()) {
            feature_database.insert(
                path.file_name().to_string_lossy().into_owned(),
                toml::from_str(&fs::read_to_string(path.path())?)?,
            );
        }

        let arc = Arc::new(feature_database);

        println!("============== FILES ==============");
        not_found.into_par_iter().try_for_each(|file| {
            let database = arc.clone();

            let mut features = Set::<(String, String, FileMode)>::new();

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
                    } else if d_name.contains("*") {
                        match get_wildcards(&d_name, true) {
                            Ok(cards) => cards.contains(file),
                            Err(_) => false,
                        }
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
                        for d_name in libraries {
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
                        file = file[..i].to_string();
                    }
                }
            });

            let io = io::stdout();
            let mut out = io.lock();
            if !features.is_empty() {
                writeln!(out, "{file} can be provided with the following features")?;
                for (feature, path, mode) in features {
                    println!("\t- {feature} (via {path}) as {mode}");
                }
            } else {
                writeln!(out, "{file}")?;
            }
            Ok::<(), anyhow::Error>(())
        })?;
    } else {
        println!("Nothing to report!");
    }

    Ok(())
}
