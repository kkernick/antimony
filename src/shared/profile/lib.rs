use crate::{
    cli,
    shared::{Set, extend_option},
};
use serde::{Deserialize, Serialize};

#[derive(Default, Deserialize, Serialize, PartialEq, Eq, Clone, Debug)]
#[serde(deny_unknown_fields, default)]
pub struct Libraries {
    pub no_sof: Option<bool>,

    /// The library roots to search. Borrows the definitions from /etc/antimony.conf
    pub roots: Option<Set<String>>,

    /// Files and Wildcards matching files.
    pub files: Option<Set<String>>,

    /// Directories and Wildcards matching directories.
    pub directories: Option<Set<String>>,
}
impl Libraries {
    /// Merge two file set together.
    pub fn merge(&mut self, libraries: Self) {
        if self.no_sof.is_none() {
            self.no_sof = libraries.no_sof;
        }

        extend_option(&mut self.roots, libraries.roots);
        extend_option(&mut self.files, libraries.files);
        extend_option(&mut self.directories, libraries.directories);
    }

    /// Construct a file set from the command line.
    pub fn from_args(args: &mut cli::run::Args) -> Option<Self> {
        let mut ret: Option<Self> = None;
        if let Some(files) = args.libraries.take() {
            ret.get_or_insert_default()
                .files
                .get_or_insert_default()
                .extend(files);
        }
        if let Some(directories) = args.directories.take() {
            ret.get_or_insert_default()
                .directories
                .get_or_insert_default()
                .extend(directories);
        }
        if let Some(roots) = args.roots.take() {
            ret.get_or_insert_default()
                .roots
                .get_or_insert_default()
                .extend(roots);
        }
        if args.no_sof {
            ret.get_or_insert_default().no_sof = Some(true);
        }
        ret
    }
}
