#![allow(clippy::missing_docs_in_private_items)]
//! The configuration file is a global configuration for Antimony.

use crate::shared::{Set, edit, env::USER_NAME, extend_option};
use serde::{Deserialize, Serialize};
use std::{
    env,
    fs::{self, read_to_string},
    path::Path,
    sync::LazyLock,
};

pub static CONFIG_FILE: LazyLock<ConfigFile> = LazyLock::new(digest_config);

pub fn digest_config() -> ConfigFile {
    let mut config = ConfigFile::new(Path::new("/etc/antimony.toml"));
    if let Ok(iter) = fs::read_dir("/etc/antimony.d") {
        for drop_in in iter.into_iter().filter_map(Result::ok) {
            config.merge(ConfigFile::new(&drop_in.path()));
        }
    }
    if let Ok(env) = env::var("AT_LIB_ROOTS") {
        config.library_roots = Some(env.split(' ').map(String::from).collect());
    }

    config
}

#[derive(Deserialize, Serialize, Clone, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    pub privileged_users: Option<Set<String>>,
    pub library_roots: Option<Set<String>>,
}
impl ConfigFile {
    #[must_use]
    pub fn is_privileged(&self) -> bool {
        self.privileged_users
            .as_ref()
            .is_some_and(|users| users.contains(USER_NAME.as_str()))
    }

    #[allow(clippy::missing_errors_doc)]
    pub fn edit(config: &str) -> Result<Option<String>, edit::Error> {
        edit::edit::<Self>(config)
    }

    pub fn merge(&mut self, config: Self) {
        extend_option(&mut self.library_roots, config.library_roots);
        extend_option(&mut self.privileged_users, config.privileged_users);
    }

    fn new(config_path: &Path) -> Self {
        if config_path.exists() {
            match read_to_string(config_path) {
                Ok(content) => match toml::from_str(&content) {
                    Ok(parsed) => return parsed,
                    Err(e) => eprintln!("Failed to read config {}: {e}", config_path.display()),
                },
                Err(e) => {
                    eprintln!("Could not read config {}: {e}", config_path.display());
                }
            }
        }

        Self::default()
    }
}
