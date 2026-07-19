//! This application ingests strace output and produces a list of missing files
//! and features that would provide it. It's used with the `trace` feature.
//!
//! You can also use it standalone (Quite useful if you're running into issues with
//! antimony itself) via `strace -ffYy program | antimony-tracer`
#![allow(unused_crate_dependencies)]

use antimony::{
    fab::resolve,
    shared::{
        Set, ThreadMap,
        feature::Feature,
        find::{self, WildcardFilter},
        profile::files::FileMode,
        store::{Object, SYSTEM_STORE, USER_STORE},
    },
};
use dialoguer::console::style;
use rayon::prelude::*;
use signal_hook::{consts, flag};
use std::{
    borrow::Cow,
    io::{self, Write, stdin},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[allow(clippy::too_many_lines)]
fn main() -> anyhow::Result<()> {
    let term = Arc::new(AtomicBool::new(false));
    flag::register(consts::SIGINT, Arc::clone(&term))?;
    flag::register(consts::SIGTERM, Arc::clone(&term))?;

    let mut err = Vec::new();
    let stdin = stdin();
    while !term.load(Ordering::Relaxed) {
        let mut line = String::new();
        match stdin.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                print!("{line}");
                err.push(line);
            }
        }
    }

    let extract = |line: &str, delim_l: char, delim_r: char| -> Option<String> {
        if let Some(l) = line.find(delim_l)
            && let Some(l) = l.checked_add(1)
            && let Some(r) = line.rfind(delim_r)
            && l != r
        {
            Some(line[l..r].trim().to_owned())
        } else {
            None
        }
    };

    println!("Generating Report...");
    // Get the files.
    let not_found: Set<String> = err
        .par_iter()
        .enumerate()
        .filter(|(_, e)| e.contains("ENOENT"))
        .filter_map(|(i, e)| {
            // Try and extract the path
            extract(e, '"', '"').map_or_else(
                // If there is no path, grab the PID and name.
                || {
                    extract(e, '[', ']').and_then(|pid| {
                        // Collect the last 100 lines to find the path
                        let lines: Vec<_> = err[i.saturating_sub(100)..i]
                            .par_iter()
                            .filter(|line| line.contains(&pid))
                            .collect();

                        let mut path = None;
                        // Iterate from our current line
                        for line in lines.into_iter().rev() {
                            // If we found the path associate with the error, break
                            path = extract(line, '"', '"');
                            if path.is_some() {
                                break;
                            }
                        }
                        path
                    })
                },
                Some,
            )
        })
        .filter(|e| {
            let path = Path::new(e);
            path.exists() && !e.starts_with("/home/antimony") && !e.starts_with("/proc")
        })
        .collect();

    // Set removes duplicates, but we also want to have the output neatly organized,
    // so unwrap into a Vector and sort it.
    let mut not_found = not_found.into_iter().collect::<Vec<_>>();
    not_found.sort();

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

        println!("{}", style("============== FILES ==============").bold());
        not_found.into_par_iter().try_for_each(|file| {
            let database = Arc::clone(&arc);
            let mut features = Set::default();

            // For each file, try and see if any part of the filepath
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
                        find::wildcards(d_name.as_ref(), true, WildcardFilter::Files)
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
                writeln!(out, "{}", style(file).italic().bold().magenta())?;
            } else {
                writeln!(
                    out,
                    "{} can be provided with the following features",
                    style(file).italic()
                )?;
                for (feature, path, mode) in features {
                    let styled = match mode {
                        FileMode::ReadOnly => style(mode).green(),
                        FileMode::ReadWrite => style(mode).yellow(),
                        FileMode::Executable => style(mode).red(),
                    };
                    println!("\t- {} (via {path}) as {styled}", style(feature).bold());
                }
            }
            Ok::<(), anyhow::Error>(())
        })?;
    }

    Ok(())
}
