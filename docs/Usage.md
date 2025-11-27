# Usage

This document outlines the Command Line Interface of Antimony.

```
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣰⣦⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀  
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⣴⠟⠹⣧⡀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀  
⠀⠀⠀⠀⠀⠀⠀⠀⣷⣦⣄⣠⣿⠃⢠⣄⠈⢻⣆⣠⣴⡞⡆⠀⠀⠀⠀⠀⠀⠀  
⠀⠀⠀⠀⠀⢀⣀⣀⣿⠀⠈⢻⣇⢀⣾⢟⡄⣸⡿⠋⠀⡇⣇⣀⣀⠀⠀⠀⠀⠀  
⠀⣤⣤⣤⣀⣱⢻⠚⠻⣧⣀⠀⢹⡿⠃⠈⢻⣟⠀⢀⣤⠧⠓⣹⣟⣀⣤⣤⣤⡀  
⠀⠈⠻⣧⠉⠛⣽⠀⠀⠀⠙⣷⡿⠁⠀⠀⠀⢻⣶⠛⠁⠀⠀⡟⠟⠉⣵⡟⠁⠀  
⠀⠀⠀⠹⣧⡀⠏⡇⠀⠀⠀⣿⠁⠀⠀⠀⠀⠀⣿⡄⠀⠀⢠⢷⠀⣼⡟⠀⠀⠀  
⠀⠀⠀⠀⠙⣟⢼⡹⡄⠀⠀⣿⡄⠀⠀⠀⠀⢀⣿⡇⠀⢀⣞⣦⢾⠟⠀⠀⠀⠀  
⠀⠠⢶⣿⣛⠛⢒⣭⢻⣶⣤⣹⣿⣤⣀⣀⣠⣾⣟⣠⣔⡛⢫⣐⠛⢛⣻⣶⠆⠀  
⠀⠀⠀⠉⣻⡽⠛⠉⠁⠀⠉⢙⣿⠖⠒⠛⠻⣿⡋⠉⠁⠈⠉⠙⢿⣿⠉⠀⠀⠀  
⠀⠀⠀⠸⠿⠷⠒⣦⣤⣴⣶⢿⣿⡀⠀⠀⠀⣽⡿⢷⣦⠤⢤⡖⠶⠿⠧⠀⠀⠀  
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠛⢿⣦⣴⡾⠟⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀  
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠙⠟⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀  
  
Sandbox Applications  
  
Usage: antimony <COMMAND>  
  
Commands:  
run          Run a profile  
create       Create a new profile  
edit         Edit an existing profile  
default      Edit the default profile  
feature      Modify the system features  
refresh      Refresh caches  
integrate    Integrate a profile into the user environment  
reset        Reset a profile back to the system-defined profile  
trace        Trace a profile for missing syscalls or files  
stat         Collect stats about a profile's sandbox  
info         List installed profiles and features  
debug-shell  Drop into a debugging shell within a profile's sandbox  
seccomp      Perform operations on the SECCOMP Database  
package      Package a Profile into a self-contained package  
export       Export user profiles  
import       Import user profiles  
help         Print this message or the help of the given subcommand(s)
```

>[!note]
>For commands that involve editing, Antimony will try and source an editor through the `EDITOR` environment variable. If it is not defined, it will attempt several well-known editors.

## Run

```
Usage: antimony run [OPTIONS] <PROFILE> [PASSTHROUGH]...
```

*Run* is the principal command for Antimony. It creates the sandbox as defined within the [Profile](./Profiles.md), launches it, then performs cleanup after the sandbox has exited. When Antimony is symlinked, it will treat the link name as the Profile, and will treat all passed argument as Passthrough. Therefore with a link `/usr/bin/antimony -> chromium`, and running `chromium --no-sandbox` would be equivalent to `antimony run chromium --no-sandbox`.

Almost every aspect of the Profile can be altered from the command line via the Options. It includes almost all aspects of the Profile definition, with only the following exceptions:

1. User, Platform, and Resource Files can not be explicitly provided. The `--ro` and `--rw` flag treats all files as Platform.
2. You cannot define [Configurations](./Configurations.md) on the Command Line
3. You cannot specify [Hooks](./Hooks.md) on the Command Line

Passthrough can either be defined as the first unrecognized option, such as `antimony run chromium --no-sandbox`, or with the explicit `--` switch to tell Antimony all subsequent arguments are for the sandboxed application—useful in cases where Antimony shares command line arguments with the program.

See [Defaults](./Defaults.md) for how Command Line Arguments are prioritized. 
## Create

```
Usage: antimony create [OPTIONS] <PROFILE>
```

*Create* is used to make new user profiles. These profiles are stored in `$AT_HOME/config/$USER/profiles`, and exclusive to the current user. The Profile cannot already exist. 

## Edit
```
Usage: antimony edit <PROFILE>
```

*Edit* is used to edit existing profiles. This can be used to create a user-profile from a system-profile, allowing user-specific modifications. The Profile must either exist as a system or user profile.

## Default

```
Usage: antimony default
```

See [Defaults](./Defaults.md)

>[!note]
>You can use both `antimony create default` and `antimony edit default` in lieu of `antimony default`.

## Feature

