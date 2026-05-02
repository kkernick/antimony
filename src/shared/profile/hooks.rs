#![allow(clippy::missing_docs_in_private_items, clippy::missing_errors_doc)]

use crate::fab::resolve;
use bilrost::{Enumeration, Message};
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

    /// If the hook did not terminate successfully, and it's not `no_fail`
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
#[derive(Deserialize, Serialize, Default, Debug, Clone, PartialEq, Eq, Message)]
#[serde(deny_unknown_fields)]
pub struct Hooks {
    /// Pre-Hooks are run before the executes.
    #[serde(default = "Vec::default")]
    pub pre: Vec<Hook>,

    /// Post-Hooks are run on cleanup.
    #[serde(default = "Vec::default")]
    pub post: Vec<Hook>,

    /// The parent Hook is an Attached Pre-Hook who controls the lifespan of the
    /// sandbox. When the parent dies, the sandbox does.
    pub parent: Option<Hook>,

    /// Inherit Hooks from other profiles, or from the parent profile
    /// in a configuration. Enabled by default, but useful if the
    /// parent defines hooks the configuration cannot use.
    pub inherit: Option<bool>,
}
impl Hooks {
    /// Merge two Hooks together.
    pub fn merge(&mut self, mut hooks: Self) {
        if self.inherit.unwrap_or(true) {
            self.pre.append(&mut hooks.pre);
            self.post.append(&mut hooks.post);

            if self.parent.is_none() {
                self.parent = hooks.parent;
            }
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Enumeration)]
pub enum Type {
    Shell = 0,
    Program = 1,
    Profile = 2,
}

/// A Hook is a program run in coordination with the profile.
///
/// Hooks are run as the user.
/// Hooks are invoked with the following environment variables:
///     `ANTIMONY_NAME`: The name of the current profile.
///     `ANTIMONY_HOME`: The path to the home folder, if it exists.
///     `ANTIMONY_CACHE`: The cache of the profile in /usr/share/antimony/cache
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Message)]
#[serde(deny_unknown_fields)]
pub struct Hook {
    /// An optional name to identify the process.
    pub name: Option<String>,

    #[serde(rename = "type")]
    pub t: Type,

    /// The contents of the hook.
    /// For scripts, the full content.
    /// For programs, the name/path of the binary.
    /// For antimony, the profile
    pub content: String,

    /// A list of arguments to be passed to the hook
    #[serde(default = "Vec::default")]
    pub arguments: Vec<String>,

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
    /// STDIN. Only one of `capture_output` and `capture_error` can be
    /// set. If both are set, `capture_error` is used.
    pub capture_output: Option<bool>,

    /// Capture the sandbox's STDERR and provide it to the Hook via
    /// STDIN. Only one of `capture_output` and `capture_error` can be
    /// set. If both are set, `capture_error` is used.
    pub capture_error: Option<bool>,
}
impl Hook {
    /// Process the hook.
    #[allow(clippy::unreachable)]
    pub fn process(
        &mut self,
        main: Option<Spawner>,
        name: &str,
        cache: &str,
        instance: &str,
        home: &Option<String>,
        parent: bool,
    ) -> Result<Option<Spawner>, HookError> {
        let handle = match self.t {
            Type::Shell | Type::Program => {
                let handle = match self.t {
                    Type::Shell => {
                        Spawner::abs("/usr/bin/bash").args(["-c", self.content.as_str()])
                    }
                    Type::Program => Spawner::new(resolve(Cow::Borrowed(&self.content)))?,
                    Type::Profile => unreachable!(),
                }
                .mode(user::Mode::Real)
                .preserve_env(self.env.unwrap_or(false))
                .env("ANTIMONY_NAME", name)
                .env("ANTIMONY_CACHE", cache)
                .env("ANTIMONY_INSTANCE", instance);

                if self.new_privileges.unwrap_or(false) {
                    handle.new_privileges_i(true);
                }
                if let Some(home) = home {
                    handle.env_i("ANTIMONY_HOME", home);
                }

                handle
            }

            Type::Profile => Spawner::abs("/usr/bin/antimony")
                .args(["run", self.content.as_str()])
                .new_privileges(true)
                .preserve_env(true)
                .mode(user::Mode::Original),
        }
        .name(self.name.get_or_insert_with(|| "hook".to_owned()));
        handle.args_i(self.arguments.drain(..));

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
            let handle = handle.spawn()?;
            if self.attach.unwrap_or(false) {
                main.map_or_else(
                    || Err(HookError::Attach),
                    |m| {
                        m.associate(handle);
                        Ok(Some(m))
                    },
                )
            } else {
                let code = handle.wait()?;
                if code != 0 && !self.can_fail.unwrap_or(false) {
                    return Err(HookError::Fail(code));
                }
                Ok(main)
            }
        }
    }
}
