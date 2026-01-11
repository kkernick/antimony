//! Run a profile.

use crate::{
    cli::{self, Run},
    fab::localize_home,
    setup::setup,
    shared::{
        config::CONFIG_FILE,
        env::RUNTIME_DIR,
        profile::{
            files::FileMode, home::HomePolicy, ipc::Portal, ns::Namespace, seccomp::SeccompPolicy,
        },
        utility,
    },
    timer,
};
use anyhow::{Result, anyhow};
use clap::Parser;
use heck::ToTitleCase;
use log::{debug, error};
use nix::errno::Errno;
use spawn::{Spawner, StreamMode};
use std::{borrow::Cow, env, fs, thread, time::Duration};

#[derive(clap::Args, Default)]
pub struct Args {
    /// The name of the profile, or a command to sandbox.
    ///
    /// If no profile with the desired name exists, it will be assumed
    /// as an arbitrary binary that will be looked up in PATH. It will
    /// then be run sandboxed, to which all aspects of the profile can be
    /// configured via the command line.
    ///
    /// This allows you to run any application in Antimony, without needing
    /// to make a profile for it (Although it's strongly recommended to
    /// create a profile for integration and repeated use).
    pub profile: String,

    /// Generate the profile, but do not run the executable.
    #[arg(short, long, default_value_t = false)]
    pub dry: bool,

    /// Refresh cache definitions. Analogous to `antimony refresh`
    #[arg(short, long, default_value_t = false)]
    pub refresh: bool,

    /// The path to the binary
    #[arg(long)]
    pub path: Option<String>,

    /// Use a configuration within the profile.
    #[arg(short, long)]
    pub config: Option<String>,

    /// Additional features.
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub features: Option<Vec<String>>,

    /// Conflicting features
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub conflicts: Option<Vec<String>>,

    /// Additional inheritance.
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub inherits: Option<Vec<String>>,

    /// Override the home policy
    #[arg(long)]
    pub home_policy: Option<HomePolicy>,

    /// Override the home name
    #[arg(long)]
    pub home_name: Option<String>,

    /// Override the home mount
    #[arg(long)]
    pub home_path: Option<String>,

    /// Override the home lock
    #[arg(long)]
    pub home_lock: Option<bool>,

    /// Override the seccomp policy
    #[arg(long)]
    pub seccomp: Option<SeccompPolicy>,

    /// Add portals
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub portals: Option<Vec<Portal>>,

    /// Add busses the sandbox can see.
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub see: Option<Vec<String>>,

    /// Add busses the sandbox can talk to.
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub talk: Option<Vec<String>>,

    /// Add busses the sandbox owns.
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub own: Option<Vec<String>>,

    /// Add busses the sandbox can call.
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub call: Option<Vec<String>>,

    /// Disable all IPC. This overrules all other IPC settings.
    #[arg(long, default_value_t = false)]
    pub disable_ipc: bool,

    /// Provide the system bus.
    #[arg(long, default_value_t = false)]
    pub system_bus: bool,

    /// Provide the user bus. xdg-dbus-proxy is not run.
    #[arg(long, default_value_t = false)]
    pub user_bus: bool,

    /// Override the file passthrough mode.
    #[arg(long)]
    pub file_passthrough: Option<FileMode>,

    /// Add read-only files
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub ro: Option<Vec<String>>,

    /// Add read-write files.
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub rw: Option<Vec<String>>,

    /// Add binaries
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub binaries: Option<Vec<String>>,

    /// Add libraries
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub libraries: Option<Vec<String>>,

    /// Add devices
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub devices: Option<Vec<String>>,

    /// Add namespaces
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub namespaces: Option<Vec<Namespace>>,

    /// Add environment variables in KEY=VALUE syntax
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub env: Option<Vec<String>>,

    /// Allow the sandbox to acquire new privileges.
    #[arg(long)]
    pub new_privileges: Option<bool>,

    /// Arguments to pass to bubblewrap/wrapper
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub sandbox_args: Option<Vec<String>>,

    /// Arguments to pass to the profile application.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub passthrough: Option<Vec<String>>,
}
impl cli::Run for Args {
    fn run(mut self) -> Result<()> {
        user::set(user::Mode::Effective)?;
        let result = || -> Result<()> {
            let info = timer!(
                "::setup",
                setup(Cow::Owned(self.profile.clone()), &mut self)
            )?;
            timer!("::run", run(info, &mut self))?;
            Ok(())
        }();

        if let Err(e) = result {
            error!("{e}");
            return Err(e);
        }
        Ok(())
    }
}

/// Wait for a filesystem to be mounted.
pub fn mounted(path: &str) -> bool {
    if let Ok(file) = fs::read_to_string("/proc/self/mountinfo") {
        file.contains(path)
    } else {
        false
    }
}

/// Wait for the document portal to be mounted.
pub fn wait_for_doc() {
    let doc_path = format!("{}/doc", RUNTIME_DIR.display());
    while !mounted(&doc_path) {
        thread::sleep(Duration::from_millis(10));
    }
}

