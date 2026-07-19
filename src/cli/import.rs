//! Import user-profiles

use crate::shared::{
    feature::{self, Feature},
    privileged,
    profile::{self, Profile},
    store::{self, Object},
    syscalls,
};
use anyhow::anyhow;
use clap::ValueHint;
use log::warn;
use std::path::Path;

#[derive(clap::Args)]
pub struct Args {
    /// The path of the object. Can also be a directory, which will import all files within.
    #[arg(value_hint = ValueHint::FilePath)]
    name: String,

    /// Target the feature set rather than the profile set.
    #[arg(short, long)]
    feature: bool,

    /// Target the system set rather than the user set.
    #[arg(short, long)]
    pub system: bool,

    /// Import data into the SECCOMP database. Overrides --feature and --system.
    #[arg(long)]
    pub seccomp: bool,
}
impl super::Run for Args {
    fn run(self) -> anyhow::Result<()> {
        if self.seccomp {
            if privileged()? {
                return syscalls::merge_database(Path::new(&self.name));
            }
            return Err(anyhow::anyhow!(
                "Modifying the SECCOMP database is a privileged operation"
            ));
        }

        let (table, kind) = if self.feature {
            (Object::Feature, "feature")
        } else {
            (Object::Profile, "profile")
        };

        let import = |src: &Path| -> anyhow::Result<()> {
            let name = src.to_string_lossy();
            let content = if self.feature {
                toml::to_string(&store::load::<Feature, feature::Error>(&name, table, true)?)
            } else {
                toml::to_string(&store::load::<Profile, profile::Error>(&name, table, true)?)
            };

            if let Ok(content) = content
                && let Some(name) = src.file_name()
                && let Some(str) = name.to_str()
            {
                if self.system {
                    if privileged()? {
                        store::SYSTEM_STORE.borrow().store(str, table, &content)?;
                    } else {
                        return Err(anyhow::anyhow!("Not allowed to modify system store"));
                    }
                } else {
                    store::USER_STORE.borrow().store(str, table, &content)?;
                }
            } else {
                warn!("Invalid {kind}: {}", src.display());
            }
            Ok(())
        };

        let profile = Path::new(&self.name);
        if profile.is_dir() {
            for profile in profile.read_dir()?.filter_map(Result::ok) {
                import(&profile.path())?;
            }
            Ok(())
        } else if profile.is_file() {
            import(profile)
        } else {
            Err(anyhow!("No such profile!"))
        }
    }
}
