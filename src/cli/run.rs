//! Run a profile.
use crate::{
    aux::{
        env::RUNTIME_DIR,
        profile::{FileMode, HomePolicy, Namespace, Portal, SeccompPolicy},
    },
    setup::setup,
};
use anyhow::{Result, anyhow};
use log::{debug, error};
use std::{borrow::Cow, thread, time::Duration};

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

    /// Use a configuration within the profile.
    #[arg(short, long)]
    pub config: Option<String>,

    /// Additional features.
    #[arg(long, value_delimiter = ' ', num_args = 1..)]
    pub features: Option<Vec<String>>,

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
    if let Ok(file) = std::fs::read_to_string("/proc/self/mountinfo") {
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

pub fn run(info: crate::setup::Info, args: &mut Args) -> Result<()> {
    info.handle.arg_i(info.profile.app_path(&args.profile))?;
    info.handle.args_i(info.post)?;

    // Run it
    if !args.dry {
        debug!("Waiting for document portal");
        wait_for_doc();

        debug!("Spawning");
        if info.handle.spawn()?.wait()? != 0 {
            error!(
                "The subcommand finished with a non-zero exit code! You may want to try `antimony trace` to see if there are missing files!"
            );
        }
    }

    crate::setup::cleanup(info.instance)?;
    Ok(())
}

/// Use the symlink name as the profile name.
pub fn as_symlink() -> Result<()> {
    match std::env::args().next() {
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
            if std::env::args().len() > 1 {
                args.passthrough = Some(std::env::args().skip(1).collect());
            }
            let info = crate::cli::run::setup(Cow::Borrowed(base), &mut args)?;
            crate::cli::run::run(info, &mut args)?;
        }
        _ => return Err(anyhow!("Failed to parse arguments")),
    }
    Ok(())
}
