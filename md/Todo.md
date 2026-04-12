# Todo

`5.0.0` will be the last Antimony major release. The following things need to be done:

* [x] Refactor benchmarker
* [x] Refactor library fabricator
* [ ] Optimize and profile
* [ ] Features
	* [x] SECCOMP Fixes
	* [x] Move Config to /etc
	* [ ] Support logging in config
	* [ ] Return info by dumping TOML + Diff of User + System
	* [ ] Refactor hooks so explicitly define type (Script, Program Antimony)
	* [ ] L2 Profile Cache where Default/Cmdline get fabricated?
	* [ ] Investigate hooks not being killed with parent.
	* [x] Allow editing invalid profiles.
	* [ ] `antimony setup` for complicated profiles.
	```toml 
	[[setup.dependencies]]
	name = "Checking Depends..."
	commands = ["command -v x", "command -v y", ...]
	fatal = true
	```
	* [ ] Command completion for loading profiles (IE antimony run ch + tab → chromium)
	* [ ] Integrate should require a path to add, but not to remove
* [ ] Ensure Steam Works
* [ ] Ensure Ubuntu Works
	* [ ] Antimony AppArmor Profile
	* [ ] Dynamic Sandbox Profiles too? TOML → AppArmor? aa-exec? How to handle privilege of loading?
* [ ] Update docs
* [ ] Update Version and add tag.