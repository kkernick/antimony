use crate::{
    cli,
    shared::{Map, Set, extend_option},
};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fmt;

/// How a file should be exposed in the sandbox.
#[derive(Hash, Default, Eq, Deserialize, Serialize, PartialEq, Clone, Copy, ValueEnum, Debug)]
#[serde(deny_unknown_fields)]
pub enum FileMode {
    /// Only allow reads
    #[default]
    #[serde(rename = "ro")]
    ReadOnly = 0,

    /// Allow writes.
    #[serde(rename = "rw")]
    ReadWrite = 1,

    /// Executable files need to be created as copies, so that chmod will work
    /// correctly.
    #[serde(rename = "rx")]
    Executable = 2,
}
impl FileMode {
    /// Get the bwrap argument for binding this file.
    #[must_use]
    pub const fn bind(&self, can_try: bool) -> &'static str {
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
    #[must_use]
    pub const fn chmod(&self) -> &'static str {
        match self {
            Self::ReadOnly => "444",
            Self::ReadWrite => "666",
            Self::Executable => "555",
        }
    }
}
impl fmt::Display for FileMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
#[derive(Default, Deserialize, Serialize, PartialEq, Eq, Clone, Debug)]
#[serde(deny_unknown_fields, default)]
pub struct Files {
    /// The default mode for files passed through the command line. If no passthrough
    /// is provided, files are not passed. This includes using the application to open
    /// files in your file explorer or setting it as the default for particular MIME types.
    pub passthrough: Option<FileMode>,

    /// A list of paths in the sandbox to mount as TMPFS.
    pub temp: Option<Vec<String>>,

    /// User files assume a root of /home/antimony unless absolute.
    pub user: Option<FileList>,

    /// Platform files are device-specific system files (Locale, Configuration, etc)
    pub platform: Option<FileList>,

    /// Resource files are system files required by libraries/binaries.
    pub resources: Option<FileList>,

    /// Runtime files are system files with unstable paths (i.e environment variables
    /// Or files located on temporary file systems). These are normalized outside
    /// caching (With all the benefits and drawbacks that entails.)
    pub runtime: Option<FileList>,

    /// Direct files take a path, and file contents.
    pub direct: Option<Map<FileMode, Map<String, String>>>,

    /// Symlinks within the sandbox. Note that the source/destination should be
    /// normalized for the sandbox, not host (i.e /home/antimony). You will
    /// need to ensure the source exists in the sandbox (Either through a standard
    /// file passthrough or by adding the file to the profile home)
    pub links: Option<Map<String, String>>,

    /// A map of filesystem paths within the sandbox to Landlock permissions that are required.
    pub permissions: Option<Map<String, FileMode>>,
}
impl Files {
    /// Merge two file sets together.
    pub fn merge(&mut self, files: Self) {
        if files.passthrough.is_some() {
            self.passthrough = files.passthrough;
        }

        #[allow(clippy::items_after_statements)]
        type FileMap = Map<FileMode, Set<String>>;
        let file_map = |lhs: &mut Option<FileMap>, rhs: Option<FileMap>| {
            if let Some(mut rhs) = rhs {
                match lhs {
                    Some(lhs) => {
                        for mode in FILE_MODES {
                            if let Some(map) = rhs.remove(&mode) {
                                lhs.entry(mode).or_default().extend(map);
                            }
                        }
                    }
                    None => *lhs = Some(rhs),
                }
            }
        };

        file_map(&mut self.user, files.user);
        file_map(&mut self.platform, files.platform);
        file_map(&mut self.resources, files.resources);
        file_map(&mut self.runtime, files.runtime);

        if let Some(mut direct) = files.direct {
            match &mut self.direct {
                Some(s_direct) => {
                    for mode in FILE_MODES {
                        if let Some(map) = direct.remove(&mode) {
                            s_direct.entry(mode).or_default().extend(map);
                        }
                    }
                }
                None => self.direct = Some(direct),
            }
        }

        extend_option(&mut self.temp, files.temp);
        extend_option(&mut self.links, files.links);
        extend_option(&mut self.permissions, files.permissions);
    }

    /// Construct a file set from the command line.
    pub fn from_args(args: &mut cli::run::Args) -> Option<Self> {
        let mut files: Option<Self> = None;

        if let Some(passthrough) = args.file_passthrough.take() {
            files.get_or_insert_default().passthrough = Some(passthrough);
        }
        if let Some(ro) = args.ro.take() {
            let files = files.get_or_insert_default();
            for file in ro {
                let list = if file.starts_with("/home") {
                    files.user.get_or_insert_default()
                } else {
                    files.platform.get_or_insert_default()
                };
                list.entry(FileMode::ReadOnly).or_default().insert(file);
            }
        }
        if let Some(rw) = args.rw.take() {
            let files = files.get_or_insert_default();
            for file in rw {
                let list = if file.starts_with("/home") {
                    files.user.get_or_insert_default()
                } else {
                    files.platform.get_or_insert_default()
                };
                list.entry(FileMode::ReadWrite).or_default().insert(file);
            }
        }

        if let Some(mut temp) = args.temp.take() {
            let files = files.get_or_insert_default();
            files.temp.get_or_insert_default().append(&mut temp);
        }

        if let Some(link) = args.link.take() {
            let links = files.get_or_insert_default().links.get_or_insert_default();
            for pair in link {
                if let Some((src, dst)) = pair.split_once('=') {
                    links.insert(src.to_owned(), dst.to_owned());
                }
            }
        }

        files
    }
}