/// Run the profile.
pub fn run(mut info: crate::setup::Info, args: &mut Args) -> Result<()> {
    let sandbox_args = &info.profile.sandbox_args;
    let add_regular = if !sandbox_args.is_empty() {
        let mut add = true;
        if sandbox_args.iter().filter(|&e| *e == "#").count() > 1 {
            return Err(anyhow!("Conflicting features! Only one feature can use #"));
        }

        let slice = match sandbox_args.iter().position(|e| e == "#") {
            Some(index) => {
                if index < sandbox_args.len() - 1 {
                    &sandbox_args[index + 1..]
                } else {
                    &sandbox_args[..]
                }
            }
            None => &sandbox_args[..],
        };

        for arg in slice {
            if arg == "!" {
                add = false;
                break;
            } else {
                info.handle.arg_i(arg)?
            }
        }
        add
    } else {
        true
    };

    if add_regular {
        info.handle
            .arg_i(localize_home(&info.profile.app_path(&info.name)))?;
        info.handle.args_i(info.post)?;
    }

    // Run it
    if !args.dry {
        if let Some(ipc) = info.profile.ipc.take()
            && !ipc.disable.unwrap_or(false)
        {
            debug!("Waiting for document portal");
            wait_for_doc();
        }

        if let Some(hooks) = &mut info.profile.hooks {
            if let Some(pre) = hooks.pre.take() {
                debug!("Processing pre-hooks");
                for hook in pre {
                    info.handle = hook
                        .process(
                            Some(info.handle),
                            &info.name,
                            &info.sys_dir.to_string_lossy(),
                            info.instance.name(),
                            &info.home,
                            false,
                        )?
                        .unwrap()
                }
            }

            // Attaching to the parent means info.handle becomes the parent
            if let Some(parent) = hooks.parent.take() {
                info.handle = parent
                    .process(
                        Some(info.handle),
                        &info.name,
                        &info.sys_dir.to_string_lossy(),
                        info.instance.name(),
                        &info.home,
                        true,
                    )?
                    .unwrap()
            }
        }

        log::trace!("Spawning");
        let code = info.handle.spawn()?.wait()?;

        if code != 0 {
            if CONFIG_FILE.auto_refresh() && !args.refresh {
                Spawner::abs(utility("notify"))
                    .pass_env("DBUS_SESSION_BUS_ADDRESS")?
                    .mode(user::Mode::Real)
                    .output(StreamMode::Pipe)
                    .args([
                        "--title",
                        &format!("Sandbox Auto-Refreshing: {}", info.name.to_title_case()),
                        "--body",
                        "The sandbox encountered an error, and is automatically attempting
                        to refresh cached definitions.",
                        "--timeout",
                        "5000",
                    ])?
                    .spawn()?
                    .wait()?;

                let cli = cli::Cli::parse();
                if let cli::Command::Run(mut args) = cli.command {
                    args.refresh = true;
                    return args.run();
                }
            }

            // Alert the user.
            let error_name = Errno::from_raw(-code);
            Spawner::abs(utility("notify")).mode(user::Mode::Real)
                .pass_env("DBUS_SESSION_BUS_ADDRESS")?
                .output(StreamMode::Pipe)
                .args([
                "--title",
                &format!("Sandbox Error: {}", info.name.to_title_case()),
                "--body",
                &format!(
                    "The sandbox terminated with <b>{}</b>. This may indicate a missing library or resource, incomplete SECCOMP filter, or invalid configuration.",
                    if error_name != Errno::UnknownErrno {
                        format!("{error_name}")
                    } else {
                        format!("exit code: {code}")
                    }
                ),
                "--timeout", "5000",
                "--urgency", "critical",
            ])?.spawn()?.output_all()?;
        }

        if let Some(mut hooks) = info.profile.hooks.take()
            && let Some(post) = hooks.post.take()
        {
            debug!("Executing post-hooks");
            for hook in post {
                hook.process(
                    None,
                    &info.name,
                    &info.sys_dir.to_string_lossy(),
                    info.instance.name(),
                    &info.home,
                    false,
                )?;
            }
        }
    }
    Ok(())
}

/// Use the symlink name as the profile name.
pub fn as_symlink() -> Result<()> {
    match env::args().next() {
        Some(name) => {
            let base = match name.rfind('/') {
                Some(i) => &name[i + 1..],
                _ => name.as_str(),
            };
            if base == "antimony" {
                return Err(anyhow!(
                    "When running without a command, Antimony expects to be symlinked with the link \
                    name corresponding to the profile name, such as /usr/bin/antimony -> ~/.local/bin/bash \
                    to run the bash profile. You may want `antimony run` instead."
                ));
            }

            let mut args = Args::default();
            if env::args().len() > 1 {
                args.passthrough = Some(env::args().skip(1).collect());
            }
            args.profile = base.to_string();
            args.run()?;
        }
        _ => return Err(anyhow!("Failed to parse arguments")),
    }
    Ok(())
}