```
Usage: antimony feature [OPTIONS] <FEATURE>
```

*Feature* allows modification of the system feature set. It principally acts as a privileged version of *Edit*, but for Features, and can also delete features.

>[!warning]
>Antimony uses Polkit to ensure the user has the necessary privilege to modify the system set. There are no user-features, and as such modifying the feature set impacts all users.

## Refresh

```
Usage: antimony refresh [OPTIONS] [PROFILE]
```

*Refresh* updates cached definitions for both library and binary resolution, alongside Profile resolution. Caches are defined given the contents of a profile, but can become stale in the event of a system update. 

*Refresh* has two primary modes:
1. As an analog to *Run*, except all cached definitions are updated. For example, `antimony refresh chromium`.
2. As a global cache updater: `antimony refresh`.

Antimony does not refresh the cache for profiles that are currently running, as this could cause issues in the sandbox. In this case, a new cache will be created, new instances will use that cache, and when the last instance of the old cache closes, it will be updated automatically. However, in cases where a complete refresh is required, the destructive `--hard` switch will simply delete the folder blindly. 

>[!warning]
>The `--hard` switch can cause erratic behavior for running instances!

Refreshing can also be done in `--dry` mode, which updates caches, but does not run the sandbox. For example `antimony refresh chromium --dry` would update Chromium’s cache, but not run the application. Similarly, `antimony refresh --dry` will merely delete existing caches, and `antimony refresh --hard --dry` will merely delete the entire cache folder.

## Integrate

```
Usage: antimony integrate [OPTIONS] <PROFILE>
```

*Integrate* is used to integrate a profile into the Desktop Environment of the user. This involves two stages:

1. Creating a symlink from `/usr/bin/antimony -> ~/.local/bin/$PROFILE`. If in your Path, this seamlessly replaces the system installation with the sandboxed version.
2. Creating a Desktop file `~/.local/share/applications` (For applications with a relevant Desktop file in `/usr/share/applications`). This shadows the system desktop file, allowing sandboxes to be launched from your Desktop Environment.

See [Profiles](./Profiles.md) for more information.

## Reset

```
Usage: antimony reset [PROFILE]
```

*Reset* is used to delete user-profiles. If the profile was derived from an existing system-profile, this effectively restores the sandbox back to the system state. If this profile was created by the user, it deletes the profile irrevocably. 

## Trace

```
Usage: antimony trace [OPTIONS] <PROFILE> <MODE> [PASSTHROUGH]...
```

*Trace* runs the sandbox under `strace`, with Passthrough defining additional `strace` arguments. This is used to collect information about:

* Files that the program tired to access, but do not exist in the sandbox
* Syscalls errors

The `--report` flag will summarize the details, and present a list of files that the sandbox tried to access, but did not exist—but that *do* exist on the host. It also lists Features that provide those files.

## Stat

```
Usage: antimony stat [OPTIONS] <PROFILE>
```

*Stat* runs a diagnostic tool in the sandbox, rather than the application, to collect information about available files. 

## Info

```
Usage: antimony info [OPTIONS] <WHAT> [NAME]
```

*Info* provides information about Antimony. What can be:

1. `profile` for information about profiles
2. `feature` for information about  features
3. `seccomp` to query the SECCOMP database

The optional Name argument can return only the information to a relevant Profile, Feature, or Binary, but it can be omitted to provide all available information. `-v` can be increment to increase the detail.

## Debug Shell

```
Usage: antimony debug-shell [OPTIONS] <PROFILE>
```

*Debug Shell* drops the terminal into a shell within the sandbox.

## SECCOMP

```
Usage: antimony seccomp <OPERATION> [PATH]
```

*SECCOMP* allows for modification of the SECCOMP Database. Like *Features*, this command requires administrative privilege. The following Operations can be performed:

1. `optimize` Will optimize the SQLite database.
2. `remove` Will delete the database completely.
3. `export` Will export the database to a path.
4. `merge` Will import the rules from an exported database.
5. `clean` Will remove binaries that no longer exist on the system

>[!warning]
>*Clean* Removes binaries not on the system, but does not have the knowledge of Features that may provide binaries through either another name, or wholesale through Direct Files. This will delete any SECCOMP definitions for those binaries.

## Package

```
Usage: antimony package [OPTIONS] <PROFILE>
```

*Package* utilizes the dependency resolution and isolated running environment of Antimony to create self-contained archives of profiles. These packages can generally be executed on any system with Antimony installed. 

Packages are created as Zip folders with the `.sb` extension. Calling `antimony run` with a path to one of these packages will cause Antimony to unpack the archive and run the stored profile.

>[!note]
>The name of the package is used by Antimony. Do not change the package name from `$PROFILE.sb`.

## Export

```
Usage: antimony export [PROFILE] [DEST]
```

*Export* saves the user-profile, or all user-profiles if not specified, to the provided destination, or the current working directory. 
## Import

```
Usage: antimony import [OPTIONS] <PROFILE>
```

*Import* takes the Profile, which can either be a file, or directory to profiles, and imports them as user-profiles for the current user. Only valid profiles for the installed version of Antimony will be imported. By default, existing profiles will not be overwritten unless the user confirms it, or the `--overwrite` flag is provided.