//! Integrate a profile into the DE.
use crate::aux::{
    env::{DATA_HOME, HOME_PATH},
    profile::Profile,
};
use anyhow::{Context, Result};
use clap::ValueEnum;
use inflector::Inflector;
use log::{debug, warn};
use std::{borrow::Cow, fs::File, io::Write, os::unix::fs::symlink, path::Path};

#[derive(clap::Args, Debug)]
pub struct Args {
    /// The name of the profile
    pub profile: String,

    /// Undo integration for the profile.
    #[arg(short, long, default_value_t = false)]
    pub remove: bool,

    /// Some desktop environments, particularly Gnome, source
    /// their icons via the Flatpak ID (The Profile ID) in this case.
    /// This value must be in reverse DNS format, and Antimony automatically
    /// prepends "antimony." on those that don't. This presents an
    /// incongruity between ID and desktop that requires a shadow that
    /// hides the original. If an integrated profile lacks an icon, you
    /// may need to use this option.
    #[arg(short, long, default_value_t = false)]
    pub shadow: bool,

    /// How to integrate configurations
    #[arg(short, long)]
    pub config_mode: Option<ConfigMode>,
}

#[derive(Default, ValueEnum, Copy, Clone, Debug, PartialEq)]
pub enum ConfigMode {
    ///Integrate each configuration as a separate desktop action
    /// within the main Desktop File.
    #[default]
    Action,

    /// Separate each configuration into its own Desktop File. This
    /// can be useful, say, for setting configurations as default
    /// application handlers.
    File,
}

impl super::Run for Args {
    fn run(self) -> Result<()> {
        user::drop(user::Mode::Real)?;
        if self.remove { remove(self) } else { id(self) }
    }
}

/// Undo integration.
pub fn remove(cmd: Args) -> Result<()> {
    let profile = Profile::new(&cmd.profile)?;

    let name = &cmd.profile;

    if std::fs::remove_file(HOME_PATH.join(".local").join("bin").join(name)).is_err() {
        warn!("Binary does not exist");
    }

    let original = DATA_HOME
        .join("applications")
        .join(format!("{}.desktop", profile.desktop(name)));

    if std::fs::remove_file(&original).is_err() {
        warn!(
            "Original .desktop file ({}) does not exist. You may need to provide if it differs from the profile name",
            original.display()
        );
    }

    let name = if profile.id(name) != profile.desktop(name) && cmd.shadow {
        Cow::Owned(profile.id(name))
    } else {
        Cow::Borrowed(profile.desktop(name))
    };

    if let Some(configs) = &profile.configuration {
        for config in configs.keys() {
            let config_desktop = DATA_HOME
                .join("applications")
                .join(format!("{name}-{config}.desktop"));
            if config_desktop.exists() {
                std::fs::remove_file(config_desktop)?;
            }
        }
    }

    let copy = DATA_HOME
        .join("applications")
        .join(format!("{name}.desktop"));

    if std::fs::remove_file(&copy).is_err() {
        warn!("Profile .desktop file ({}) does not exist", copy.display());
    }

    Ok(())
}

