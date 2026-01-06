//! The Profile defines what application should be run, and what features it needs
//! to function properly. It's the core of Antimony, and has been separated into
//! separate files for readability.

pub mod files;
pub mod home;
pub mod hooks;
pub mod ipc;
pub mod ns;
pub mod seccomp;

use crate::{
    cli::{self, info::Info},
    fab::{self, get_wildcards},
    shared::{
        IMap, ISet,
        config::CONFIG_FILE,
        db::{self, Database, DatabaseCache, Table},
        edit,
        env::HOME,
        format_iter,
    },
    timer,
};
use ahash::RandomState;
use console::style;
use log::{debug, info};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    fs, io,
    path::{Path, PathBuf},
    sync::LazyLock,
};
use thiserror::Error;
use which::which;

/// On our hot-path, SQLite is slower than directly than reading a file.
/// To solve this, Antimony will immediately dump all profile information
/// directly into memory (Here) on program start.
pub static USER_CACHE: LazyLock<DatabaseCache> =
    LazyLock::new(|| db::dump_all(Database::User, Table::Profiles));
pub static SYSTEM_CACHE: LazyLock<DatabaseCache> =
    LazyLock::new(|| db::dump_all(Database::System, Table::Profiles));
pub static HASH_CACHE: LazyLock<DatabaseCache> =
    LazyLock::new(|| db::dump_all(Database::Cache, Table::Profiles));

