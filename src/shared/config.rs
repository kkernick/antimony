//! The configuration file is a global configuration for Antimony.

use crate::shared::{
    Map, Set, edit,
    env::{AT_HOME, USER_NAME},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, read_to_string},
    path::{Path, PathBuf},
    sync::LazyLock,
};

pub static CONFIG_PATH: LazyLock<PathBuf> = LazyLock::new(|| AT_HOME.join("config.toml"));
pub static CONFIG_FILE: LazyLock<ConfigFile> = LazyLock::new(digest_config);

pub fn digest_config() -> ConfigFile {
    let mut config = ConfigFile::new(Path::new("/etc/antimony.toml"));
    if let Ok(iter) = fs::read_dir("/etc/antimony.d") {
        for drop_in in iter.into_iter().filter_map(|d| d.ok()) {
            config.merge(ConfigFile::new(&drop_in.path()));
        }
    }

    if let Ok(env) = std::env::var("AT_FORCE_TEMP") {
        config.force_temp = Some(env != "0")
    }
    if let Ok(env) = std::env::var("AT_SYSTEM_MODE") {
        config.system_mode = Some(env != "0")
    }
    if let Ok(env) = std::env::var("AT_AUTO_REFRESH") {
        config.auto_refresh = Some(env != "0")
    }
    if let Ok(env) = std::env::var("AT_LIB_ROOTS") {
        config.library_roots = env.split(" ").map(String::from).collect()
    }

    config
}

#[derive(Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    pub force_temp: Option<bool>,
    pub system_mode: Option<bool>,
    pub auto_refresh: Option<bool>,
    pub privileged_users: Option<Set<String>>,

    #[serde(skip_serializing_if = "Set::is_empty", default = "Set::default")]
    pub library_roots: Set<String>,

    #[serde(skip_serializing_if = "Map::is_empty", default = "Map::default")]
    pub environment: Map<String, String>,
}
impl ConfigFile {
    pub fn auto_refresh(&self) -> bool {
        self.auto_refresh.unwrap_or(false)
    }

    pub fn force_temp(&self) -> bool {
        self.force_temp.unwrap_or(false)
    }

    pub fn system_mode(&self) -> bool {
        self.system_mode.unwrap_or(false)
    }

    pub fn is_privileged(&self) -> bool {
        if let Some(users) = &self.privileged_users {
            users.contains(USER_NAME.as_str())
        } else {
            false
        }
    }

    pub fn library_roots(&self) -> &Set<String> {
        &self.library_roots
    }

    pub fn environment(&self) -> &Map<String, String> {
        &self.environment
    }

    pub fn edit(config: &str) -> Result<Option<String>, edit::Error> {
        edit::edit::<Self>(config)
    }

    pub fn merge(&mut self, mut config: ConfigFile) {
        let switch = |s: &mut Option<bool>, o: Option<bool>| {
            *s = match s {
                Some(true) => Some(true),
                None | Some(false) => o,
            }
        };

        switch(&mut self.force_temp, config.force_temp);
        switch(&mut self.system_mode, config.system_mode);
        switch(&mut self.auto_refresh, config.auto_refresh);

        self.library_roots.extend(config.library_roots);
        self.environment.extend(config.environment);

        if let Some(users) = config.privileged_users.take() {
            self.privileged_users.get_or_insert_default().extend(users);
        }
    }

    fn new(config_path: &Path) -> Self {
        if config_path.exists() {
            match read_to_string(config_path) {
                Ok(content) => match toml::from_str(&content) {
                    Ok(parsed) => return parsed,
                    Err(e) => eprintln!("Failed to read config {}: {e}", config_path.display()),
                },
                Err(e) => {
                    eprintln!("Could not read config {}: {e}", config_path.display())
                }
            }
        }

        Self {
            force_temp: None,
            system_mode: None,
            auto_refresh: None,
            privileged_users: None,
            library_roots: Set::default(),
            environment: Map::default(),
        }
    }
}
