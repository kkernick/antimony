use crate::{
    cli,
    fab::resolve,
    shared::{Map, Set},
};
use clap::ValueEnum;
use console::style;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// How a file should be exposed in the sandbox.
#[derive(Debug, Hash, Default, Eq, Deserialize, Serialize, PartialEq, Clone, Copy, ValueEnum)]
#[serde(deny_unknown_fields)]
pub enum FileMode {
    /// Only allow reads
    #[default]
    #[serde(rename = "ro")]
    ReadOnly,

    /// Allow writes.
    #[serde(rename = "rw")]
    ReadWrite,

    /// Executable files need to be created as copies, so that chmod will work
    /// correctly.
    #[serde(rename = "rx")]
    Executable,
}
impl FileMode {
    /// Get the bwrap argument for binding this file.
    pub fn bind(&self, can_try: bool) -> &'static str {
        match self {
            Self::ReadWrite => {
                if can_try {
                    "--bind-try"
                } else {
                    "--bind"
                }
            }
            _ => {
                if can_try {
                    "--ro-bind-try"
                } else {
                    "--ro-bind"
                }
            }
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
impl std::fmt::Display for FileMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadOnly => write!(f, "ro"),
            Self::ReadWrite => write!(f, "rw"),
            Self::Executable => write!(f, "rx"),
        }
    }
}

/// Why use strum when we can just make a static array?
pub static FILE_MODES: [FileMode; 3] = [
    FileMode::Executable,
    FileMode::ReadOnly,
    FileMode::ReadWrite,
];

/// For each file mode, we have a list of files.
pub type FileList = Map<FileMode, Set<String>>;

/// Files, RO/RW, and Modes.
#[derive(Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct Files {
    /// The default mode for files passed through the command line. If no passthrough
    /// is provided, files are not passed. This includes using the application to open
    /// files in your file explorer or setting it as the default for particular MIME types.
    pub passthrough: Option<FileMode>,

    /// User files assume a root of /home/antimony unless absolute.
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub user: FileList,

    /// Platform files are device-specific system files (Locale, Configuration, etc)
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub platform: FileList,

    /// Resource files are system files required by libraries/binaries.
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub resources: FileList,

    /// Direct files take a path, and file contents.
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub direct: Map<FileMode, Map<String, String>>,
}
impl Files {
    /// Merge two file sets together.
    pub fn merge(&mut self, files: Self) {
        if files.passthrough.is_some() {
            self.passthrough = files.passthrough;
        }

        let mut user = files.user;
        let s_user = &mut self.user;
        for mode in FILE_MODES {
            if let Some(map) = user.swap_remove(&mode) {
                s_user.entry(mode).or_default().extend(map);
            }
        }

        let mut sys = files.platform;
        let s_user = &mut self.platform;

        for mode in FILE_MODES {
            if let Some(map) = sys.swap_remove(&mode) {
                s_user.entry(mode).or_default().extend(map);
            }
        }

        let mut sys = files.resources;
        let s_user = &mut self.resources;
        for mode in FILE_MODES {
            if let Some(map) = sys.swap_remove(&mode) {
                s_user.entry(mode).or_default().extend(map);
            }
        }

        let mut direct = files.direct;
        let s_user = &mut self.direct;
        for mode in FILE_MODES {
            if let Some(map) = direct.swap_remove(&mode) {
                s_user.entry(mode).or_default().extend(map);
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
                    &mut files.user
                } else {
                    &mut files.platform
                };
                list.entry(FileMode::ReadOnly).or_default().insert(file);
            });
        }
        if let Some(rw) = args.rw.take() {
            let files = files.get_or_insert_default();
            rw.into_iter().for_each(|file| {
                let list = if file.starts_with("/home") {
                    &mut files.user
                } else {
                    &mut files.platform
                };
                list.entry(FileMode::ReadWrite).or_default().insert(file);
            });
        }

        files
    }

    /// Get info about the installed files.
    pub fn info(&self) {
        let get_files = |list: &FileList, mode| -> Set<String> {
            let mut ret = Set::default();
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

        for mode in FILE_MODES {
            let mut files = Set::default();
            files.extend(get_files(&self.platform, mode));
            files.extend(get_files(&self.resources, mode));
            files.extend(get_files(&self.user, mode));

            if let Some(mode_files) = self.direct.get(&mode) {
                for file in mode_files.keys() {
                    files.insert(format!("\t\t- {}", style(file).italic()));
                }
            }
            if !files.is_empty() {
                println!("\t- {mode} Files:");
                files.into_iter().for_each(|file| println!("{file}"))
            }
        }
    }
}
