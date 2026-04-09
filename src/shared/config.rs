//! The configuration file is a global configuration for Antimony.

use crate::shared::{
    Set, edit,
    env::{AT_HOME, USER_NAME},
    store,
};
use log::{error, warn};
use parking_lot::Mutex;
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
    if let Ok(env) = std::env::var("AT_CACHE_DB")
        && env != "0"
    {
        config.cache_store = Mutex::new(Some(store::Store::Database));
    }
    if let Ok(env) = std::env::var("AT_CONFIG_DB")
        && env != "0"
    {
        config.config_store = Mutex::new(Some(store::Store::Database));
    }

    config
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    pub force_temp: Option<bool>,
    pub system_mode: Option<bool>,
    pub auto_refresh: Option<bool>,
    pub privileged_users: Option<Set<String>>,

    #[serde(skip_serializing_if = "Set::is_empty", default = "Set::default")]
    pub library_roots: Set<String>,

    pub config_store: Mutex<Option<store::Store>>,
    pub cache_store: Mutex<Option<store::Store>>,
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

    pub fn cache_store(&self) -> store::Store {
        self.cache_store.lock().unwrap_or(store::Store::File)
    }

    pub fn config_store(&self) -> store::Store {
        self.config_store.lock().unwrap_or(store::Store::File)
    }

    pub fn edit(config: &str) -> Result<Option<String>, edit::Error> {
        edit::edit::<Self>(config)
    }

    pub fn update(&self) -> anyhow::Result<()> {
        fs::write(AT_HOME.join("config.toml"), toml::to_string(self)?)?;
        Ok(())
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

        if let Some(users) = config.privileged_users.take() {
            self.privileged_users.get_or_insert_default().extend(users);
        }

        if let Some(cache) = config.cache_store.lock().take() {
            let mut s = self.cache_store.lock();
            if s.is_none() {
                s.replace(cache);
            }
        }
        if let Some(config) = config.config_store.lock().take() {
            let mut s = self.config_store.lock();
            if s.is_none() {
                s.replace(config);
            }
        }
    }

    fn new(config_path: &Path) -> Self {
        if config_path.exists() {
            match read_to_string(config_path) {
                Ok(content) => match toml::from_str(&content) {
                    Ok(parsed) => return parsed,
                    Err(e) => error!("Failed to read config {}: {e}", config_path.display()),
                },
                Err(e) => {
                    warn!("Could not read config {}: {e}", config_path.display())
                }
            }
        }

        Self {
            force_temp: None,
            system_mode: None,
            auto_refresh: None,
            privileged_users: None,
            library_roots: Set::default(),
            cache_store: Mutex::default(),
            config_store: Mutex::default(),
        }
    }
}
impl PartialEq for ConfigFile {
    fn eq(&self, other: &Self) -> bool {
        self.force_temp == other.force_temp
            && self.system_mode == other.system_mode
            && self.auto_refresh == other.auto_refresh
            && self.privileged_users == other.privileged_users
            && self.library_roots == other.library_roots
            && self.config_store.lock().as_ref() == other.config_store.lock().as_ref()
            && self.cache_store.lock().as_ref() == other.cache_store.lock().as_ref()
    }
}
impl Clone for ConfigFile {
    fn clone(&self) -> Self {
        Self {
            force_temp: self.force_temp,
            system_mode: self.system_mode,
            auto_refresh: self.auto_refresh,
            library_roots: self.library_roots.clone(),
            privileged_users: self.privileged_users.clone(),
            config_store: Mutex::new(*self.config_store.lock()),
            cache_store: Mutex::new(*self.cache_store.lock()),
        }
    }
}
