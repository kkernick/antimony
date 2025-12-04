//! Run the sandbox under strace to locate missing files.
use crate::{
    fab::{lib::get_wildcards, resolve},
    setup::setup,
    shared::{env::AT_HOME, feature::Feature, profile::FileMode},
};
use anyhow::{Result, anyhow};
use clap::ValueEnum;
use dashmap::DashMap;
use rayon::prelude::*;
use std::{
    borrow::Cow,
    collections::HashSet,
    fs,
    io::{self, Write},
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

/// The mode to run strace under
#[derive(Debug, Clone, ValueEnum, Default)]
pub enum Mode {
    /// Only trace syscalls that return errors.
    #[default]
    Errors,

    /// Trace all syscalls, even those that succeed.
    /// Useful to see the context in which an error occurred.
    All,
}

#[derive(clap::Args, Debug, Default)]
pub struct Args {
    /// The name of the profile
    pub profile: String,

    /// What to trace.
    pub mode: Mode,

    /// Collect the trace log and list files that the sandbox tried to access,
    /// and feature they are available in.
    #[arg(short, long, default_value_t = false)]
    pub report: bool,

    /// Use a configuration within the profile.
    #[arg(short, long)]
    pub config: Option<String>,

    /// Arguments to pass to strace directly
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub trace_args: Option<Vec<String>>,

    /// Arguments to pass to the sandbox application.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub passthrough: Option<Vec<String>>,
}

impl super::Run for Args {
    fn run(mut self) -> Result<()> {
        let mut args = super::run::Args {
            binaries: Some(vec!["strace".to_string()]),
            config: self.config.clone(),
            passthrough: self.passthrough.take(),
            ..Default::default()
        };

        match setup(Cow::Borrowed(&self.profile), &mut args) {
            Ok(info) => trace(info, self),
            Err(e) => Err(anyhow!("Failed to run profile: {e}")),
        }
    }
}

pub fn trace(info: crate::setup::Info, mut args: Args) -> Result<()> {
    let mut err = Vec::<String>::new();

    // Ignore SIGINT.
    let term = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&term))?;

    let handle = info.handle.args([
        "strace",
        match args.mode {
            Mode::Errors => "-ffyYZ",
            Mode::All => "-ffyY",
        },
        "-v",
        "-s",
        "256",
    ])?;

    if let Some(args) = args.trace_args.take() {
        handle.args_i(args)?;
    };

    let mut handle = handle
        .arg(info.profile.app_path(&info.name))?
        .args(info.post)?
        .error(true)
        .spawn()?;

    let error = handle.error()?;
    while let Some(line) = error.read_line() {
        print!("{line}");
        if args.report {
            err.push(line)
        }
    }

    // Reporting collects all the files that were inaccessible,
    // and offers features that can provide them.
    if args.report {
        // Get the files.
        let not_found: HashSet<String> = err
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

                let mut features = HashSet::<(String, String, FileMode)>::new();

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

                    let mut matches =
                        |mode: &FileMode, d_name: &String, file: &String| -> Option<()> {
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
                            if let Some(direct) = &files.direct {
                                for (mode, entry) in direct {
                                    for d_name in entry.keys() {
                                        if matches(mode, d_name, &file).is_some() {
                                            break 'feature_loop;
                                        }
                                    }
                                }
                            }
                            if let Some(user) = &files.user {
                                for (mode, entry) in user {
                                    for d_name in entry {
                                        if matches(mode, d_name, &file).is_some() {
                                            break 'feature_loop;
                                        }
                                    }
                                }
                            }
                            if let Some(system) = &files.platform {
                                for (mode, entry) in system {
                                    for d_name in entry {
                                        if matches(mode, d_name, &file).is_some() {
                                            break 'feature_loop;
                                        }
                                    }
                                }
                            }
                            if let Some(system) = &files.resources {
                                for (mode, entry) in system {
                                    for d_name in entry {
                                        if matches(mode, d_name, &file).is_some() {
                                            break 'feature_loop;
                                        }
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
                        println!("\t- {feature} (via {path}) as {mode:?}");
                    }
                } else {
                    writeln!(out, "{file}")?;
                }
                Ok::<(), anyhow::Error>(())
            })?;
        }
    }
    crate::setup::cleanup(info.instance)
}
