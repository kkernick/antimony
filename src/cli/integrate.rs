//! Integrate a profile into the DE.

use crate::{
    cli::{self, Cli, Command},
    shared::{
        env::{CONFIG_HOME, DATA_HOME, HOME_PATH, SESSION_BUS},
        profile::Profile,
    },
};
use anyhow::{Context, Result, anyhow};
use clap::{Parser, ValueEnum, ValueHint};
use heck::ToTitleCase;
use log::info;
use spawn::Spawner;
use std::{
    fmt::Write as FormatWrite,
    fs::{self, File},
    io::Write as IoWrite,
    os::unix::fs::symlink,
    path::Path,
};
use user::{Mode, USER};

/// Create integrate arguments from subcommand passthrough.
#[allow(clippy::unreachable)]
pub fn integrate_vec(profile: &str, mut passthrough: Vec<String>) -> self::Args {
    let mut command: Vec<String> = vec!["antimony", "integrate", profile]
        .into_iter()
        .map(String::from)
        .collect();

    command.append(&mut passthrough);
    let cli = Cli::parse_from(command);
    match cli.command {
        Command::Integrate(args) => args,
        _ => unreachable!(),
    }
}

#[allow(clippy::struct_excessive_bools)]
#[derive(clap::Args, Default)]
pub struct Args {
    /// The name of the profile
    #[arg(value_hint = ValueHint::CommandName)]
    pub profile: String,

    /// Undo integration for the profile.
    #[arg(short, long)]
    pub remove: bool,

    /// Some desktop environments, particularly Gnome, source
    /// their icons via the Flatpak ID (The Profile ID) in this case.
    /// This value must be in reverse DNS format, and Antimony automatically
    /// prepends "antimony." on those that don't. This presents an
    /// incongruity between ID and desktop that requires a shadow that
    /// hides the original. If an integrated profile lacks an icon, you
    /// may need to use this option.
    #[arg(short, long)]
    pub shadow: bool,

    /// Setup a user systemd service to run the sandbox on startup.
    /// Antimony has a race condition with autostart because the session bus
    /// may not be available when it starts. This impacts setting a sandbox
    /// to autostart via your DE, or /etc/xdg/autostart configurations.
    /// Antimony fixes this by installing the sandbox as a user-level
    /// service that waits for the bus before launching.
    ///
    /// Because this behavior differs from how /etc/xdg/autostart usually
    /// works, you need to explicitly opt-in to autostart, even if the
    /// program is set to autostart by the system. Antimony will stub
    /// the usual autostart mechanism and use a service.
    #[arg(short, long)]
    pub autostart: bool,

    /// If autostart, enable the service immediately.
    #[arg(short, long)]
    pub enable: bool,

    /// How to integrate configurations
    #[arg(long)]
    pub config_mode: Option<ConfigMode>,

    /// Create a desktop file if one does not exist
    #[arg(long)]
    pub create_desktop: bool,

    /// Overwrite a desktop file if it already exists.
    #[arg(short, long)]
    pub overwrite: bool,
}

#[derive(Default, ValueEnum, Copy, Clone, PartialEq, Eq)]
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

impl cli::Run for Args {
    fn run(self) -> Result<()> {
        if self.remove {
            let mut profile = match Profile::new(&self.profile, None, None, true) {
                Ok(profile) => profile.0,
                Err(_) => Profile {
                    path: Some(self.profile.clone()),
                    ..Default::default()
                },
            };

            // Load directly, since we can remove profiles that don't exist
            remove(&mut profile, &self)
        } else {
            integrate(
                &mut Profile::new(&self.profile, None, None, false)?.0,
                &self,
                false,
            )
        }
    }
}

