use crate::{fab::resolve_env, shared::profile::append};
use console::style;
use nix::{errno, unistd::pipe};
use serde::{Deserialize, Serialize};
use spawn::{HandleError, SpawnError, Spawner, StreamMode};
use std::borrow::Cow;
use thiserror::Error;

/// An error for hooks.
#[derive(Debug, Error)]
pub enum HookError {
    /// If the path and content are both missing
    #[error("Hooks need a path or content")]
    Missing,

    /// If the hook doesn't support attaching
    #[error("This hook cannot be attached")]
    Attach,

    /// If the hook did not terminate successfully, and it's not no_fail
    #[error("Hooks failed with exit code: {0}")]
    Fail(i32),

    /// Errors spawning the hook
    #[error("Failed to spawn hook: {0}")]
    Spawn(#[from] SpawnError),

    /// Error handling the hook
    #[error("Failed to communicate with hook: {0}")]
    Handle(#[from] HandleError),

    /// Error for the Parent not being provided the correct handle
    #[error("Post Hooks cannot be parents!")]
    Parent,

    /// Misc system errors.
    #[error("System error running hooks: {0}")]
    Errno(#[from] errno::Errno),
}

/// The Hooks structure contains both pre and post hooks.
#[derive(Deserialize, Serialize, Default, Debug)]
#[serde(deny_unknown_fields, default)]
pub struct Hooks {
    /// Pre-Hooks are run before the executes.
    pub pre: Option<Vec<Hook>>,

    /// Post-Hooks are run on cleanup.
    pub post: Option<Vec<Hook>>,

    /// The parent Hook is an Attached Pre-Hook who controls the lifespan of the
    /// sandbox. When the parent dies, the sandbox does.
    pub parent: Option<Hook>,
}
impl Hooks {
    /// Merge two IPC sets together.
    pub fn merge(&mut self, hooks: Self) {
        append(&mut self.pre, hooks.pre);
        append(&mut self.post, hooks.post);

        if self.parent.is_none() {
            self.parent = hooks.parent;
        }
    }

    pub fn info(&self) {
        if let Some(pre) = &self.pre {
            println!("\tPre-Hooks");
            for hook in pre {
                hook.info();
            }
        }
        if let Some(post) = &self.post {
            println!("\tPost-Hooks");
            for hook in post {
                hook.info();
            }
        }

        if let Some(parent) = &self.parent {
            println!("\tParent Hooks");
            parent.info();
        }
    }
}

/// A Hook is a program run in coordination with the profile.
/// Hooks are run as the user.
/// Hooks are invoked with the following environment variables:
///     ANTIMONY_NAME: The name of the current profile.
///     ANTIMONY_HOME: The path to the home folder, if it exists.
///     ANTIMONY_CACHE: The cache of the profile in /usr/share/antimony/cache
#[derive(Deserialize, Serialize, Default, Debug)]
#[serde(deny_unknown_fields, default)]
pub struct Hook {
    /// An optional name to identify the process.
    pub name: Option<String>,

    /// The path to a binary
    pub path: Option<String>,

    /// The raw content of a shell script. If both path and content are defined, path is used.
    pub content: Option<String>,

    /// A list of arguments to be passed to the hook
    pub args: Option<Vec<String>>,

    /// In pre-hooks a hook can be attached to the sandbox. In this mode, the hook runs alongside
    /// the sandbox. If false, the program waits for the hook to terminate before launching
    /// the sandbox.
    pub attach: Option<bool>,

    /// Share the environment with the hook.
    pub env: Option<bool>,

    /// If the Hook can fail. If false, an error will abort the program.
    pub can_fail: Option<bool>,

    /// Allow the hook to obtain new privileges. Needed if the binary/script
    /// requires privilege antimony does not have.
    pub new_privileges: Option<bool>,

    /// Capture the sandbox's STDOUT and provide it to the Hook via
    /// STDIN. Only one of capture_output and capture_error can be
    /// set. If both are set, capture_error is used.
    pub capture_output: Option<bool>,

    /// Capture the sandbox's STDERR and provide it to the Hook via
    /// STDIN. Only one of capture_output and capture_error can be
    /// set. If both are set, capture_error is used.
    pub capture_error: Option<bool>,
}
impl Hook {
    /// Process the hook.
    pub fn process(
        self,
        main: Option<Spawner>,
        name: &str,
        cache: &str,
        instance: &str,
        home: &Option<String>,
        parent: bool,
    ) -> Result<Option<Spawner>, HookError> {
        let handle = if let Some(path) = self.path {
            Spawner::new(resolve_env(Cow::Owned(path)))?
        } else if let Some(content) = self.content {
            Spawner::abs("/usr/bin/bash").args(["-c", content.as_str()])?
        } else {
            return Err(HookError::Missing);
        }
        .name(&self.name.unwrap_or("hook".to_string()));

        handle.preserve_env_i(self.env.unwrap_or(false));
        handle.env_i("ANTIMONY_NAME", name)?;
        handle.env_i("ANTIMONY_CACHE", cache)?;
        handle.env_i("ANTIMONY_INSTANCE", instance)?;
        handle.mode_i(user::Mode::Real);

        if self.new_privileges.unwrap_or(false) {
            handle.new_privileges_i(true);
        }

        if let Some(args) = self.args {
            handle.args_i(args)?;
        }

        if let Some(home) = home {
            handle.env_i("ANTIMONY_HOME", home)?;
        }

        if parent {
            if let Some(main) = main {
                if self.capture_output.unwrap_or(false) {
                    let pipe = pipe()?;
                    handle.input_i(StreamMode::Fd(pipe.0));
                    main.output_i(StreamMode::Fd(pipe.1));
                }

                if self.capture_error.unwrap_or(false) {
                    let pipe = pipe()?;
                    handle.input_i(StreamMode::Fd(pipe.0));
                    main.error_i(StreamMode::Fd(pipe.1));
                }

                handle.associate(main.spawn()?);
                Ok(Some(handle))
            } else {
                Err(HookError::Parent)
            }
        } else {
            let mut handle = handle.spawn()?;
            if self.attach.unwrap_or(false) {
                if let Some(m) = main {
                    m.associate(handle);
                    Ok(Some(m))
                } else {
                    Err(HookError::Attach)
                }
            } else {
                let code = handle.wait()?;
                if code != 0 && !self.can_fail.unwrap_or(false) {
                    return Err(HookError::Fail(code));
                }
                Ok(main)
            }
        }
    }

    pub fn info(&self) {
        if let Some(name) = &self.name {
            println!("Hook: {name}");
        }

        if self.content.is_some() {
            print!("\t\t/usr/bin/bash -c ...")
        } else if let Some(path) = &self.path {
            print!("\t\t{path} ")
        }

        if let Some(args) = &self.args {
            for arg in args {
                print!("{arg} ")
            }
        }
        println!();
        if self.can_fail.unwrap_or(false) {
            println!("\t\t\t-> Non-Failing")
        }

        if self.env.unwrap_or(false) {
            println!("\t\t\t-> Environment Aware")
        }

        if self.attach.unwrap_or(false) {
            println!("\t\t\t-> Attached")
        }

        println!(
            "\t\t\t-> Allow New Privileges: {}",
            if self.new_privileges.unwrap_or(false) {
                style("Yes").red()
            } else {
                style("No").green()
            }
        );

        if self.capture_output.unwrap_or(false) {
            println!("\t\t\t-> Capturing Sandbox Output")
        }

        if self.capture_error.unwrap_or(false) {
            println!("\t\t\t-> Capturing Sandbox Error")
        }
    }
}
