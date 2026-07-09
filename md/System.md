## System Maintenance

## Creating/Editing

Antimony is bundled with a curated set of profiles and features that cover a wide range of applications. These are typically installed in `/usr/share/antimony/config`, and require *privilege* to modify. This is in the typical sense of the word, as in using `sudo` to edit them directly (though doing so prevents Antimony from ensuring valid modifications), but also because Antimony keeps a list of `privileged_users` in `/etc/antimony.toml` or one of its drop-ins in `/etc/antimony.toml.d` that skip the usual PolKit check.

The `--system` flag on `edit` will open the system profile, but the above restrictions are in effect; if you couldn’t modify the file *outside* Antimony, and haven’t told Antimony you have permission to do so, you’ll need to pass a PolKit dialog. 

Otherwise, when the `--system` flag is omitted, Antimony creates a *copy* as a *User Profile*. As the name suggests, this is a user-specific version of the *System Profile* stored in `/usr/share/antimony/config/$USER` that you can freely modify. 

The `--feature` flag pivots to the Feature Set, and operates identically; there are *System Features*, and Antimony will create a *User Profile* copy for each User that can be freely modified.

The `edit` command serves the purpose of both modifying existing profiles/features, as well as creating new ones. Simply pass an “Object” (Antimony jargon to consolidate profiles and features) name to `edit` without a corresponding file, and Antimony will use the default file in `/usr/share/config/{profile,feature}.toml` and create a copy—with a mountain of information.

## Removing

The `remove` command can have a confusing name, as it does multiple things depending on certain conditions:
* If there is no User Profile, this command does what it says on the tin: it removes the System Profile. As expected, you need privilege for this operation. You will no longer be able to run the profile, and will have to recreate it (Or reinstall)
* If there is a User Profile, this command functions more like a `reset` command: it removes the User Profile, thus resetting to the System one. You will continue to be able to run the profile, though any user-modifications will no longer be there.

Again, the `--feature` flag operates identically for the Feature Set.

## Import/Output

The `import` command can add any valid `.toml` to your User Store, while `export` can copy a profile outside Antimony’s system directory. 

## Refresh

Antimony creates a per-profile version of your system library folder in a specialized SOF, typically located in `/usr/share/antimony/cache/run`. Indexed by a cache, Antimony will usually automatically update should the profile be modified, but if the system itself changes, such as a package update that upgrades libraries or binaries, it could cause Antimony’s cached definitions to fail. 

The `refresh` command serves to forcefully recalculate SOF definitions—and all its associated caches. It’s usually wise to run after a system update. 

`refresh` is safe to run with running instances, as it detects profiles using an SOF, and creates the updated files in a temporary location that then seamlessly replaces the original once all instances have closed—such as after a reboot. The downside to a regular refresh is that cached definitions are never deleted—simply updated or with new definitions added. The `--hard` flag deletes the entire Cache Dir; this pulls the rug under running instances, and they will probably start throwing errors, but in cleans up no longer used caches.

## Integrate

Antimony can seamlessly integrate with your desktop environment through two approaches:
* It creates a symlink in your local bin (`~/.local/bin`). Should this be in your `PATH` (And before the system `/usr/bin`), the sandboxed profile will replace the native binary.
* If the profile has an associated `id` (Either explicitly defined in the profile, or with a matching file in `/usr/share/applications`),  Antimony will replace the system desktop file with a local version in `~/.local/share/applications`. This has the effect of replacing it—running the binary from your DE will launch it in a sandbox.

There are a lot of ways to integrate a profile, with myriad arguments to match:
* `--shadow`: Some DE’s (namely GNOME), source an application’s icon from its Desktop File. Because Antimony mimics Flatpak, GNOME additionally sources the internal `id` Antimony uses, then expects a Desktop File with that name. It then sources the Icon from that. Unfortunately, Flatpak IDs are expected to be in rDNS format (i.e contains a period), so Antimony adds an `antimony.` prefix . So, if:
	* You’re running GNOME
	* Your application uses IPC
	* Your application ships with a Desktop File without a dot in in (e.g `chromium.desktop`).
  GNOME won’t give the sandbox the correct icon when you run it. The `--shadow` flag resolves this by:
	* Replacing the real Desktop File with a `NoDisplay` version in `~/.local/share/applications`, which hides the original
	* Creates an `antimony.$PROFILE.desktop` identical to the real one. This ensures GNOME can find the correct file. The *Shadow* exists to ensure there isn’t a duplicate.
* `--config-mode`: Antimony profiles can have [Configurations](./Configurations.md), but it isn’t obvious the best way to expose them as a Desktop File. Antimony, therefore, supports two options:
	* `file` will create a separate Desktop File for each configuration (Including the regular profile without any Configuration). This is useful, as you can assign/open file types with different Configurations.
	* `desktop` will integrate each Configuration as an *Action*, which is usually for things like “Open a new Window.” You can access these by right clicking the application in GNOME.
* `--create-desktop`: If there isn’t an associated Desktop File for a profile, such as `bash`, Antimony will stop at creating the binary symlink; you can use `--create-desktop` to ask Antimony to create a minimal file—even if one doesn’t exist.
* `--autostart` and `--enable`: Typically, programs can ask to be run on startup by placing them in `/etc/xdg/autostart`, or `~/.config/autostart`. Antimony uses `systemd` instead, as it provide a more reliable, customizable means of running something when the user logs in via User Services. With `--autostart`, Antimony:
	* Creates a new user service in `~/.config/systemd/user`.
	* Uses the same `NoDisplay` trick to disable any existing `/etc/xdg/autostart` file.
	The `--enable` flag actually enables the service so it will run on next login. You can also use `systemctl start --user antimony-$PROFILE` to start it. If there are configurations, a `@` unit will be created. You can then enable which configurations you like with `systemctl enable --user antimony-$PROFILE@$CONFIG.service`.

You can undo everything above with `--remove`.




