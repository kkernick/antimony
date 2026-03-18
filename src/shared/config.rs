//! The configuration file is a global configuration for Antimony.

use crate::shared::{
    Set, edit,
    env::{AT_HOME, USER_NAME},
    store,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, read_to_string},
    path::PathBuf,
    sync::LazyLock,
};
use user::as_effective;

pub static CONFIG_PATH: LazyLock<PathBuf> = LazyLock::new(|| AT_HOME.join("config.toml"));
pub static CONFIG_FILE: LazyLock<ConfigFile> = LazyLock::new(ConfigFile::default);

#[derive(Deserialize, Serialize)]
pub struct ConfigFile {
    pub force_temp: Option<bool>,
    pub system_mode: Option<bool>,
    pub auto_refresh: Option<bool>,
    pub privileged_users: Option<Set<String>>,

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
        as_effective!(anyhow::Result<()>, {
            fs::write(AT_HOME.join("config.toml"), toml::to_string(self)?)?;
            Ok(())
        })??;
        Ok(())
    }
}
impl Default for ConfigFile {
    fn default() -> Self {
        let config_path = AT_HOME.join("config.toml");
        let mut config = if config_path.exists()
            && let Ok(content) = read_to_string(config_path)
            && let Ok(parsed) = toml::from_str(&content)
        {
            parsed
        } else {
            Self {
                force_temp: None,
                system_mode: None,
                auto_refresh: None,
                privileged_users: None,
                cache_store: Mutex::default(),
                config_store: Mutex::default(),
            }
        };

        if let Ok(env) = std::env::var("AT_FORCE_TEMP") {
            config.force_temp = Some(env != "0")
        }
        if let Ok(env) = std::env::var("AT_SYSTEM_MODE") {
            config.system_mode = Some(env != "0")
        }
        if let Ok(env) = std::env::var("AT_AUTO_REFRESH") {
            config.auto_refresh = Some(env != "0")
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
}
impl PartialEq for ConfigFile {
    fn eq(&self, other: &Self) -> bool {
        self.force_temp == other.force_temp
            && self.system_mode == other.system_mode
            && self.auto_refresh == other.auto_refresh
            && self.privileged_users == other.privileged_users
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
            privileged_users: self.privileged_users.clone(),
            config_store: Mutex::new(*self.config_store.lock()),
            cache_store: Mutex::new(*self.cache_store.lock()),
        }
    }
}