/// Undo integration.
/// ## Errors
/// If antimony is improperly set up, or if it has inadequate permission to remove the files.
pub fn remove(profile: &mut Profile, cmd: &Args) -> Result<()> {
    user::set(user::Mode::Real)?;
    let name = &cmd.profile;

    let binary = HOME_PATH.join(".local").join("bin").join(name);
    match fs::remove_file(&binary) {
        Err(e) => eprintln!("{}: {e}.", binary.display()),
        Ok(()) => println!("Removed binary integration"),
    }

    let application = DATA_HOME
        .join("applications")
        .join(format!("{}.desktop", profile.desktop(name)));

    match fs::remove_file(&application) {
        Err(e) => eprintln!("{}: {e}.", application.display()),
        Ok(()) => println!("Removed profile desktop integration"),
    }

    let xdg = CONFIG_HOME
        .join("autostart")
        .join(format!("{}.desktop", profile.desktop(name)));
    match fs::remove_file(&xdg) {
        Err(e) => eprintln!("{}: {e}.", xdg.display()),
        Ok(()) => println!("Removed profile service integration"),
    }

    let name = if profile.id(name) != profile.desktop(name) && cmd.shadow {
        profile.id(name)
    } else {
        profile.desktop(name).into_owned()
    };

    let configurations: Vec<_> = profile.configuration.drain().collect();
    for (config, mut info) in configurations {
        if info.id.is_some() {
            info.merge(profile.clone())?;
            remove(&mut info, cmd)?;
        } else {
            let config_desktop = DATA_HOME
                .join("applications")
                .join(format!("{name}-{config}.desktop"));
            if config_desktop.exists() {
                match fs::remove_file(&config_desktop) {
                    Err(e) => eprintln!("{}: {e}.", config_desktop.display()),
                    Ok(()) => println!("Removed configuration integration"),
                }
            }
        }
    }

    let service = CONFIG_HOME
        .join("systemd")
        .join("user")
        .join(format!("antimony-{name}.service"));
    if service.exists() {
        match fs::remove_file(&service) {
            Err(e) => eprintln!("{}: {e}.", service.display()),
            Ok(()) => println!("Removed autostart service file"),
        }
    }

    let copy = DATA_HOME
        .join("applications")
        .join(format!("{name}.desktop"));

    if copy != application {
        match fs::remove_file(&copy) {
            Err(e) => eprintln!("{}: {e}.", copy.display()),
            Ok(()) => println!("Removed shadow file"),
        }
    }

    Ok(())
}

/// Fix the exec line to point to Antimony.
fn fix_exec(
    name: &str,
    local: &str,
    line: &mut String,
    config: Option<&str>,
    cmd: &Args,
    package: bool,
) {
    let args = line
        .find(' ')
        .map_or(" ", |index| &line[index.checked_add(1).unwrap_or(index)..]);

    match config {
        Some(config) => {
            *line = if package {
                String::from(format!("{name}={local} --config {config} {args}").trim())
            } else {
                String::from(
                    format!(
                        "{name}=antimony run {} --config {config} {args}",
                        cmd.profile
                    )
                    .trim(),
                )
            };
        }
        None => {
            *line = String::from(format!("{name}={local} {args}").trim());
        }
    }
}

/// Add the configurations, either by appending the Desktop File, or creating
/// new ones.
fn manage_configurations(
    contents: &mut Vec<String>,
    cmd: &Args,
    profile: &mut Profile,
    name: &str,
    local: &str,
    package: bool,
) -> Result<()> {
    match cmd.config_mode.unwrap_or_default() {
        // Integrate each into one desktop file with actions.
        ConfigMode::Action => {
            contents.push("\n".into());
            for config in profile.configuration.keys() {
                if package {
                    contents.push(format!(
                        "[Desktop Action {config}]\n\
                            Name=Antimony {} Configuration \n\
                            Exec={local} --config {config} %U \n",
                        config.to_title_case(),
                    ));
                } else {
                    contents.push(format!(
                        "[Desktop Action {config}]\n\
                        Name=Antimony {} Configuration \n\
                        Exec=antimony run {} --config {config} -- %U \n",
                        config.to_title_case(),
                        cmd.profile
                    ));
                }
            }
        }

        // Provide each configuration as a desktop file.
        ConfigMode::File => {
            let configurations: Vec<_> = profile.configuration.drain().collect();
            for (config, mut info) in configurations {
                if info.id.is_some() {
                    info.merge(profile.clone())?;
                    integrate(&mut info, cmd, package)?;
                } else {
                    let config_desktop = DATA_HOME
                        .join("applications")
                        .join(format!("{name}-{config}.desktop"));

                    let mut contents = contents.clone();
                    for line in &mut contents {
                        // Point to the symlink
                        if line.starts_with("Exec=") {
                            fix_exec("Exec", local, line, Some(config.as_str()), cmd, package);
                        }
                        if line.starts_with("TryExec=") {
                            fix_exec("TryExec", local, line, Some(config.as_str()), cmd, package);
                        }
                        if line.starts_with("Name=") {
                            let _ = write!(line, " ({})", config.to_title_case());
                        }
                    }
                    write!(File::create(config_desktop)?, "{}", contents.join("\n"))?;
                }
            }
        }
    }

    Ok(())
}

