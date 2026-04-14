# Todo

`5.0.0` will be the last Antimony major release. The following things need to be done:

* [x] Refactor benchmarker
* [x] Refactor library fabricator
* [ ] Optimize and profile
* [ ] Features
	* [x] SECCOMP Fixes
	* [x] Move Config to /etc
	* [x] Support logging in config
	* [x] Return info by dumping TOML + Diff of User + System
	* [x] Refactor hooks so explicitly define type (Script, Program Antimony)
	* [x] L2 Profile Cache where Default/Cmdline get fabricated?
		* The Command Line must be cached with the profile. 
	* [x] Investigate hooks not being killed with parent.
	* [x] Allow editing invalid profiles.
	* [ ] `antimony setup` for complicated profiles.
	```toml 
	[[setup.dependencies]]
	name = "Checking Depends..."
	commands = ["command -v x", "command -v y", ...]
	fatal = true
	```
	* [ ] Command completion for loading profiles (IE antimony run ch + tab → chromium)
	* [x] Integrate should require a path to add, but not to remove
	* [ ] Ensure all environment variables have a fallback (IE `XDG_*`), and have support on the Spawner for an `env_or` to pass a default.
	* [ ] Investigate sandbox fails after calling `run` after making a change to the profile. Stale cache?
* [ ] Ensure Steam Works
* [ ] Ensure Ubuntu Works
	* [ ] Antimony AppArmor Profile
	* [ ] Dynamic Sandbox Profiles too? TOML → AppArmor? aa-exec? How to handle privilege of loading?
* [ ] Update docs
* [ ] Actually add a comprehensive testing suite
* [ ] Update Version and add tag.