//! Run a profile.
use crate::{
    aux::{
        env::RUNTIME_DIR,
        profile::{FileMode, HomePolicy, Namespace, Portal, SeccompPolicy},
    },
    setup::setup,
};
use anyhow::{Result, anyhow};
use inflector::Inflector;
use log::debug;
use nix::{errno::Errno, sys::signal::Signal::SIGTERM};
use spawn::Spawner;
use std::{borrow::Cow, env, fs, io::Write, thread, time::Duration};
use user::Mode;

#[derive(clap::Args, Debug, Default)]
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

    /// Collect output from the sandbox, and output it to a log file.
    #[arg(short, long, default_value_t = false)]
    pub log: bool,

    /// Refresh cache definitions. Analogous to `antimony refresh`
    #[arg(short, long, default_value_t = false)]
    pub refresh: bool,

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

    /// Arguments to pass to the profile application.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub passthrough: Option<Vec<String>>,
}
impl super::Run for Args {
    fn run(mut self) -> Result<()> {
        let info = setup(Cow::Owned(self.profile.clone()), &mut self)?;
        run(info, &mut self)
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
        thread::sleep(Duration::from_millis(100));
    }
}

pub fn run(mut info: crate::setup::Info, args: &mut Args) -> Result<()> {
    info.handle.arg_i(info.profile.app_path(&info.name))?;
    info.handle.args_i(info.post)?;
    info.handle.error_i(true);

    // Run it
    if !args.dry {
        if let Some(hooks) = &mut info.profile.hooks
            && let Some(pre) = hooks.pre.take()
        {
            debug!("Processing pre-hooks");
            for hook in pre {
                hook.process(
                    Some(&mut info.handle),
                    &info.name,
                    &info.sys_dir.to_string_lossy(),
                    &info.home,
                )?;
            }
        }

        debug!("Waiting for document portal");
        wait_for_doc();

        debug!("Spawning");
        let mut handle = info.handle.spawn()?;
        let code = handle.wait_for_signal(SIGTERM, Duration::from_millis(100))?;

        let log = if code != 0 && args.log {
            let error = handle.error_all()?;
            // Write it to a log file.
            if !error.is_empty() {
                let log = info.sys_dir.join(&info.instance).with_extension("log");
                let mut file = fs::File::create(&log)?;
                file.write_all(error.as_bytes())?;
                Some(log)
            } else {
                None
            }
        } else {
            None
        };

        if code != 0 {
            // Alert the user.
            let error_name = Errno::from_raw(-code);
            #[rustfmt::skip]
            let handle = Spawner::new("notify-send")
                .args([
                    &format!("Sandbox Error: {}", info.name.to_title_case()),
                    &format!(
                        "The sandbox terminated with <b>{}</b>. This may indicate a missing library or resource, incomplete SECCOMP filter, or invalid configuration.",
                        if error_name != Errno::UnknownErrno {format!("{error_name}")} else {format!("exit code: {code}")},
                    ),
                    "-u", "critical",
                    "-a", "Antimony",
                    "-t", "5000",
                ])?;

            // If there are errors, give the user the option to open the log
            if log.is_some() {
                handle.args_i(["-A", "Open=Open Error Log"])?;
            }

            let output = handle
                .preserve_env(true)
                .mode(Mode::Real)
                .output(true)
                .spawn()?
                .output_all()?;

            // If we want to open, use xdg-open.
            if let Some(log) = log
                && output.starts_with("Open")
            {
                Spawner::new("xdg-open")
                    .arg(log.to_string_lossy())?
                    .preserve_env(true)
                    .mode(Mode::Real)
                    .spawn()?
                    .wait()?;
            }
        } else if let Some(log) = log
            && args.log
        {
            log::info!("Log is available at {log:?}")
        }

        if let Some(mut hooks) = info.profile.hooks.take()
            && let Some(post) = hooks.post.take()
        {
            debug!("Processing post-hooks");
            for hook in post {
                hook.process(
                    None,
                    &info.name,
                    &info.sys_dir.to_string_lossy(),
                    &info.home,
                )?;
            }
        }
    }

    crate::setup::cleanup(info.instance)?;
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
            let info = crate::cli::run::setup(Cow::Borrowed(base), &mut args)?;
            crate::cli::run::run(info, &mut args)?;
        }
        _ => return Err(anyhow!("Failed to parse arguments")),
    }
    Ok(())
}