/// Make a shadow for a desktop file. By adding `NoDisplay`, we hide it from desktop environments.
/// This is used it two ways:
/// 1. For DEs that use the ID to source file icons (i.e GNOME), we need to create an `antimony.desktop`
///    file for applications without an rDNS name. This could create two identical entries, so we hide
///    the original one.
/// 2. XDG autostart files (In /etc/xdg/autostart) treat `NoDisplay` as "don't run". We use this so we can
///    use systemd as the autostart mechanism instead.
fn make_shadow(desktop_file: &Path) -> Result<Vec<String>> {
    Ok(fs::read_to_string(desktop_file)
        .with_context(|| "Failed to read desktop file")?
        .split('\n')
        .map(|e| {
            if e.contains("Name=") {
                return e.to_owned() + " (Native)\nNoDisplay=true";
            }
            e.to_owned()
        })
        .collect())
}

/// Format a desktop file to point to Antimony
///
/// ## Errors
///
/// Permission errors creating the files.
pub fn format_desktop(
    cmd: &Args,
    profile: &mut Profile,
    name: &str,
    desktop_file: &Path,
    local: &str,
    out_path: &Path,
    package: bool,
) -> Result<()> {
    let name = if package {
        name.to_owned()
    } else if profile.id(name) != profile.desktop(name) && cmd.shadow {
        // Create a shadow copy to turn on NoDisplay
        let local_desktop = make_shadow(desktop_file)?;

        // Write it out.
        let shadow = out_path.join(format!("{}.desktop", profile.desktop(name)));

        if let Some(parent) = shadow.parent()
            && !parent.exists()
        {
            fs::create_dir_all(parent)?;
        }

        write!(File::create(shadow)?, "{}", local_desktop.join("\n"))?;
        profile.id(name)
    } else {
        profile.desktop(name).into_owned()
    };

    let antimony_desktop = out_path.join(format!("{name}.desktop"));
    if antimony_desktop.exists() && !cmd.overwrite {
        return Err(anyhow!("Desktop file already exists!"));
    }

    let desktop =
        fs::read_to_string(desktop_file).with_context(|| "Failed to read desktop file")?;

    // Make the new desktop file.
    info!("Creating desktop file from {}", desktop_file.display());

    let desktop_actions = desktop.contains("Actions=");
    let append_actions = |line: &mut String| {
        for name in profile.configuration.keys() {
            let _ = write!(line, "{name};");
        }

        line.push_str("native;");
    };

    let mut contents: Vec<String> = desktop.lines().map(ToOwned::to_owned).collect();
    for line in &mut contents {
        // Point to the symlink
        if line.starts_with("Exec=") {
            fix_exec("Exec", local, line, None, cmd, package);
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
            *line = String::from("DBusActivatable=false");
        }

        if cmd.config_mode.unwrap_or_default() == ConfigMode::Action && line.starts_with("Actions=")
        {
            append_actions(line);
        }
    }

    if !package {
        contents.push(format!(
            "\n[Desktop Action native]\n\
            Name=Run {} Natively\n\
            Exec={}",
            cmd.profile.to_title_case(),
            profile.app_path(&cmd.profile)
        ));
    }

    // Add configurations.
    manage_configurations(&mut contents, cmd, profile, &name, local, package)?;

    if let Some(parent) = antimony_desktop.parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent)?;
    }

    write!(
        File::create(antimony_desktop).with_context(|| "Failed to create new desktop file")?,
        "{}",
        contents.join("\n")
    )
    .with_context(|| "Failed to write new desktop file")?;
    Ok(())
}

