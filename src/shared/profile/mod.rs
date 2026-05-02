#![allow(
    clippy::absolute_paths,
    clippy::missing_docs_in_private_items,
    clippy::missing_errors_doc
)]
//! The Profile defines what application should be run, and what features it needs
//! to function properly. It's the core of Antimony, and has been separated into
//! separate files for readability.

pub mod files;
pub mod home;
pub mod hooks;
pub mod ipc;
pub mod lib;
pub mod ns;
pub mod seccomp;

use crate::{
    cli, fab,
    shared::{
        Map, Set,
        config::CONFIG_FILE,
        edit,
        env::HOME,
        profile::lib::Libraries,
        store::{self, CACHE_STORE, Object, USER_STORE},
    },
    timer,
};
use ahash::RandomState;
use bilrost::{Message, OwnedMessage};
use log::debug;
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, io, path::Path};
use thiserror::Error;
use user::as_real;
use which::which;

/// An error for issues around Profiles.
#[derive(Debug, Error)]
pub enum Error {
    /// When a profile doesn't exist.
    #[error("No such profile {0}: {1}")]
    NotFound(String, Cow<'static, str>),

    /// When the profile cannot be Deserialized.
    #[error("Failed to deserialize profile: {0}")]
    Deserialize(#[from] toml::de::Error),

    /// When the profile cannot be Serialized.
    #[error("Failed to serialize profile: {0}")]
    Serialize(#[from] toml::ser::Error),

    /// When the profile cannot be Deserialized from cache.
    #[error("Failed to deserialize cached profile: {0}")]
    Cache(#[from] bilrost::DecodeError),

    /// Misc IO errors.
    #[error("I/O Error: {0}")]
    Io(#[from] io::Error),

    /// User errors.
    #[error("SetUID error: {0}")]
    User(#[from] user::Error),

    /// Errors resolving/creating paths.
    #[error("Path error: {0}")]
    Path(#[from] which::Error),

    /// Errors incorporating features.
    #[error("Feature error: {0}")]
    Feature(#[from] crate::fab::features::Error),

    #[error("Profile Store Error: {0}")]
    Store(#[from] store::Error),
}

#[inline]
fn default_inherits() -> Set<String> {
    if !CONFIG_FILE.system_mode() && USER_STORE.borrow().exists("default", Object::Profile) {
        Set::from_iter(["default".to_owned()])
    } else {
        Set::default()
    }
}

/// The definitions needed to sandbox an application.
#[derive(Deserialize, Serialize, Default, Debug, PartialEq, Message)]
#[serde(deny_unknown_fields, default)]
pub struct Profile {
    /// The path to the application
    pub path: Option<String>,

    /// The path to start within inside the sandbox.
    pub dir: Option<String>,

    /// Run in lockdown mode.
    pub lockdown: Option<bool>,

    /// The ID of the application is a unique identifier that, when not defined,
    /// defaults to the name of the binary. It should be the name of the associated
    /// .desktop file installed in /usr/share/applications used to launch the
    /// program normally.
    ///
    /// It's used in two ways:
    ///     1.  It's used as the Internal Flatpak ID. Gnome sources the icon for an application
    ///         by loading the desktop file with the same name as the ID (With the .desktop)
    ///         extension. If it cannot find such a desktop file, it will return a generic icon.
    ///         Other desktop environments, such as KDE, will use the icon defined in the .desktop
    ///         file that was launched originally. This means that the internal ID is irrelevant
    ///         when running under KDE.
    ///     2.  For integration.
    pub id: Option<String>,

    /// Features the sandbox uses.
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub features: Set<String>,

    /// Features that should be excluded from running under the profile.
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub conflicts: Set<String>,

    /// A list of profiles to use as a foundation for missing values.
    ///
    /// Missing values inherit those from the inherited profiles,
    /// conflicting values take the profile's, not the inherited.
    ///
    /// This can be used to create multiple variants of a single profiles, such as
    /// versions of an editor (Zed Preview inherits Zed), a baseline configuration
    /// (`LibreOffice`), or different variants (A version of Chromium using the Home,
    /// and another without any home for a clean variant).
    ///
    /// path and id cannot be inherited. However,
    /// inherit itself is recursive, so if a profile inherits a profile, and that profile
    /// inherits something else, the top-level profile will inherit both.
    ///
    /// When this value is not defined, it will default to only the user Default Profile.
    /// If you define inherits, if you do not put "default" in the list, the profile will
    /// exclude the default profile (In case you need to exempt a profile
    /// from the Default Profile). You can define inherits to [] if you just want to exempt
    /// the Profile from the Default.
    #[serde(default = "default_inherits")]
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub inherits: Set<String>,

    /// Configuration for the profile's home.
    pub home: Option<home::Home>,

    /// The SECCOMP policy dictates whether to use SECCOMP to constrain the sandbox.
    pub seccomp: Option<seccomp::SeccompPolicy>,

    /// IPC communication through D-Bus mediated via xdg-dbus-proxy.
    pub ipc: Option<ipc::Ipc>,

    /// Files passed to the sandbox. System files are canonicalized at the sandbox root,
    /// Home files are canonicalized at /home/antimony
    pub files: Option<files::Files>,

    /// Binaries needed in the sandbox.
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub binaries: Set<String>,

    /// Libraries needed in the sandbox. They can be listed as:
    /// 1. Files (eg /usr/lib/lib.so)
    /// 2. Directories (eg /usr/lib/mylib) to which all contents will be resolved
    /// 3. Wildcards (eg lib*), which can match directories and files.
    pub libraries: Option<Libraries>,

    /// Devices needed in the sandbox, at /dev.
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub devices: Set<String>,

    /// Namespaces, such as User and Net.
    #[serde(skip_serializing_if = "Set::is_empty")]
    pub namespaces: Set<ns::Namespace>,

    /// Environment Variable Keypairs
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub environment: Map<String, String>,

    /// Arguments to pass to the sandboxed application directly.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<String>,

    /// Configurations act as embedded profiles, inheriting the main one.
    #[serde(skip_serializing_if = "Map::is_empty")]
    #[bilrost(recurses)]
    pub configuration: Map<String, Self>,

    /// Hooks are either embedded shell scripts, or paths to executables that are run in coordination with the profile.
    pub hooks: Option<hooks::Hooks>,

    /// Whether the program has unique privileges that `NO_NEW_PRIVS` can restrict.
    /// Note that this does grant privileges, it merely allows an application with existing privileges to
    /// keep them when running within the sandbox. However, this property being allowed in the sandbox
    /// means that an other unprivileged process could gain extra privilege if there's a binary in the
    /// sandbox with privilege, and this flag is enabled (Though note the sandbox cannot elevate to root,
    /// regardless of privilege).
    pub new_privileges: Option<bool>,

    /// Arguments to pass to Bubblewrap directly before the program. This could be actual bubblewrap arguments,
    /// or a wrapper for the sandbox.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sandbox_args: Vec<String>,
}
impl Profile {
    /// Construct a profile from the command line.
    /// Technically, everything needed for a profile can be specified
    /// from the command line that is needed to run a profile, so
    /// this can be used to either supplement a profile, or run applications
    /// without profiles defined.
    ///
    /// You probably shouldn't use this as the default way of running stuff,
    /// however.
    pub fn from_args(args: &mut cli::run::Args) -> Result<Self, Error> {
        let mut profile = Self {
            path: args.path.take(),
            dir: args.dir.take(),
            lockdown: args.lockdown.take(),
            seccomp: args.seccomp.take(),
            new_privileges: args.new_privileges.take(),
            ..Default::default()
        };

        if let Some(mut arguments) = args.passthrough.take() {
            profile.arguments.append(&mut arguments);
        }

        if let Some(mut sandbox_args) = args.sandbox_args.take() {
            profile.sandbox_args.append(&mut sandbox_args);
        }

        if let Some(features) = args.features.take() {
            profile.features.extend(features);
        }

        if let Some(inherits) = args.inherits.take() {
            profile.inherits.extend(inherits);
        }

        if let Some(conflicts) = args.conflicts.take() {
            profile.conflicts.extend(conflicts);
        }

        if let Some(binaries) = args.binaries.take() {
            profile.binaries.extend(binaries);
        }

        if let Some(devices) = args.devices.take() {
            profile.devices.extend(devices);
        }

        if let Some(namespaces) = args.namespaces.take() {
            profile.namespaces.extend(namespaces);
        }

        profile.libraries = lib::Libraries::from_args(args);
        profile.files = files::Files::from_args(args);
        profile.ipc = ipc::Ipc::from_args(args);

        if let Some(env) = args.env.take() {
            let environment = &mut profile.environment;
            for pair in env {
                if let Some((key, value)) = pair.split_once('=') {
                    environment.insert(key.to_owned(), value.to_owned());
                }
            }
        }

        if let Some(lock) = args.home_lock.take() {
            profile.home.get_or_insert_default().lock = Some(lock);
        }
        if let Some(name) = args.home_name.take() {
            profile.home.get_or_insert_default().name = Some(name);
        }
        if let Some(path) = args.home_path.take() {
            profile.home.get_or_insert_default().path = Some(path);
        }
        if let Some(policy) = args.home_policy.take() {
            profile.home.get_or_insert_default().policy = Some(policy);
        }

        Ok(profile)
    }

    /// Load a new profile from all supported locations.
    pub fn new(
        name: &str,
        config: Option<String>,
        args: Option<&mut cli::run::Args>,
        foreign: bool,
    ) -> Result<(Self, String), Error> {
        debug!("Loading {name}");

        let mut profile = timer!(
            "::load",
            match store::load::<Self, Error>(name, Object::Profile, true) {
                Ok(profile) => profile,
                Err(Error::Store(store::Error::Io(e))) if e.kind() == io::ErrorKind::NotFound => {
                    debug!("No profile: {name}, assuming binary");
                    Self {
                        path: Some(which::which(name)?.to_owned()),
                        ..Default::default()
                    }
                }
                Err(e) => return Err(e),
            }
        );
        if name == "default" {
            return Ok((profile, "default".to_owned()));
        }

        if let Some(args) = args {
            if !CONFIG_FILE.system_mode() {
                let cmd_profile = Self::from_args(args)?;
                profile = profile.base(cmd_profile)?;
            }
            if !std::path::Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
                && profile.path.is_none()
            {
                profile.path = Some(which::which(&profile.app_path(name))?.to_owned());
            }
        }

        let hash = profile.hash_str(&config);
        if let Ok(bytes) = timer!(
            "::fetch_cache",
            CACHE_STORE.borrow().bytes(&hash, Object::Profile)
        ) {
            debug!("Using cached profile");
            return Ok((Self::decode(bytes.as_slice())?, hash));
        }

        if let Some(path) = &profile.path {
            if path.starts_with('~') {
                profile.path = Some(path.replace('~', HOME.as_str()));
            } else if path.starts_with("$AT_HOME")
                && let Some(home) = &profile.home
            {
                profile.path = Some(path.replace("$AT_HOME", &home.path(name).to_string_lossy()));
            }
        }

        let app_path = profile.app_path(name);
        let path = Path::new(app_path.as_ref());
        if !as_real!(path.exists())? {
            match which::which(app_path.as_ref()) {
                Ok(path) => profile.path = Some(path.to_owned()),
                Err(_) => {
                    if !foreign {
                        return Err(Error::NotFound(
                            name.to_owned(),
                            Cow::Borrowed("Profile binary does not exist on system"),
                        ));
                    }
                }
            }
        }

        debug!("No cache available");
        for inherit in profile.inherits.clone() {
            profile.merge(Self::new(&inherit, None, None, true)?.0)?;
        }

        if let Some(config) = config {
            debug!("Loading configuration");
            if profile.configuration.is_empty() {
                return Err(Error::NotFound(
                    name.to_owned(),
                    Cow::Borrowed("Profile does not have any configurations"),
                ));
            }
            match profile.configuration.remove(&config) {
                Some(conf) => {
                    let hooks = conf.hooks.clone();
                    profile = profile.base(conf)?;
                    if let Some(hooks) = hooks
                        && !hooks.inherit.unwrap_or(true)
                    {
                        profile.hooks = Some(hooks);
                    }
                }
                None => {
                    return Err(Error::NotFound(
                        name.to_owned(),
                        Cow::Owned(format!("Configuration {config} does not exist")),
                    ));
                }
            }
        }

        debug!("Fabricating features");
        fab::features::fabricate(&mut profile, name)?;
        CACHE_STORE
            .borrow()
            .dump(&hash, Object::Profile, &Self::encode_to_bytes(&profile))?;

        Ok((profile, hash))
    }

    /// Use another profile as the base for the caller.
    /// This function effectively inverts the logic of `merge`:
    /// The values in the source take precedent, either appending or
    /// overwriting the caller.
    ///
    /// The difference is that values unaffected by `merge` persist in
    /// the caller.
    #[allow(clippy::assigning_clones)]
    pub fn base(mut self, mut source: Self) -> Result<Self, Error> {
        source.id = self.id.take();
        source.inherits = self.inherits.clone();

        source.merge(self)?;
        Ok(source)
    }

    /// Merge the contents of one profile into another.
    /// The merging process follows two rules:
    ///     1.  If the caller has a value defined for single-value
    ///         options, the argument's value is ignored.
    ///     2.  For list values, the argument's values are either
    ///         taken if the caller has no value defined, or
    ///         the caller's list is appended with the contents
    ///         of the argument.
    /// This function fails if inherit is defined, but its
    /// contents cannot be inherited.
    ///
    /// Cache status is not inherited, neither is path, name,
    /// or desktop.
    pub fn merge(&mut self, mut profile: Self) -> Result<(), Error> {
        if self.path.is_none() {
            self.path = profile.path;
        }

        if self.dir.is_none() {
            self.dir = profile.dir;
        }

        if self.lockdown.is_none() {
            self.lockdown = profile.lockdown;
        }

        if self.seccomp.is_none() {
            self.seccomp = profile.seccomp;
        }

        if self.new_privileges.is_none() {
            self.new_privileges = profile.new_privileges;
        }

        if let Some(home) = profile.home {
            if let Some(s_home) = &mut self.home {
                s_home.merge(home);
            } else {
                self.home = Some(home);
            }
        }

        if let Some(files) = profile.files {
            if let Some(s_files) = &mut self.files {
                s_files.merge(files);
            } else {
                self.files = Some(files);
            }
        }

        if let Some(ipc) = profile.ipc {
            if let Some(s_ipc) = &mut self.ipc {
                s_ipc.merge(ipc);
            } else {
                self.ipc = Some(ipc);
            }
        }

        if let Some(hooks) = profile.hooks {
            if let Some(s_hooks) = &mut self.hooks {
                s_hooks.merge(hooks);
            } else {
                self.hooks = Some(hooks);
            }
        }

        if let Some(libraries) = profile.libraries {
            if let Some(s_lib) = &mut self.libraries {
                s_lib.merge(libraries);
            } else {
                self.libraries = Some(libraries);
            }
        }

        for (name, config) in profile.configuration {
            self.configuration.insert(name, config);
        }

        for (key, val) in profile.environment {
            self.environment.insert(key, val);
        }

        self.namespaces.extend(profile.namespaces);
        self.binaries.extend(profile.binaries);
        self.devices.extend(profile.devices);
        self.features.extend(profile.features);
        self.conflicts.extend(profile.conflicts);
        self.arguments.append(&mut profile.arguments);
        self.sandbox_args.append(&mut profile.sandbox_args);
        Ok(())
    }

    /// Get the path of the profile binary.
    #[must_use]
    pub fn app_path<'a>(&'a self, name: &'a str) -> Cow<'a, str> {
        self.path.as_ref().map_or_else(
            || which(name).map_or(Cow::Borrowed(name), Cow::Borrowed),
            |path| Cow::Borrowed(path),
        )
    }

    /// Get the id name, using as the Flatpak ID.
    /// It's either the id name, or from `name()`
    #[must_use]
    pub fn desktop<'a, 'b>(&'b self, name: &'a str) -> Cow<'a, str>
    where
        'b: 'a,
    {
        self.id.as_ref().map_or_else(
            || {
                let path = self.app_path(name);
                let bin_name = if let Some(i) = path.rfind('/')
                    && let Some(i) = i.checked_add(1)
                {
                    let slice = &path[i..];
                    Cow::Owned(slice.to_owned())
                } else {
                    path
                };

                if bin_name.contains('.') {
                    bin_name
                } else {
                    Cow::Borrowed(name)
                }
            },
            |id| Cow::Borrowed(id),
        )
    }

    /// Format the id as the Flatpak ID.
    #[must_use]
    pub fn id(&self, name: &str) -> String {
        let id = self.desktop(name);
        if id.contains('.') {
            id.to_string()
        } else {
            format!("antimony.{id}")
        }
    }

    /// Get the numerical hash of the profile.
    pub fn num_hash(&self, config: &Option<String>) -> u64 {
        let mut bytes = Self::encode_to_bytes(self).to_vec();
        bytes.push(u8::from(CONFIG_FILE.system_mode()));
        if let Some(config) = config {
            bytes.extend_from_slice(config.as_bytes());
        }
        RandomState::with_seeds(0, 0, 0, 0).hash_one(bytes)
    }

    /// Get the Profile's hash.
    #[must_use]
    pub fn hash_str(&self, config: &Option<String>) -> String {
        format!("{}", self.num_hash(config))
    }

    /// Edit a profile.
    pub fn edit(profile: &str) -> Result<Option<String>, edit::Error> {
        edit::edit::<Self>(profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::panic)]
    fn validate_profiles() {
        for profile in store::SYSTEM_STORE
            .borrow()
            .get(Object::Profile)
            .expect("Failed to get profiles")
        {
            let store = store::SYSTEM_STORE.borrow();
            toml::from_str::<Profile>(
                &store
                    .fetch(&profile, Object::Profile)
                    .expect("Failed to fetch"),
            )
            .unwrap_or_else(|_| panic!("Failed to fetch {profile}"));
        }
    }
}