/// An error for issues around Profiles.
#[derive(Debug, Error)]
pub enum Error {
    /// When a profile doesn't exist.
    #[error("No such profile {0}: {1}")]
    NotFound(String, Cow<'static, str>),

    /// When the profile cannot be Deserialized.
    #[error("Failed to deserialize profile: {0}")]
    Deserialize(#[from] toml::de::Error),

    /// When the profile cannot be Deserialized.
    #[error("Failed to cache profile: {0}")]
    Cache(#[from] postcard::Error),

    /// When the profile cannot be Serialized.
    #[error("Failed to serialize profile: {0}")]
    Serialize(#[from] toml::ser::Error),

    /// Misc IO errors.
    #[error("I/O Error: {0}: {1}")]
    Io(&'static str, io::Error),

    /// Misc Errno errors.
    #[error("System error: {0}: {1}")]
    Errno(&'static str, nix::errno::Errno),

    /// Errors resolving/creating paths.
    #[error("Path error: {0}")]
    Path(#[from] which::Error),

    /// Errors for profile arguments specified on the command line.
    #[error("Command line error: {0}")]
    CommandLine(&'static str, String, Vec<String>),

    /// Errors incorporating features.
    #[error("Feature error: {0}")]
    Feature(#[from] crate::fab::features::Error),

    /// Database errors.
    #[error("Database error: {0}")]
    Database(#[from] db::Error),
}

/// Append two things together. Used for Profile Merging.
fn append<T>(s: &mut Option<Vec<T>>, p: Option<Vec<T>>) {
    if let Some(mut source) = p {
        if let Some(dest) = s {
            dest.append(&mut source)
        } else {
            *s = Some(source);
        }
    }
}

/// Print info about the libraries used in a feature/profile.
pub fn library_info(libraries: &ISet<String>, verbose: u8) {
    println!("\t- Libraries:");
    for library in libraries {
        if verbose > 2 && library.contains("*") {
            match get_wildcards(library, true) {
                Ok(wilds) => {
                    for wild in wilds {
                        println!("\t\t- {}", style(wild).italic());
                    }
                }
                Err(_) => println!("\t\t- {}", style(library).italic()),
            }
        } else {
            println!("\t\t- {}", style(library).italic());
        }
    }
}

/// The definitions needed to sandbox an application.
#[derive(Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields, default)]
pub struct Profile {
    /// The path to the application
    pub path: Option<String>,

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
    #[serde(skip_serializing_if = "ISet::is_empty")]
    pub features: ISet<String>,

    /// Features that should be excluded from running under the profile.
    #[serde(skip_serializing_if = "ISet::is_empty")]
    pub conflicts: ISet<String>,

    /// A list of profiles to use as a foundation for missing values.
    ///
    /// Missing values inherit those from the inherited profiles,
    /// conflicting values take the profile's, not the inherited.
    ///
    /// This can be used to create multiple variants of a single profiles, such as
    /// versions of an editor (Zed Preview inherits Zed), a baseline configuration
    /// (LibreOffice), or different variants (A version of Chromium using the Home,
    /// and another without any home for a clean variant).
    ///
    /// path and id cannot be inherited. However,
    /// inherit itself is recursive, so if a profile inherits a profile, and that profile
    /// inherits something else, the top-level profile will inherit both.
    ///
    /// When this value is not defined, it will default to ["default"], which will inherit
    /// the user Default Profile. If you define inherits, if you do not put "default" in the
    /// list the profile will exclude the default profile (In case you need to exempt a profile
    /// from the Default Profile). You can define inherits to [] if you just want to exempt
    /// the Profile from the Default.
    pub inherits: Option<ISet<String>>,

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
    #[serde(skip_serializing_if = "ISet::is_empty")]
    pub binaries: ISet<String>,

    /// Libraries needed in the sandbox. They can be listed as:
    /// 1. Files (eg /usr/lib/lib.so)
    /// 2. Directories (eg /usr/lib/mylib) to which all contents will be resolved
    /// 3. Wildcards (eg lib*), which can match directories and files.
    #[serde(skip_serializing_if = "ISet::is_empty")]
    pub libraries: ISet<String>,

    /// Devices needed in the sandbox, at /dev.
    #[serde(skip_serializing_if = "ISet::is_empty")]
    pub devices: ISet<String>,

    /// Namespaces, such as User and Net.
    #[serde(skip_serializing_if = "ISet::is_empty")]
    pub namespaces: ISet<ns::Namespace>,

    /// Environment Variable Keypairs
    #[serde(skip_serializing_if = "IMap::is_empty")]
    pub environment: IMap<String, String>,

    /// Arguments to pass to the sandboxed application directly.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<String>,

    /// Configurations act as embedded profiles, inheriting the main one.
    #[serde(skip_serializing_if = "IMap::is_empty")]
    pub configuration: IMap<String, Profile>,

    /// Hooks are either embedded shell scripts, or paths to executables that are run in coordination with the profile.
    pub hooks: Option<hooks::Hooks>,

    /// Whether the program has unique privileges that NO_NEW_PRIVS can restrict.
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
    /// Load a profile from the database. This does not include any feature fabrication.
    pub fn load(name: &str) -> Result<Self, Error> {
        if name == "default" {
            if !CONFIG_FILE.system_mode()
                && let Ok(cache) = USER_CACHE.as_ref()
                && let Some(def) = cache.get("default")
            {
                return Ok(toml::from_str(def)?);
            } else if let Ok(cache) = SYSTEM_CACHE.as_ref()
                && let Some(def) = cache.get("default")
            {
                if !CONFIG_FILE.system_mode() {
                    db::store_str("default", def, Database::User, Table::Profiles)?;
                }
                return Ok(toml::from_str(def)?);
            } else {
                return Ok(Profile::default());
            }
        }

        // Try and load a file absolutely if the file is given.
        if name.ends_with(".toml") {
            let path = PathBuf::from(name);
            if path.exists() {
                info!("Using File");
                return Ok(toml::from_str(
                    &fs::read_to_string(path).map_err(|e| Error::Io("reading TOML", e))?,
                )?);
            }
        }

        if !CONFIG_FILE.system_mode()
            && let Ok(cache) = USER_CACHE.as_ref()
            && let Some(profile) = cache.get(name)
        {
            info!("Using Profile Cache");
            return Ok(toml::from_str(profile)?);
        }

        if let Ok(cache) = SYSTEM_CACHE.as_ref()
            && let Some(profile) = cache.get(name)
        {
            info!("Using System Cache");
            return Ok(toml::from_str(profile)?);
        }

        Err(Error::NotFound(
            name.to_string(),
            Cow::Borrowed("No such profile"),
        ))
    }

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
            profile.features.extend(features)
        }

        if let Some(inherits) = args.inherits.take() {
            profile.inherits = Some(inherits.into_iter().collect())
        }

        if let Some(conflicts) = args.conflicts.take() {
            profile.conflicts.extend(conflicts)
        }

        if let Some(binaries) = args.binaries.take() {
            profile.binaries.extend(binaries)
        }

        if let Some(libraries) = args.libraries.take() {
            profile.libraries.extend(libraries)
        }

        if let Some(devices) = args.devices.take() {
            profile.devices.extend(devices)
        }

        if let Some(namespaces) = args.namespaces.take() {
            profile.namespaces.extend(namespaces)
        }

        profile.files = files::Files::from_args(args);
        profile.ipc = ipc::Ipc::from_args(args);

        if let Some(env) = args.env.take() {
            let environment = &mut profile.environment;
            env.into_iter().for_each(|pair| {
                if let Some((key, value)) = pair.split_once('=') {
                    environment.insert(key.to_string(), value.to_string());
                }
            });
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

        fab::features::fabricate(&mut profile, "cmdline")?;
        Ok(profile)
    }

    /// Load a new profile from all supported locations.
    pub fn new(name: &str, config: Option<String>) -> Result<Profile, Error> {
        debug!("Loading {name}");

        let mut profile = timer!("::load", Self::load(name))?;
        if name == "default" {
            return Ok(profile);
        }

        let hash = timer!("::hash", {
            let mut hash = profile.hash_str()?;
            if let Some(config) = &config {
                hash += config;
            }
            hash += &CONFIG_FILE.system_mode().to_string();
            hash
        });

        if let Ok(cache) = timer!("::hash_fetch", HASH_CACHE.as_ref())
            && let Some(profile) = cache.get(&hash)
        {
            debug!("Using cached profile");
            return Ok(timer!("::cache_parse", toml::from_str(profile)?));
        }

        debug!("No cache available");
        let to_inherit: ISet<String> = match &profile.inherits {
            Some(i) => i.clone(),
            None => {
                if !CONFIG_FILE.system_mode()
                    && db::exists("default", Database::User, Table::Profiles)?
                {
                    ISet::from_iter(["default".to_string()])
                } else {
                    ISet::default()
                }
            }
        };

        for inherit in to_inherit {
            profile.merge(Profile::new(&inherit, None)?)?;
        }

        if let Some(config) = config {
            debug!("Loading configuration");
            if !profile.configuration.is_empty() {
                match profile.configuration.swap_remove(&config) {
                    Some(conf) => {
                        profile = profile.base(conf)?;
                    }
                    None => {
                        return Err(Error::NotFound(
                            name.to_string(),
                            Cow::Owned(format!("Configuration {config} does not exist")),
                        ));
                    }
                }
            } else {
                return Err(Error::NotFound(
                    name.to_string(),
                    Cow::Borrowed("Profile does not have any configurations"),
                ));
            }
        };

        if let Some(path) = &profile.path
            && path.starts_with("~")
        {
            profile.path = Some(path.replace("~", HOME.as_str()))
        }

        // Try and lookup the path. If it doesn't work, then the corresponding application
        // isn't installed. This is fine, as long as the user doesn't try and run the profile.
        if !name.ends_with(".toml") && profile.path.is_none() {
            profile.path = Some(which::which(&profile.app_path(name))?.to_string());
        }

        debug!("Fabricating features");
        fab::features::fabricate(&mut profile, name)?;
        db::save(&hash, &profile, Database::Cache, Table::Profiles)?;
        Ok(profile)
    }

    /// Use another profile as the base for the caller.
    /// This function effectively inverts the logic of `merge`:
    /// The values in the source take precedent, either appending or
    /// overwriting the caller.
    ///
    /// The difference is that values unaffected by `merge` persist in
    /// the caller.
    pub fn base(mut self, mut source: Self) -> Result<Self, Error> {
        source.id = self.id.take();
        source.inherits = self.inherits.take();

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

        if self.seccomp.is_none() {
            self.seccomp = profile.seccomp;
        }

        if self.new_privileges.is_none() {
            self.new_privileges = profile.new_privileges;
        }

        if let Some(home) = profile.home {
            if let Some(s_home) = &mut self.home {
                s_home.merge(home)
            } else {
                self.home = Some(home)
            }
        }

        if let Some(files) = profile.files {
            if let Some(s_files) = &mut self.files {
                s_files.merge(files)
            } else {
                self.files = Some(files);
            }
        }

        if let Some(ipc) = profile.ipc {
            if let Some(s_ipc) = &mut self.ipc {
                s_ipc.merge(ipc)
            } else {
                self.ipc = Some(ipc);
            }
        }

        if let Some(hooks) = profile.hooks {
            if let Some(s_hooks) = &mut self.hooks {
                s_hooks.merge(hooks)
            } else {
                self.hooks = Some(hooks)
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
        self.libraries.extend(profile.libraries);
        self.devices.extend(profile.devices);
        self.features.extend(profile.features);
        self.conflicts.extend(profile.conflicts);
        self.arguments.append(&mut profile.arguments);
        self.sandbox_args.append(&mut profile.sandbox_args);
        Ok(())
    }

    pub fn app_path<'a>(&'a self, name: &'a str) -> Cow<'a, str> {
        match &self.path {
            Some(path) => Cow::Borrowed(path),
            None => match which(name) {
                Ok(path) => Cow::Borrowed(path),
                Err(_) => Cow::Borrowed(name),
            },
        }
    }

    /// Get the id name, using as the Flatpak ID.
    /// It's either the id name, or from name()
    pub fn desktop<'a, 'b>(&'b self, name: &'a str) -> Cow<'a, str>
    where
        'b: 'a,
    {
        if let Some(id) = &self.id {
            Cow::Borrowed(id)
        } else {
            let path = self.app_path(name);
            let bin_name = if let Some(i) = path.rfind('/') {
                let slice = &path[i + 1..];
                Cow::Owned(slice.to_string())
            } else {
                path
            };

            if bin_name.contains('.') {
                bin_name
            } else {
                Cow::Borrowed(name)
            }
        }
    }

    /// Format the id as the Flatpak ID.
    pub fn id(&self, name: &str) -> String {
        let id = self.desktop(name);
        if id.contains('.') {
            id.to_string()
        } else {
            format!("antimony.{id}")
        }
    }

    /// Get the numerical hash of the profile.
    /// Note that while *deserializing* from postcard throws an error,
    /// we can serialize it for the purposes of hashing.
    pub fn num_hash(&self) -> Result<u64, Error> {
        timer!("::hash", {
            Ok(RandomState::with_seeds(0, 0, 0, 0).hash_one(postcard::to_stdvec(&self)?))
        })
    }

    /// Get the Profile's hash.
    pub fn hash_str(&self) -> Result<String, Error> {
        Ok(format!("{}", self.num_hash()?))
    }

    /// Edit a profile.
    pub fn edit(path: &Path) -> Result<Option<()>, edit::Error> {
        edit::edit::<Self>(path)
    }
}
impl Info for Profile {
    /// Get information about a profile.
    fn info(&self, name: &str, verbose: u8) {
        print!(
            "{} => {} ",
            style(name).bold(),
            style(self.app_path(name)).italic()
        );
        print!("{} ", self.arguments.join(" "));

        if let Some(inherits) = &self.inherits {
            if inherits.is_empty() {
                print!("(No default)");
            } else {
                print!("(Inherits: {})", format_iter(inherits.iter()));
            }
        };
        println!();

        if verbose > 0 {
            println!(
                "\t- Allow New Privileges: {}",
                if self.new_privileges.unwrap_or(false) {
                    style("Yes").red()
                } else {
                    style("No").green()
                }
            );

            println!(
                "\t- Sandbox Arguments: {}",
                format_iter(self.sandbox_args.iter())
            );

            if let Some(id) = &self.id {
                println!("\t- ID: {id}");
            }

            println!(
                "\t- Required Features: {}",
                format_iter(self.features.iter())
            );

            println!(
                "\t- Conflicting Features: {}",
                format_iter(self.conflicts.iter())
            );

            if let Some(home) = &self.home {
                println!("\t- Home");
                home.info(name);
            }

            println!(
                "\t- SECCOMP: {}",
                match self.seccomp.unwrap_or_default() {
                    seccomp::SeccompPolicy::Permissive => style("Permissive").yellow(),
                    seccomp::SeccompPolicy::Enforcing => style("Enforcing").green(),
                    seccomp::SeccompPolicy::Notifying => style("Notify").blue(),
                    seccomp::SeccompPolicy::Disabled => style("Disabled").red(),
                }
            );

            if let Some(ipc) = &self.ipc {
                ipc.info();
            }

            if let Some(files) = &self.files {
                files.info()
            }

            println!("\t- Binaries: {}", format_iter(self.binaries.iter()));

            library_info(&self.libraries, verbose);

            println!("\t- Devices:");
            for device in &self.devices {
                println!("\t\t- {}", style(device).italic());
            }

            println!("\t- Namespaces: {}", format_iter(self.namespaces.iter()));

            println!("\t- Environment Variables:");
            for (key, value) in &self.environment {
                println!("\t\t - {key} = {value}");
            }

            println!(
                "\t- Configurations: {}",
                self.configuration
                    .keys()
                    .map(|k| k.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            );

            if let Some(hooks) = &self.hooks {
                hooks.info();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::env::AT_HOME;

    #[test]
    fn validate_profiles() {
        let profiles = Path::new(AT_HOME.as_path()).join("profiles");
        if profiles.exists() {
            for path in fs::read_dir(profiles)
                .expect("No profiles to test")
                .filter_map(|e| e.ok())
            {
                toml::from_str::<Profile>(
                    &fs::read_to_string(path.path()).expect("Failed to read profile"),
                )
                .expect("Failed to parse profile");
            }
        }
    }
}