/// Integrate a profile so it can be launched in place of the original in Desktop Environments.
///
/// ## Errors
/// If antimony is improperly set up, or if it has inadequate permission to create the files.
#[allow(clippy::too_many_lines)]
pub fn integrate(profile: &mut Profile, cmd: &Args, package: bool) -> Result<()> {
    user::set(user::Mode::Real)?;

    // Collect environment.
    let antimony = which::which("antimony")?;
    let name = &cmd.profile;

    // If ~/.local/bin is in PATH, the symlink takes precedence over
    // and thus applications will run in the sandbox unless the absolute
    // path in /usr/bin is given.
    info!("Creating symlink in ~/.local/bin");
    let local = HOME_PATH.join(".local").join("bin");
    if !local.exists() {
        println!("Creating a local bin folder at ~/.local/bin. You may need to update your PATH if you want
            to launch sandboxed applications from the command line without the explicit path.");
        fs::create_dir_all(&local)?;
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
    if desktop_file.exists() {
        format_desktop(
            cmd,
            profile,
            name,
            &desktop_file,
            &local,
            &DATA_HOME.join("applications"),
            package,
        )?;
    } else if cmd.create_desktop {
        let mut contents: Vec<String> = vec![
            "[Desktop Entry]",
            &format!("Name={name}"),
            &format!("Exec={local}"),
            "Type=Application",
        ]
        .into_iter()
        .map(ToOwned::to_owned)
        .collect();

        let mut actions = "Actions=native;".to_owned();
        if cmd.config_mode == Some(ConfigMode::Action) {
            for config in profile.configuration.keys() {
                let _ = write!(actions, "{config};");
            }
        }
        contents.push(actions);

        contents.extend([
            "[Desktop Action native]".to_owned(),
            format!("Name=Run {} Natively", name.to_title_case()),
            format!("Exec={}", profile.app_path(&cmd.profile)),
        ]);

        let out = DATA_HOME
            .join("applications")
            .join(name)
            .with_extension("desktop");
        if let Some(parent) = out.parent()
            && !parent.exists()
        {
            fs::create_dir_all(parent)?;
        }
        manage_configurations(&mut contents, cmd, profile, name, &local, package)?;
        fs::write(out, contents.join("\n"))?;
    }

    if cmd.autostart {
        let autostart_name = format!("{}.desktop", profile.desktop(name));
        let service_file = Path::new("/etc")
            .join("xdg")
            .join("autostart")
            .join(&autostart_name);
        if service_file.exists() {
            info!("Overriding XDG Service");
            let shadow = make_shadow(&service_file)?;
            let shadow_path = CONFIG_HOME.join("autostart").join(autostart_name);
            if let Some(parent) = shadow_path.parent()
                && !parent.exists()
            {
                fs::create_dir_all(parent)?;
            }

            fs::write(shadow_path, shadow.join("\n"))?;
        }

        let service_vec = |config: bool| -> Vec<_> {
            let mut description = format!("Description=Run {name} under Antimony");
            if config {
                description.push_str(" with Configuration %i");
            }
            let after = format!("After=run-user-{}.mount", USER.real);
            let requires = format!("Requires=run-user-{}.mount", USER.real);
            let condition = format!("ConditionPathExists=/run/user/{}/bus", USER.real);
            let environment = format!(
                "Environment=DBUS_SESSION_BUS_ADDRESS={}",
                SESSION_BUS.as_str()
            );
            let mut exec = format!("ExecStart=antimony run {name}");
            if config {
                exec.push_str(" -c %i");
            }

            [
                "[Unit]",
                &description,
                &after,
                &requires,
                &condition,
                "[Service]",
                "Type=simple",
                "Restart=on-failure",
                "RestartSec=1",
                "StartLimitBurst=10",
                "StartLimitIntervalSec=60",
                &environment,
                "Environment=NOTIFY=none",
                &exec,
                "[Install]",
                "WantedBy=default.target",
            ]
            .into_iter()
            .map(String::from)
            .collect()
        };

        let mut service_path = CONFIG_HOME
            .join("systemd")
            .join("user")
            .join(format!("antimony-{name}.service"));
        if let Some(parent) = service_path.parent()
            && !parent.exists()
        {
            fs::create_dir_all(parent)?;
        }
        fs::write(&service_path, service_vec(false).join("\n"))?;

        if !profile.configuration.is_empty() {
            service_path.set_file_name(format!("antimony-{name}@.service"));
            fs::write(&service_path, service_vec(true).join("\n"))?;
            println!("This profile has a configuration! A configuration service has been added.");
        }

        if cmd.enable {
            Spawner::abs("/usr/bin/systemctl")
                .args(["--user", "daemon-reload"])
                .mode(Mode::Real)
                .preserve_env(true)
                .spawn()?
                .wait()?;

            if profile.configuration.is_empty() {
                Spawner::abs("/usr/bin/systemctl")
                    .args(["--user", "enable", &format!("antimony-{name}.service")])
                    .mode(Mode::Real)
                    .preserve_env(true)
                    .spawn()?
                    .wait()?;
            } else {
                println!(
                    "Configuration found, not enabling. Enable a specific configuration with antimony-{name}@config.service. To enable the non-config profile, enable antimony-{name}.service"
                );
            }
        } else {
            println!("Service in place. Reload the daemon or reboot, and enable it!");
        }
    }

    Ok(())
}
