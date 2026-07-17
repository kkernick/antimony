# Packages

One key component of Antimony is that is indexes the system for every file and resource a program needs to run. It does this through a combination of explicit signals (i.e Profiles and Features), and implicit information gathering (i.e SOF and shell-script parsing). This inventory of required files engenders the possibility of compressing all that information into a single, self-contained file—in other words a package.

This was first explored in Version 2, but promptly dropped in Version 3 as it had fallen into disrepair and was not actually useful enough to be used; it was more a technical exercise in what Antimony could *theoretically* do, without being actually useful (Quite like `spawn`’s `fork` feature).

In version 5.2.0, Packages have made a return, though entirely divorced from the original design. This document is more a high-level discussion of the feature, the sub-command is largely self describing and you can `antimony package PROFILE` to your heart’s content.

## Self-Contained

The original package format described itself as “self-contained,” yet still had a rather large dependency: Antimony itself. To run a package, you would pass the path to the package rather than a profile name to `antimony run`, and it would unpack the ZIP compressed contents. This zipped folder contained:

1. The profile TOML
2. The SOF
3. Resources and Binaries

However, this hard requirement made packages a non-starter; if you already had Antimony installed, what was the point in using a package? Technically it allowed you to run software you would otherwise need to install, but the new Package is *truly* self-contained.

This is done by quite literally bolting a `bilrost` encoded payload to the end of the Antimony binary. Then, we abuse the ELF header to stick a very specific set of bits into a region that is otherwise unused—which lets Antimony immediately recognize that it is running with a payload, rather than needing to scan for that information (And all the latency that would cause for every execution)

We were already using `bilrost` for serializing cache files, and the entire payload is compressed by ZSTD to bring packages—on average—to a few hundred megabytes. Sure, it’s a hefty executable, but it also contains *all* the dependencies needed to run it. This payload contains everything from the original package, with a distinction between system resources (`bwrap`, `ldd`, etc) and sandbox resources.

*Technically*, because you are running `antimony` and `bwrap`, you need *some* shared libraries on the system to run a package, specifically:

* `libzstd` (`linux-headers`, `curl`, `boost`, `llvm`)
* `libsqlite3` (`gnupg`, `qt6`, `util-linux`)
* `libseccomp` (`systemd`, `glycin`)
* `libdbus` (`dbus-broker`, `pipewire`, `plasma-workspace`, `systemd`)
* `libgcc` (Literally everything)
* `libcap` (`gstreamer`, `avahai`, `perf`)
* `libc` (Literally everything

But these are so ubiquitous that they are almost certainly already installed on your system. The packages is brackets are just some examples of packages that depend on these libraries.

***

Another problem we need to deal with is that, without an Antimony installation, we cannot rely on things like SetUID, Library Roots, or Caches. This is particularly difficult because the first stage, the bundled `antimony`, `bwrap` and other executable needed to actually run the sandbox need packaged libraries.

The solution is utilizing `bubblewrap` to create *two* sandboxes: an interior for pivoting required libraries and binaries into the correct positions, then the second, regular sandbox that Antimony executes from within that environment.

## Integration

Packages also now support DE integration, allowing you to seamlessly use packages as you would regular system applications.

During the packaging stage, Antimony collects all potential icons and desktop files for an application and its configurations, and adds those to the package, the `--integrate` argument on the package on the new host will then have those files—if they existed—put into place in the user’s home. The package itself will then be copied to a stable location (`$XDG_DATA_HOME/antimony/packages`). The `--integrate` flag accepts all typical `integrate` arguments, so integration can be undone with the `--remove` flag.

## Refreshing

There are two forms of refresh with packages:

1. `./package.sb --refresh` tells Antimony to tear down the previously unpacked package and extract it again from the payload; because there is no versioning scheme for packages, if you install an updated version of a package, Antimony will reuse the existing unpacked directory unless you (A) reboot the computer (As the package is unpacked in `/tmp`, or (B) call `--refresh`
2. `./package.sb -- --refresh` The `--` separator passes the `--refresh` flag to Antimony’s typical `run` arguments, which does a standard refresh (Though in the context of a package this only really refreshes non-executable files as libraries/binaries are packaged.