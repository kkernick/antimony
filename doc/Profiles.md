# Profiles

Antimony creates a sandboxed environment for applications to run in via Profiles, which document the files and resources necessary for the application to function. Profiles are stored within Antimony’s Home (Defaulting to `/usr/share/antimony`), as TOML documents. A collection of Profiles are installed by default as *System Profiles*, and users can additionally create their own *User Profiles*, which exist in `/usr/share/antimony/config/$USER`—these Profiles can include novel definitions, or modifications to a System Profile.

## Creating a Profile

Antimony’s default home is protected from frivolous modification, so you’ll need to use Antimony to mediate access. Creating a Profile is as easy as:

```bash
antimony create my_profile
```

With the name of the profile provided as `my_profile`. This will open up an instance of your default editor (Specified via `EDITOR`, or defaulting to a series of commonly installed ones), allowing you to specify all the various attributes of the application. While the new profile documentation can look intimidating, Antimony needs very little information to properly confine a profile. Simple binaries, like `bash` and command line utilities, do not need profiles *at all*. If the argument to `antimony run` does not point to a valid Profile name, it is assumed to be a binary, and will run it (See [The Command Line Profile](./Defaults). For example, there is no `zsh` profile defined, but this works:

```bash
antimony run zsh
```

Antimony only needs help in three areas:
1. Runtime Dependencies: If your program loads libraries at runtime, calls binaries, or accesses files, Antimony will need you to provide them.
2. IPC: If you want your program to communicate with others, whether that be interaction with your Desktop Environment via Portals, or through the System Bus, you’ll have to provide them.
3. Runtime Requirements: If your program needs to connect to the internet/network, has a unique desktop file name and you want to integrate it into the environment, or has a executable not located in your PATH, you’ll need to specify them as well.

An important design philosophy of Antimony is that by default, the sandbox is completely restrictive. The above `zsh` call has no access to IPC, host files, or libraries outside of those needed to run `zsh`. The onus is on the user/administrator to make the decision on what features of the host should be provided to the sandbox.

### Features
Rather than requiring users to specify all the various requirements for a Profile, they can instead use preexisting *Features*, which encompass a collection of files, resources, and functionality under a name. There a lot of features, which you can view via `antimony info feature`, but the general idea is to encapsulate a common feature of an application to reduce duplication and work. Examples include:
* `gtk3`, `gtk4` and `libadwaita` for the feature set of GTK applications
* `qt5`, `qt6` and `kf6` for the feature set of QT/KDE applications.
* `electron` for Chromium and Electron Applications
* `network` for Network Connectivity
* `xdg-open` to open URLs and Files within the sandbox with the host’s default application.

Using features allows for complicated applications to be expressed concisely; consider the profile for Chromium:

```toml
home.policy = "Enabled"

features = ["electron", "pipewire", "vaapi", "network", "xdg-open", "qt5"]
arguments = ["--ozone-platform=wayland"]

[ipc]
portals = ["FileChooser"]

```

Or a GTK application, such as Amberol:

```toml
id =  "io.bassi.Amberol"
home.policy = "Enabled"

features = ["libadwaita", "mpris", "pipewire", "gst", "vaapi"]

[ipc]
portals = ["Background", "FileChooser", "Inhibit"]
```

Or a QT application, such as Okular:

```toml
id =  "org.kde.okular"
home.policy = "Enabled"

features = ["kf6"]
libraries = ["libOkular6Core*"]

[ipc]
portals = ["FileChooser"]
```

In fact, the largest profile in the current System Default is `virt-manager`, at 14 lines:

```toml
home.policy = "Enabled"

features = ["gtk3", "python", "network", "gtksourceview"]
libraries = ["libvirt*", "libosinfo*"]

[files]
system.ReadOnly = [
  "/var/run/libvirt",
  "/usr/share/hwdata"
]

[ipc]
system_bus = true
own = ["org.virt-manager.virt-manager"]
```

The takeaway from this is that Profiles are not particularly verbose, nor do they take significant effort or grappling esoteric syntax to create.

### Debugging Tools

Antimony provides a set of debugging tools to help make profile creation easier.

#### Tracing
`strace` is a program that traces a binary, printing syscall information. You can trace an application in the sandbox using the `trace` command:

```bash
antimony trace my_profile errors/all --report
```

`trace` dumps the output of `strace` to your console, which is *very* verbose, so you have the option of only logging errors, or outputting the entire thing. If you don’t know how to parse `strace`, you can optionally provide the `--report` flag, which will give you a summary at the end of program execution. This report informs you of every file that the sandbox tried to access, but couldn’t because the file didn’t exist, yet the same file *does* exist on the Host. This can help pin-point files that the profile needs to run, but that the sandbox doesn’t provide. If a Feature can provide that file, they will be listed underneath the file in question, as it can be excessively permissive. Consider the following report:

```
/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq can be provided with the following features
       - udev.toml (via /sys) as ReadOnly
```

While the `udev` feature can indeed provide this file, it does so by providing the *entire* `/sys` folder. It would be more prudent that, if this file is necessary, to pass it directly, or one of it’s parents.

#### Info
The `info` command dumps information related to Features, Profiles, and [SECCOMP](./SECCOMP.md). This can either list everything, or be narrowed down to a specific Feature or Profile. By passing increasing levels of verbosity (Through `-v`), you can get a deeper insight into the files provided by Features, Wildcards, etc. While not quite as verbose as `strace`, the output is still rather long as you add more `-vvvv`.

#### Debug Shell
The `debug-shell` command will create the sandbox for the Profile, but rather than executing the application will drop you into a shell with some utilities for navigating the command line. You can check the contents of the environment, validate files and libraries, and even try and run the application with your own arguments or under different traces (So long as you provide them with `binaries`).

## Editing a Profile

You can use `antimony edit` to edit a profile. Trying to edit  *System Profile* will create a copy for the user, allowing you to customize the behavior without affecting other users. This will open the TOML in your editor of choice.

## Running a Profile

Once you’ve defined a Profile, you can run it with `antimony run profile_name`.  The `run` command has arguments for every aspect of the Profile which you can override (Options specified on the command line overrule Profile definitions). There is a lot, which you can view with either `-h` for a more concise summary, or `--help` for a verbose one. For example, you can set `seccomp` for a single instance via:

```bash
antimony run profile_name --seccomp enforcing
```

## Deleting a Profile

You cannot delete *System Profiles* (You can undo Integration, mentioned below), but you *can* delete *User Profiles*. The command of interest is `antimony reset`, which—as the name suggests—is designed to be used to remove a User’s version of a System Profile—one created by editing a system profile. If the Profile was made by the User, and has no System counterpart, you can delete it with the `--remove` option.

## Integrating a Profile
Antimony supports integrating Profiles into your environment; this makes sandboxed versions of applications seamlessly replace the original versions. There are two components to this, depending on what kind of application you are integrating:

### Binary Integration

All Profiles integrate themselves by installing a symlink to Antimony at `~/.local/bin`. When running as a symlink, Antimony will use it’s name as the profile argument to the `run` command, so: executing the symlink `~/.local/bin/chromium -> /usr/bin/antimony`, would be equivalent to `antimony run chromium`.

This behavior allows you to shadow the original application in shell environments. Simply define you PATH to have `~/.local/bin` in front, and calling `chromium` from the command line will use Antimony instead. This works particularly well for CLI applications. If you need to run the unconfined version on the host, you can just spell out the absolute path, such as `/usr/bin/chromium`.

### Desktop Integration

If you provided an `id` attribute in the Profile, and a corresponding desktop file exists at `/usr/share/applications/id.desktop`, then Antimony will additionally shadow that as well (CLI applications that don’t have any such desktop file will simply stop after binary integration).

The behavior of this depends on whether `id` is a valid Reverse DNS Name, which is required for Portal integration. Basically, some Desktop Environments (Principally Gnome), will source a desktop file with the sandbox’s `id` for icons. However, the ID must be, as mentioned, a valid Reverse DNS (In other words it needs a `.` character). If your `id` doesn’t contain one, like `chromium`, Antimony sticks a prefix onto the end to ensure the name is valid (`antimony.chromium`, in this case). However, this now presents an issue where the Desktop is looking for a desktop file named `antimony.chromium.desktop`, which does not exist.

This presents two cases:
1. If the `id` is valid, Antimony can cleanly create a copy of the system desktop file at `$XDG_DATA_HOME/applications`. The FreeDesktop specification allows for users to modify system desktop files by creating a copy in this location, which is exactly what Antimony does, and replaces the executable to point to the symlink created in binary integration.
2. If the `id` is invalid, you might need to provide the `--shadow` argument, which will create two desktop files at `$XDG_DATA_HOME/applications`:
	1. `id.desktop` will be used to shadow the system copy, otherwise you would have *two* identical copies.
	2. `antimony.id.desktop` is the actual desktop file your environment will recognize and present in App Menus.

Integration is done with `antimony integrate profile_name`. See [Configurations](./Configurations.md) if your profile uses them. Your Desktop Environment may need a few moments to recognize the new files, and you may need to log out to see the changes. You should be presented with an identical application to before, but one that launches under Antimony instead of on the host.

To undo integration, pass the `--remove` flag. Or, if you want to do it manually:
1. Delete the symlink at `~/.local/bin/name`
2. Delete the desktop files at `$XDG_DATA_HOME/applications/id.desktop` and perhaps `antimony.id.desktop`

## Refreshing

Antimony caches Profiles to ensure fast startup. However, when you update your system, the cached definitions may become out of date. To reconcile this, you can use `antimony refresh`, which performs sandbox setup on either the provided Profile, or every integrated profile, but without running the applications.
