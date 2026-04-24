## 5.0.0

### Breaking Changes

#### Antimony
* Profile/Feature libraries have been split into `files` and `directories` within a new `[libraries]` header. System profiles have been updated, but user profiles will need to be updated.
	* `libraries.no_sof` now controls whether roots are mounted, rather than simply having `/usr/lib` in the library list. The `lib` feature has been updated, so no action is needed.
	* `libraries.roots` now control per-profile library roots, which are added to a newly defined `library_roots` field in Antimony’s configuration. You will need to set your library roots if you are installing manually.
* Profile Hooks have been changed. Existing hooks will need to be migrated.
	* A mandatory `type` field now specifies between `Program`, `Shell`, and `Antimony` the first two are analogous to the old `content`/`path` scheme, with `Antimony` allowing privileged execution of another profile (Other hooks only run as the user). The `content` field now either accepts the script content, path, or profile name.
	* `args` has been renamed to `arguments` to align with the Profile field.
* Profile IPC has been renamed to use plural keys. User profiles may need to be updated:
	* `own` is now `owns`
	* `talk` in now `talks`
	* `call` is now `calls`
	* `see` is now `sees`
* The Configuration file has been moved to `/etc/antimony.toml`. Drop-in support is available by placing TOML files in `/etc/antimony.d` The old location is no longer supported; you will need to migrate your existing configuration. 
* The `config` command has been removed, as the configuration file is now a root-owned config in `/etc`.
* The `backend` command has been removed, and the database backend has been removed. There is no use case where it performs better than files. 
* The `cache_store` and `config_store` keys are no longer valid in the configuration file, and will need to be removed.

#### Utilities
* `antimony-bench` has been updated, and can only be used to benchmark version 2.4.0 and greater. 
* `antimony-dumper` and `antimony-monitor` now take an absolute path for the instance.

### New

* The configuration file now supports defining environment variables within the `[environment]` header. These values are only used if it is undefined, so setting `RUST_LOG = "info"` would set that value by default, but calling `RUST_LOG=trace antimony` would overwrite that, as would explicitly setting the value in the environment through `XDG_CONFIG_HOME/environment.d`, `.bashrc`, `/etc/profile`, etc.
* The `info` command has been brought back, dumping the TOML contents of profiles/feature, and diffing user and system definitions.
* The `integrate` command now allows you to create arbitrary profiles in `~/.local/bin` which lets you run programs within Antimony even if a profile is not defined (In the same way it works through the command line)
* The `edit` command now allows you to edit malformed profiles, though they must be valid in order for them to be saved.
* Antimony now ships with an AppArmor profile. You should no longer need to modify `sysctl` to get it, or any application run within it, to work.
* A front-end test suite has been added, allowing us to test `antimony` directly as a binary, rather than through Rust.

### Fixes

* Undoing integration via `antimony integrate -r` is now allowed even if the profile application has been uninstalled from the system.
* `antimony-lockdown` can no longer lose ownership of its home after an update
* `antimony-dumper` respects the `no-timeout` argument.
* `antimony-monitor` should no longer busy-wait as a zombie if the sandbox closes but Antimony does not signal it to terminate.
* `antimony-tracer` will no longer report paths in `/home/antimony`, such as in cases where you symlinked your home user to that path for localization.
### Profiles

### Features

* The `qt6` profile has been broken up into sub-features. User profiles may need to be updated.

### Crates

#### Common
* `common::singleton` can now have multiple instances.

#### Spawn
* `spawn` now performs `String` to `CString` on the `spawn()` call, so the `arg` suite of functions no longer return errors, and always succeed.
* `spawn` no longer has the `fork` feature. It was an interesting proof-of-concept, but had no utility.
* `Spawner` now supports a `env_or` function, that either takes the value from the environment, or falls back to the provided default.

### Technical

* `find` calls now filter on file type, and `get_dir` now checks for `*.so*` for non-executable libraries.
* `get_dir` now returns only the libraries that are external to that directory that are required, effectively treating the directory as it’s own library. 
* `proxy` is no longer initialized in its own thread.
* The `rayon` thread pool now initialized to half of hardware concurrency as it shows better performance on benchmarks.
* `user` and `temp` now have their own errors, which can help identify potential configuration issues.
* Instance directories are separate from the SOF directory for profiles, saving ~10000us on the hot path by avoiding a symlink. A simple refresh should make all required changes, though leftover files may need to be cleaned up in the `CACHE_DIR`.
* `indexset` has been removed, as we now use a pre-seeded `ahash` Hasher for Sets. 
* Only a single hash is computed on the hot path.
* We now use `bilrost` instead of `postcard`.
* The profile hash is now computed with `CONFIG_MODE` and Configuration prior to hashing, so hashes are uniform without random suffixes attached.
* The `*_STORE` statics are no longer duplicated per-thread. This was a relic of the database store.