/// Integrate a profile so it can be launched in place of the original in Desktop Environments.
pub fn id(cmd: Args) -> Result<()> {
    let profile = Profile::new(&cmd.profile)?;

    // Collect environment.
    let antimony = which::which("antimony")?;
    let name = &cmd.profile;

    // If ~/.local/bin is in PATH, the symlink takes precedence over
    // and thus applications will run in the sandbox unless the absolute
    // path in /usr/bin is given.
    debug!("Creating symlink in ~/.local/bin");
    let local = HOME_PATH.join(".local").join("bin");
    if !local.exists() {
        println!("Creating a local bin folder at ~/.local/bin. You may need to update your PATH if you want
            to launch sandboxed applications from the command line without the explicit path.");
        std::fs::create_dir_all(&local)?;
    }

    let local = local.join(name);
    if !local.exists() {
        symlink(antimony, &local).with_context(|| "Failed to create local symlink")?;
    }

    let local = local.to_string_lossy();

    let desktop_file = Path::new("/usr")
        .join("share")
        .join("applications")
        .join(format!("{}.desktop", profile.desktop(name)));

    if !desktop_file.exists() {
        warn!("The profile does not have a desktop file.");
        return Ok(());
    }

    let name = if profile.id(name) != profile.desktop(name) && cmd.shadow {
        // Create a shadow copy to turn on NoDisplay
        let local_desktop: Vec<String> = std::fs::read_to_string(&desktop_file)
            .with_context(|| "Failed to read desktop file")?
            .split("\n")
            .map(|e| {
                if e.contains("Name=") {
                    return e.to_string() + " (Native)\nNoDisplay=true";
                }
                e.to_string()
            })
            .collect();

        // Write it out.
        let shadow = DATA_HOME
            .join("applications")
            .join(format!("{}.desktop", profile.desktop(name)));

        if let Some(parent) = shadow.parent() {
            std::fs::create_dir_all(parent)?;
        }

        debug!("Writing shadow");
        write!(File::create(shadow)?, "{}", local_desktop.join("\n"))?;
        Cow::Owned(profile.id(name))
    } else {
        Cow::Borrowed(profile.desktop(name))
    };

    let desktop =
        std::fs::read_to_string(desktop_file).with_context(|| "Failed to read desktop file")?;

    // Make the new desktop file.
    debug!("Creating desktop file");

    let desktop_actions = desktop.contains("Actions=");
    let append_actions = |line: &mut String| {
        if let Some(configs) = &profile.configuration {
            for name in configs.keys() {
                line.push_str(&format!("{name};"));
            }
        }
    };

    let fix_exec = |name, line: &mut String, config: Option<&str>| {
        let args = match line.find(' ') {
            Some(index) => &line[index + 1..],
            None => " ",
        };

        match config {
            Some(config) => {
                *line = format!(
                    "{name}=antimony run {} --config {config} {args}",
                    cmd.profile
                )
                .trim()
                .to_string()
            }
            None => *line = format!("{name}={local} {args}").trim().to_string(),
        }
    };

    let mut contents: Vec<String> = desktop.lines().map(|e| e.to_string()).collect();
    for line in &mut contents {
        // Point to the symlink
        if line.starts_with("Exec=") {
            fix_exec("Exec", line, None);
            if !desktop_actions {
                line.push_str("\nActions=");
                append_actions(line);
            }
        }
        if line.starts_with("TryExec=") {
            line.clear();
        }

        // GTK applications subscribe to a single instance paradigm, where
        // a single instance will spawn new windows, rather than entirely new
        // instances of the app. However, this just causes D-Bus to launch the
        // application unconfined.
        if line.starts_with("DBusActivatable=") {
            *line = "DBusActivatable=false".to_string();
        }

        if cmd.config_mode.unwrap_or_default() == ConfigMode::Action && line.starts_with("Actions=")
        {
            append_actions(line);
        }
    }

    // Add configurations.
    if let Some(configs) = &profile.configuration {
        match cmd.config_mode.unwrap_or_default() {
            // Integrate each into one desktop file with actions.
            ConfigMode::Action => {
                contents.push("\n".into());
                for config in configs.keys() {
                    contents.push(format!(
                        "[Desktop Action {config}]\n\
                        Name=Antimony Configuration {}\n\
                        Exec=antimony run {} --config {config} %U \n",
                        config.to_title_case(),
                        cmd.profile
                    ));
                }
            }

            // Provide each configuration as a desktop file.
            ConfigMode::File => {
                for config in configs.keys() {
                    let config_desktop = DATA_HOME
                        .join("applications")
                        .join(format!("{name}-{config}.desktop"));

                    let mut contents = contents.clone();
                    for line in &mut contents {
                        // Point to the symlink
                        if line.starts_with("Exec=") {
                            fix_exec("Exec", line, Some(config));
                        }
                        if line.starts_with("TryExec=") {
                            fix_exec("TryExec", line, Some(config));
                        }
                        if line.starts_with("Name=") {
                            line.push_str(&format!(" ({})", config.to_title_case()));
                        }
                    }

                    write!(File::create(config_desktop)?, "{}", contents.join("\n"))?;
                }
            }
        }
    }

    let antimony_desktop = DATA_HOME
        .join("applications")
        .join(format!("{name}.desktop"));

    write!(
        File::create(antimony_desktop).with_context(|| "Failed to create new desktop file")?,
        "{}",
        contents.join("\n")
    )
    .with_context(|| "Failed to write new desktop file")?;
    Ok(())
}
