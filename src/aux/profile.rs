//! The Profile defines what application should be run, and what features it needs
//! to function properly.
use crate::aux::edit;
use crate::aux::env::{AT_HOME, PWD, USER_NAME};
use crate::aux::path::which_exclude;
use crate::fab::lib::get_wildcards;
use crate::fab::resolve;
use crate::{cli, fab};
use clap::ValueEnum;
use console::style;
use log::debug;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs::File;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

/// An error for issues around Profiles.
#[derive(Debug)]
pub enum Error {
    /// When a profile doesn't exist.
    NotFound(String, &'static str),

    /// When the profile cannot be Deserialized.
    Deserialize(toml::de::Error),

    /// When the profile cannot be Serialized.
    Serialize(toml::ser::Error),

    /// Misc IO errors.
    Io(&'static str, std::io::Error),

    /// Misc Errno errors.
    Errno(&'static str, nix::errno::Errno),

    /// Errors resolving/creating paths.
    Path(which::Error),

    /// Errors for profile arguments specified on the command line.
    CommandLine(&'static str, String, Vec<String>),

    /// Errors incorporating features.
    Feature(crate::fab::features::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::NotFound(name, reason) => write!(
                f,
                "Profile {name} not found: {reason}. \
                Check the path, or create one with `antimony create`"
            ),
            Self::Deserialize(e) => write!(f, "Failed to read profile: {e}"),
            Self::Serialize(e) => write!(f, "Failed to write profile: {e}"),
            Self::Io(what, e) => write!(f, "Failed to {what}: {e}"),
            Self::Errno(what, e) => write!(f, "{what} failed: {e}"),
            Self::Path(e) => write!(f, "Path error: {e}"),
            Self::Feature(e) => write!(f, "Failed to resolve feature: {e}"),
            Self::CommandLine(arg, value, valid) => {
                write!(
                    f,
                    "Unrecognized value for {arg}: {value}. Expected one of {valid:?}"
                )
            }
        }
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Deserialize(e) => Some(e),
            Self::Serialize(e) => Some(e),
            Self::Io(_, e) => Some(e),
            Self::Errno(_, e) => Some(e),
            Self::Path(e) => Some(e),
            Self::Feature(e) => Some(e),
            _ => None,
        }
    }
}
impl From<which::Error> for Error {
    fn from(val: which::Error) -> Self {
        Error::Path(val)
    }
}
impl From<toml::de::Error> for Error {
    fn from(val: toml::de::Error) -> Self {
        Error::Deserialize(val)
    }
}
impl From<toml::ser::Error> for Error {
    fn from(val: toml::ser::Error) -> Self {
        Error::Serialize(val)
    }
}
impl From<crate::fab::features::Error> for Error {
    fn from(val: crate::fab::features::Error) -> Self {
        Error::Feature(val)
    }
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

fn extend<T>(s: &mut Option<BTreeSet<T>>, p: Option<BTreeSet<T>>)
where
    T: Ord,
{
    if let Some(source) = p {
        if let Some(dest) = s {
            dest.extend(source);
        } else {
            *s = Some(source);
        }
    }
}

/// The definitions needed to sandbox an application.
#[derive(Debug, Hash, Deserialize, Serialize, PartialEq, Eq, Default)]
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
    pub features: Option<BTreeSet<String>>,

    /// Features that should be excluded from running under the profile.
    pub conflicts: Option<BTreeSet<String>>,

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
    pub inherits: Option<BTreeSet<String>>,

    /// Configuration for the profile's home.
    pub home: Option<Home>,

    /// The SECCOMP policy dictates whether to use SECCOMP to constrain the sandbox.
    pub seccomp: Option<SeccompPolicy>,

    /// IPC communication through D-Bus mediated via xdg-dbus-proxy.
    pub ipc: Option<Ipc>,

    /// Files passed to the sandbox. System files are canonicalized at the sandbox root,
    /// Home files are canonicalized at /home/antimony
    pub files: Option<Files>,

    /// Binaries needed in the sandbox.
    pub binaries: Option<BTreeSet<String>>,

    /// Libraries needed in the sandbox. They can be listed as:
    /// 1. Files (eg /usr/lib/lib.so)
    /// 2. Directories (eg /usr/lib/mylib) to which all contents will be resolved
    /// 3. Wildcards (eg lib*), which can match directories and files.
    pub libraries: Option<BTreeSet<String>>,

    /// Devices needed in the sandbox, at /dev.
    pub devices: Option<BTreeSet<String>>,

    /// Namespaces, such as User and Net.
    pub namespaces: Option<BTreeSet<Namespace>>,

    /// Environment Variable Keypairs
    pub environment: Option<BTreeMap<String, String>>,

    /// Arguments to pass to the sandboxed application directly.
    pub arguments: Option<Vec<String>>,

    /// Configurations act as embedded profiles, inheriting the main one.
    pub configuration: Option<BTreeMap<String, Profile>>,
}
impl Profile {
    /// Get where the profile's user location is.
    /// When a user creates a new profile, or modifies a system one,
    /// this is the location is it stored.
    pub fn user_profile(name: &str) -> PathBuf {
        AT_HOME
            .join("config")
            .join(USER_NAME.as_str())
            .join("profiles")
            .join(name)
            .with_extension("toml")
    }

    /// Get where the profile's system location is.
    pub fn system_profile(name: &str) -> PathBuf {
        AT_HOME.join("profiles").join(name).with_extension("toml")
    }

    /// Get the location of the user's default profile.
    pub fn default_profile() -> PathBuf {
        AT_HOME
            .join("config")
            .join(USER_NAME.as_str())
            .join("default.toml")
    }

    /// Get the path of a profile.
    pub fn path(name: &str) -> Result<PathBuf, Error> {
        if name == "default" {
            let path = Self::default_profile();
            if !path.exists() {
                std::fs::copy(AT_HOME.join("config").join("default.toml"), &path)
                    .map_err(|e| Error::Io("Failed to create default profile", e))?;
            }
            return Ok(path);
        }

        // Try and load a file absolutely if the file is given.
        if name.ends_with(".toml") {
            let path = PathBuf::from(name);
            if path.exists() {
                return Ok(path);
            }
        }

        // Try and load user-configured profile from AT_HOME
        let user = Self::user_profile(name);
        if user.exists() {
            return Ok(user);
        }

        // Try and load the system profile from AT_HOME
        let system = Self::system_profile(name);
        if system.exists() {
            return Ok(system);
        }

        // Try and load the system profile from AT_HOME
        let local = PWD.join("config").join("profiles").join(name);
        if local.exists() {
            return Ok(local);
        }

        Err(Error::NotFound(name.to_string(), "No such profile"))
    }

    /// Construct a profile from the command line.
    /// Technically, everything needed for a profile can be specified
    /// from the command line that is needed to run a profile, so
    /// this can be used to either supplement a profile, or run applications
    /// without profiles defined.
    ///
    /// You probably shouldn't use this as the default way of running stuff,
    /// however.
    pub fn from_args(args: &mut cli::run::Args) -> Self {
        let mut profile = Self {
            seccomp: args.seccomp.take(),
            arguments: args.passthrough.take(),
            ..Default::default()
        };

        if let Some(features) = args.features.take() {
            profile.features = Some(features.into_iter().collect())
        }

        if let Some(conflicts) = args.conflicts.take() {
            profile.conflicts = Some(conflicts.into_iter().collect())
        }

        if let Some(inherits) = args.inherits.take() {
            profile.inherits = Some(inherits.into_iter().collect())
        }

        if let Some(binaries) = args.binaries.take() {
            profile.binaries = Some(binaries.into_iter().collect())
        }

        if let Some(libraries) = args.libraries.take() {
            profile.libraries = Some(libraries.into_iter().collect())
        }

        if let Some(devices) = args.devices.take() {
            profile.devices = Some(devices.into_iter().collect())
        }

        if let Some(namespaces) = args.namespaces.take() {
            profile.namespaces = Some(namespaces.into_iter().collect())
        }

        profile.files = Files::from_args(args);
        profile.ipc = Ipc::from_args(args);

        if let Some(env) = args.env.take() {
            let environment = profile.environment.get_or_insert_default();
            env.into_iter().for_each(|pair| {
                if let Some((key, value)) = pair.split_once('=') {
                    environment.insert(key.to_string(), value.to_string());
                }
            });
        }
        profile
    }

    /// Load a new profile from all supported locations.
    pub fn new(name: &str) -> Result<Profile, Error> {
        debug!("Loading {name}");
        let profile = std::fs::read_to_string(Profile::path(name)?)
            .map_err(|e| Error::Io("read profile", e))?;

        let mut profile: Profile = toml::from_str(profile.as_str())?;

        let to_inherit: BTreeSet<String> = match &profile.inherits {
            Some(i) => i.clone(),
            None => {
                if Profile::default_profile().exists() {
                    BTreeSet::from_iter(["default".to_string()])
                } else {
                    BTreeSet::new()
                }
            }
        };

        for inherit in to_inherit {
            profile.merge(toml::from_str(
                &std::fs::read_to_string(Profile::path(&inherit)?)
                    .map_err(|e| Error::Io("read inherited profile", e))?,
            )?)?;
        }

        // Try and lookup the path. If it doesn't work, then the corresponding application
        // isn't installed. This is fine, as long as the user doesn't try and run the profile.
        profile.path = Some(which_exclude(profile.app_path(name))?);

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
        source.path = self.path.clone();
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
    pub fn merge(&mut self, profile: Self) -> Result<(), Error> {
        if self.seccomp.is_none() {
            self.seccomp = profile.seccomp;
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

        if let Some(env) = profile.environment {
            if let Some(s_env) = &mut self.environment {
                s_env.extend(env)
            } else {
                self.environment = Some(env);
            }
        }

        if let Some(ipc) = profile.ipc {
            if let Some(s_ipc) = &mut self.ipc {
                s_ipc.merge(ipc)
            } else {
                self.ipc = Some(ipc);
            }
        }

        if let Some(configs) = profile.configuration {
            for (name, config) in configs {
                self.configuration
                    .get_or_insert_default()
                    .insert(name, config);
            }
        }

        extend(&mut self.namespaces, profile.namespaces);
        extend(&mut self.binaries, profile.binaries);
        extend(&mut self.libraries, profile.libraries);
        extend(&mut self.devices, profile.devices);
        extend(&mut self.features, profile.features);
        extend(&mut self.conflicts, profile.conflicts);
        append(&mut self.arguments, profile.arguments);
        Ok(())
    }

    pub fn app_path<'a>(&'a self, name: &'a str) -> &'a str {
        match &self.path {
            Some(path) => path,
            None => name,
        }
    }

    /// Get the id name, using as the Flatpak ID.
    /// It's either the id name, or from name()
    pub fn desktop<'a, 'b>(&'b self, name: &'a str) -> &'a str
    where
        'b: 'a,
    {
        if let Some(id) = &self.id {
            id
        } else {
            let path = self.app_path(name);
            let bin_name = if let Some(i) = path.rfind('/') {
                &path[i + 1..]
            } else {
                path
            };

            if bin_name.contains('.') {
                bin_name
            } else {
                name
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

    /// Get the Profile's hash.
    pub fn hash_str(&self) -> String {
        let mut s = DefaultHasher::new();
        self.hash(&mut s);
        format!("{}", s.finish())
    }

    /// Get information about a profile.
    pub fn info(&self, name: &str, verbose: u8) {
        print!(
            "{} => {} ",
            style(name).bold(),
            style(self.app_path(name)).italic()
        );
        if let Some(args) = &self.arguments {
            print!("{} ", args.join(" "))
        }

        if let Some(inherits) = &self.inherits {
            if inherits.is_empty() {
                print!("(No default)");
            } else {
                print!("(Inherits: {inherits:?})");
            }
        };
        println!();

        if verbose > 0 {
            if let Some(id) = &self.id {
                println!("\t- ID: {id}");
            }

            if let Some(features) = &self.features {
                println!("\t- Required Features: {features:?}");
            }

            if let Some(conflicts) = &self.conflicts {
                println!("\t- Conflicting Features: {conflicts:?}");
            }

            if let Some(home) = &self.home {
                println!("\t- Home");
                home.info(name);
            }

            println!(
                "\t- SECCOMP: {}",
                match self.seccomp.unwrap_or_default() {
                    SeccompPolicy::Permissive => style("Permissive").yellow(),
                    SeccompPolicy::Enforcing => style("Enforcing").green(),
                    SeccompPolicy::Disabled => style("Disabled").red(),
                }
            );

            if let Some(ipc) = &self.ipc {
                ipc.info();
            }

            if let Some(files) = &self.files {
                files.info()
            }

            if let Some(binaries) = &self.binaries {
                println!("\t- Binaries: {binaries:?}");
            }

            if let Some(libraries) = &self.libraries {
                library_info(libraries, verbose);
            }

            if let Some(devices) = &self.devices {
                println!("\t- Devices:");
                for device in devices {
                    println!("\t\t- {}", style(device).italic());
                }
            }

            if let Some(namespaces) = &self.namespaces {
                println!(
                    "\t- Namespaces: {}",
                    namespaces
                        .iter()
                        .map(|e| format!("{e:?}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                );
            }

            if let Some(envs) = &self.environment {
                println!("\t- Environment Variables:");
                for (key, value) in envs {
                    println!("\t\t - {key} = {value}");
                }
            }

            if let Some(configs) = &self.configuration {
                println!(
                    "\t- Configurations: {}",
                    configs
                        .keys()
                        .map(|k| k.to_string())
                        .collect::<Vec<_>>()
                        .join(" ")
                );
            }
        }
    }

    /// Integrate features into the profile, populating all fields with information
    /// from all dependencies.
    pub fn integrate(mut self, name: &str, user_cache: &Path) -> Result<Profile, Error> {
        let feature_cache = user_cache.join("profile.cache");
        if feature_cache.exists() {
            self = toml::from_str(
                &std::fs::read_to_string(feature_cache)
                    .map_err(|e| Error::Io("read feature cache", e))?,
            )?;
        } else {
            fab::features::fabricate(&mut self, name)?;
            std::fs::create_dir_all(user_cache).map_err(|e| Error::Io("write user cache", e))?;

            write!(
                File::create(feature_cache).map_err(|e| Error::Io("write feature cache", e))?,
                "{}",
                toml::to_string(&self)?
            )
            .map_err(|e| Error::Io("write feature cache", e))?;
        }
        Ok(self)
    }

    /// Edit a profile.
    pub fn edit(path: &Path) -> Result<Option<()>, edit::Error> {
        edit::edit::<Self>(path)
    }
}

/// Print info about the libraries used in a feature/profile.
pub fn library_info(libraries: &BTreeSet<String>, verbose: u8) {
    println!("\t- Libraries:");
    for library in libraries {
        if verbose > 2 && library.contains("*") {
            match get_wildcards(library) {
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

/// The SECCOMP Policy for the Profile
#[derive(Hash, Debug, Deserialize, Serialize, PartialEq, Eq, Copy, Clone, ValueEnum, Default)]
#[serde(deny_unknown_fields)]
pub enum SeccompPolicy {
    /// Disable SECCOMP
    #[default]
    Disabled,

    /// Syscalls are logged to construct a policy for the profile.
    Permissive,

    /// The policy is enforced: unrecognized syscalls return with EPERM.
    Enforcing,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq, Hash, Default)]
pub struct Home {
    pub name: Option<String>,
    pub policy: Option<HomePolicy>,
}
impl Home {
    pub fn merge(&mut self, home: Self) {
        if self.name.is_none() {
            self.name = home.name;
        }
        if self.policy.is_none() {
            self.policy = home.policy;
        }
    }

    pub fn from_args(args: &mut cli::run::Args) -> Self {
        Self {
            name: args.home_name.take(),
            policy: args.home_policy.take(),
        }
    }

    pub fn info(&self, name: &str) {
        println!(
            "\t\t- Home Path: ~/.local/share/antimony/{}",
            match &self.name {
                Some(name) => name,
                None => name,
            }
        );
        if let Some(policy) = &self.name {
            println!("\t\t- Home Policy: {policy}");
        }
    }
}

/// The Home Policy being set creates a persistent home folder for the profile.
#[derive(Hash, Debug, Deserialize, Serialize, PartialEq, Eq, Clone, Copy, ValueEnum, Default)]
#[serde(deny_unknown_fields)]
pub enum HomePolicy {
    /// Do not use a home profile.
    #[default]
    None,

    /// The Home Folder is passed read/write. Applications that only permit a single
    /// instance, such as Chromium, will get upset if you launch multiple instances of
    /// the sandbox.
    Enabled,

    /// Once an application has been configured, Overlay effectively freezes it in place by
    /// mounting it as a temporary overlay. Changes made in the sandbox are discarded, and
    /// it can be shared by multiple instances, even if that application doesn't typically
    /// support multiple instances (Zed, Chromium, etc).
    Overlay,
}

/// Files, RO/RW, and Modes.
#[derive(Hash, Default, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Files {
    /// The default mode for files passed through the command line. If no passthrough
    /// is provided, files are not passed. This includes using the application to open
    /// files in your file explorer or setting it as the default for particular MIME types.
    pub passthrough: Option<FileMode>,

    /// User files assume a root of /home/antimony unless absolute.
    pub user: Option<FileList>,

    /// System files are mounted at the sandbox root.
    pub system: Option<FileList>,

    /// Direct files take a path, and file contents.
    pub direct: Option<BTreeMap<FileMode, BTreeMap<String, String>>>,
}
impl Files {
    /// Merge two file sets together.
    pub fn merge(&mut self, mut files: Self) {
        if files.passthrough.is_some() {
            self.passthrough = files.passthrough;
        }

        if let Some(mut user) = files.user.take() {
            let s_user = self.user.get_or_insert_default();
            for mode in FileMode::iter() {
                if let Some(map) = user.remove(&mode) {
                    s_user
                        .get_mut(&mode)
                        .get_or_insert(&mut BTreeSet::new())
                        .extend(map);
                }
            }
        }

        if let Some(mut sys) = files.system.take() {
            let s_user = self.system.get_or_insert_default();
            for mode in FileMode::iter() {
                if let Some(map) = sys.remove(&mode) {
                    s_user
                        .get_mut(&mode)
                        .get_or_insert(&mut BTreeSet::new())
                        .extend(map);
                }
            }
        }

        if let Some(mut direct) = files.direct.take() {
            let s_user = self.direct.get_or_insert_default();
            for mode in FileMode::iter() {
                if let Some(map) = direct.remove(&mode) {
                    s_user
                        .get_mut(&mode)
                        .get_or_insert(&mut BTreeMap::new())
                        .extend(map);
                }
            }
        }
    }

    /// Construct a file set from the command line.
    pub fn from_args(args: &mut cli::run::Args) -> Option<Self> {
        let mut files: Option<Self> = None;

        if let Some(passthrough) = args.file_passthrough.take() {
            files.get_or_insert_default().passthrough = Some(passthrough)
        }
        if let Some(ro) = args.ro.take() {
            let files = files.get_or_insert_default();
            ro.into_iter().for_each(|file| {
                let list = if file.starts_with("/home") {
                    files.user.get_or_insert_default()
                } else {
                    files.system.get_or_insert_default()
                };
                list.entry(FileMode::ReadOnly).or_default().insert(file);
            });
        }
        if let Some(rw) = args.rw.take() {
            let files = files.get_or_insert_default();
            rw.into_iter().for_each(|file| {
                let list = if file.starts_with("/home") {
                    files.user.get_or_insert_default()
                } else {
                    files.system.get_or_insert_default()
                };
                list.entry(FileMode::ReadWrite).or_default().insert(file);
            });
        }

        files
    }

    /// Get info about the installed files.
    pub fn info(&self) {
        let get_files = |list: &FileList, mode| -> HashSet<String> {
            let mut ret = HashSet::new();
            if let Some(files) = list.get(&mode) {
                for file in files {
                    ret.insert(format!(
                        "\t\t- {}",
                        style(resolve(Cow::Borrowed(file))).italic()
                    ));
                }
            }
            ret
        };

        for mode in FileMode::iter() {
            let mut files = HashSet::new();
            if let Some(system) = &self.system {
                files.extend(get_files(system, mode));
            }
            if let Some(user) = &self.user {
                files.extend(get_files(user, mode));
            }
            if let Some(direct) = &self.direct
                && let Some(mode_files) = direct.get(&mode)
            {
                for file in mode_files.keys() {
                    files.insert(format!("\t\t- {}", style(file).italic()));
                }
            }
            if !files.is_empty() {
                println!("\t- {mode:?} Files:");
                files.into_iter().for_each(|file| println!("{file}"))
            }
        }
    }
}

/// Files can either be passed Read-Only, or ReadWrite.
/// Note that some applications, particularly KDE,
/// do not write to the file directly; they make a copy in the same directory
/// (In this case the sandbox), then *move* it from source to destination to replace it.
/// However, because Antimony provides file as bind mounts, this operation will fail.
/// In such cases, use portals, put the file directly in the profile's home folder, or
/// pass the parent folder.
pub type FileList = BTreeMap<FileMode, BTreeSet<String>>;

#[derive(
    Hash,
    Default,
    Debug,
    Eq,
    Deserialize,
    Serialize,
    PartialEq,
    PartialOrd,
    Ord,
    EnumIter,
    Clone,
    Copy,
    ValueEnum,
)]
#[serde(deny_unknown_fields)]
pub enum FileMode {
    #[default]
    ReadOnly,
    ReadWrite,

    /// Executable files need to be created as copies, so that chmod will work
    /// correctly.
    Executable,
}
impl FileMode {
    /// Get the bwrap argument for binding this file.
    pub fn bind(&self) -> &'static str {
        match self {
            Self::ReadWrite => "--bind-try",
            _ => "--ro-bind-try",
        }
    }

    /// Get the chmod value that should be used for direct files.
    pub fn chmod(&self) -> &'static str {
        match self {
            Self::ReadOnly => "444",
            Self::ReadWrite => "666",
            Self::Executable => "555",
        }
    }
}

/// IPC mediated via xdg-dbus-proxy.
#[derive(Hash, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct Ipc {
    /// Disable all IPC, regardless of what has been set.
    pub disable: Option<bool>,

    /// Provide the system bus. Defaults to false
    pub system_bus: Option<bool>,

    /// Provide the user bus directly. xdg-dbus-proxy is not run. Defaults to false.
    pub user_bus: Option<bool>,

    /// Freedesktop portals.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub portals: BTreeSet<Portal>,

    /// Busses that the sandbox can see, but not interact with.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub see: BTreeSet<String>,

    /// Busses the sandbox can talk over.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub talk: BTreeSet<String>,

    /// Busses the sandbox owns.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub own: BTreeSet<String>,

    /// Call semantics.
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub call: BTreeSet<String>,
}
impl Ipc {
    /// Merge two IPC sets together.
    pub fn merge(&mut self, mut ipc: Self) {
        if self.disable.is_none() {
            self.disable = ipc.disable;
        }

        if self.system_bus.is_none() {
            self.system_bus = ipc.system_bus;
        }
        if self.user_bus.is_none() {
            self.user_bus = ipc.user_bus;
        }

        self.portals.append(&mut ipc.portals);
        self.see.append(&mut ipc.see);
        self.talk.append(&mut ipc.talk);
        self.own.append(&mut ipc.own);
        self.call.append(&mut ipc.call);
    }

    /// Construct an IPC set from the command line.
    pub fn from_args(args: &mut cli::run::Args) -> Option<Self> {
        let mut ipc: Option<Self> = None;

        if let Some(portals) = args.portals.take() {
            ipc.get_or_insert_default().portals = portals.into_iter().collect();
        };
        if let Some(see) = args.see.take() {
            ipc.get_or_insert_default().see = see.into_iter().collect();
        };
        if let Some(talk) = args.talk.take() {
            ipc.get_or_insert_default().talk = talk.into_iter().collect();
        };
        if let Some(own) = args.own.take() {
            ipc.get_or_insert_default().own = own.into_iter().collect();
        };
        if let Some(call) = args.call.take() {
            ipc.get_or_insert_default().call = call.into_iter().collect();
        };

        if args.user_bus {
            ipc.get_or_insert_default().user_bus = Some(true);
        }
        if args.system_bus {
            ipc.get_or_insert_default().system_bus = Some(true);
        }
        if args.disable_ipc {
            ipc.get_or_insert_default().disable = Some(true);
        }

        ipc
    }

    /// Get info about the IPC set.
    pub fn info(&self) {
        println!("\t- IPC mediated via xdg-dbus-proxy");
        if !self.portals.is_empty() {
            println!(
                "\t\t- Portals: {}",
                self.portals
                    .iter()
                    .map(|e| format!("{e:?}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
        if !self.talk.is_empty() {
            println!("\t\t- Talk: {:?}", self.talk);
        }
        if !self.see.is_empty() {
            println!("\t\t- Visible: {:?}", self.see);
        }
        if !self.own.is_empty() {
            println!("\t\t- Owns: {:?}", self.own);
        }
        if !self.call.is_empty() {
            println!("\t\t- Calls via: {:?}", self.call);
        }
    }
}

/// A non-exhaustive list of Portals. Some may not be
/// implemented for certain Desktop Environments.
/// Not all applications use portals, even if they
/// are provided to the sandbox.
#[derive(
    Debug, Eq, Hash, PartialEq, Deserialize, Serialize, EnumIter, ValueEnum, Clone, PartialOrd, Ord,
)]
#[serde(deny_unknown_fields)]
pub enum Portal {
    Background,
    Camera,
    Clipboard,
    Documents,
    FileChooser,
    Flatpak,
    GlobalShortcuts,
    Inhibit,
    Location,
    Notification,
    OpenURI,
    ProxyResolver,
    Realtime,
    ScreenCast,
    Screenshot,
    Settings,
    Secret,
    NetworkMonitor,
}

/// Namespaces. By default, none are shared. You will likely not need to use these
/// directly, as they are included in relevant features.
#[derive(
    Default, Debug, Eq, Hash, PartialEq, Deserialize, Serialize, ValueEnum, Clone, PartialOrd, Ord,
)]
#[serde(deny_unknown_fields)]
pub enum Namespace {
    #[default]
    None,
    All,

    /// The user namespace is needed to create additional sandboxes (Such as chromium)
    User,

    Ipc,
    Pid,

    /// Use the network feature instead.
    Net,

    Uts,
    CGroup,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_profiles() {
        let profiles = Path::new(AT_HOME.as_path()).join("profiles");
        if profiles.exists() {
            for path in std::fs::read_dir(profiles)
                .expect("No profiles to test")
                .filter_map(|e| e.ok())
            {
                toml::from_str::<Profile>(
                    &std::fs::read_to_string(path.path()).expect("Failed to read profile"),
                )
                .expect("Failed to parse profile");
            }
        }
    }
}
