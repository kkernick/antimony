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
	* [x] `antimony setup` for complicated profiles.
		* I tried, but it’s not very useful :(
		* You end up having to do a lot of manual work, so a dedicated setup doesn’t have much utility
	* [ ] Command completion for loading profiles (IE antimony run ch + tab → chromium)
	* [x] Integrate should require a path to add, but not to remove
	* [x] Ensure all environment variables have a fallback (IE `XDG_*`), and have support on the Spawner for an `env_or` to pass a default.
	* [ ] Investigate sandbox fails after calling `run` after making a change to the profile. Stale cache?
	* [x] `info` should be able to dump SECCOMP info, too.
		* There’s really no good place to put it. `info` is tailored for Profiles/Features, `seccomp` is tailored for paths.
	* [x] Migrate instance information entirely in `RUNTIME`. `HASH/INSTANCE`. No symlink.
	* [x] Profile `HASH` should incorporate config/other info before hashing, so they’re consistently named.
* [x] Ensure Steam Works
* [x] Ensure Ubuntu Works
	* [x] Antimony AppArmor Profile
	* [x] Dynamic Sandbox Profiles too? TOML → AppArmor? aa-exec? How to handle privilege of loading?
		* `aa-exec` only allows using a policy already loaded.
		* Our `antimony` profile does not allow `pix`.
* [ ] Update docs
* [ ] Actually add a comprehensive testing suite
* [ ] Update Version and add tag.