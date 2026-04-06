use crate::{cli, shared::StableSet};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct Libraries {
    pub no_sof: Option<bool>,

    /// The library roots to search. Borrows the definitions from /etc/antimony.conf
    #[serde(skip_serializing_if = "StableSet::is_empty")]
    pub roots: StableSet<String>,

    /// Files and Wildcards matching files.
    #[serde(skip_serializing_if = "StableSet::is_empty")]
    pub files: StableSet<String>,

    /// Directories and Wildcards matching directories.
    #[serde(skip_serializing_if = "StableSet::is_empty")]
    pub directories: StableSet<String>,
}
impl Libraries {
    /// Merge two file set together.
    pub fn merge(&mut self, libraries: Self) {
        if self.no_sof.is_none() {
            self.no_sof = libraries.no_sof
        }

        self.roots.extend(libraries.roots);
        self.files.extend(libraries.files);
        self.directories.extend(libraries.directories);
    }

    /// Construct a file set from the command line.
    pub fn from_args(args: &mut cli::run::Args) -> Option<Self> {
        let mut ret: Option<Self> = None;
        if let Some(files) = args.libraries.take() {
            ret.get_or_insert_default().files.extend(files);
        }
        if let Some(directories) = args.directories.take() {
            ret.get_or_insert_default().directories.extend(directories);
        }
        if let Some(roots) = args.roots.take() {
            ret.get_or_insert_default().roots.extend(roots);
        }
        if args.no_sof {
            ret.get_or_insert_default().no_sof = Some(true)
        }
        ret
    }
}
