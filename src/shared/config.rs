use crate::shared::{Set, edit, env::AT_HOME};
use nix::libc::getpwuid;
use serde::{Deserialize, Serialize};
use std::{ffi::CString, fs::read_to_string, path::Path, sync::LazyLock};
use user::USER;

pub static CONFIG_FILE: LazyLock<ConfigFile> = LazyLock::new(ConfigFile::default);

#[derive(Deserialize, Serialize)]
pub struct ConfigFile {
    force_temp: Option<bool>,
    system_mode: Option<bool>,
    privileged_users: Option<Set<String>>,
}
impl ConfigFile {
    pub fn force_temp(&self) -> bool {
        self.force_temp.unwrap_or(false)
    }

    pub fn system_mode(&self) -> bool {
        self.system_mode.unwrap_or(false)
    }

    pub fn is_privileged(&self) -> bool {
        if let Some(users) = &self.privileged_users {
            unsafe {
                let passwd = getpwuid(USER.real.as_raw());
                if passwd.is_null() || (*passwd).pw_name.is_null() {
                    false
                } else {
                    let name = CString::from_raw((*passwd).pw_name);
                    let str = name.to_string_lossy();
                    users.contains(str.as_ref())
                }
            }
        } else {
            false
        }
    }

    pub fn edit(path: &Path) -> Result<Option<()>, edit::Error> {
        edit::edit::<Self>(path)
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
                privileged_users: None,
            }
        };

        if let Ok(env) = std::env::var("AT_FORCE_TEMP") {
            config.force_temp = Some(env != "0")
        }
        if let Ok(env) = std::env::var("AT_SYSTEM_MODE") {
            config.system_mode = Some(env != "0")
        }

        config
    }
}
