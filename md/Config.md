# Antimony Config

The Antimony Configuration, not be confused with Profile Configurations, is a setting file locate within `$AT_HOME` that controls global behavior of Antimony. Due to this, modification of the Configuration is considered a *Privileged* Operation. Privileged Users are:

1.  Users expressly allowed to modify the configuration from within the configuration itself.
2. The User that owns `$AT_HOME`
3. A User that can pass a PolKit prompt when attempting a privileged operation. 

All values in the Configuration can be overridden through environment variables at runtime. The table below illustrates the available settings:

| Name               | Environment       | Description                                      |
| ------------------ | ----------------- | ------------------------------------------------ |
| `force_temp`       | `AT_FORCE_TEMP`   | Place cache files in `/tmp`.                     |
| `system_mode`      | `AT_SYSTEM_MODE`  | Do not use user-profiles                         |
| `auto_refresh`     | `AT_AUTO_REFRESH` | Automatically refresh if a profile failed to run |
| `privileged_users` | N/A               | Users allowed to modify the configuration        |
| `config_store`     | `AT_CACHE_DB`     | The Backend for Configurations                   |
| `cache_store`      | `AT_CONFIG_DB`    | The Backend for Caches                           |
