# Speed

Antimony is *fast*. This document outlines the techniques used to optimize a very slow process, and to highlight Antimony’s improvements over its predecessors.

## Comparisons

### Configuration

The configuration of Antimony’s installation can have a profound effect on performance. These are divided into *Build Time* configuration, and *Run Time* configuration.

1. *Build Time*: Choosing to compile Antimony yourself, and using `-Ctarget-cpu=native` in your `RUSTFLAGS` optimizing the resulting binary for your architecture, and can drastically improve performance. Antimony publishes binaries for each release, but these are tailored to work on all x86 machines. Additionally, further optimization can be squeezed out of the binary for your particular workflow and profiles using the `pgo` or `bolt` helper scripts (Which require additional dependencies).
2. *Run Time*: The most important performance consideration is the privileges given to the `antimony` executable, and the location of `AT_HOME`. By default, Antimony creates hard-links for library files in a cache located within `AT_HOME`. If it cannot do this, such as if its lacking the `FOWNER` capability, or if `AT_HOME` isn’t on the same partition as `/usr/lib`, then it will create copies in `/tmp`. This has a drastic toll on performance.

>[!warning]
>Creating files in `/tmp` can have security considerations on top of performance if Antimony is not `setuid`. If running as a regular user, Antimony’s cache folder will globally accessible to all programs running as the user. With `setuid`, Antimony can protect write-access to to its temporary cache.

### Older Implementations

Antimony is the final iteration of a several year long object to create fast, usable, and function sandboxes for Linux. This project initially started as a Shell Script, borrowing from an example provided on the [Arch Wiki](https://wiki.archlinux.org/title/Bubblewrap) describing a way to coordinate a `bubblewrap` invocation with `xdg-dbus-proxy` to get Portals to work outside of Flatpak. That script eventually became too complicated, and turned into SB, a Python program. Speed and complexity eventually lead to a re-implementation in C++. 

Antimony breaks off from SB (Only sharing a name and general goal), allowing a stark departure from the shell script roots. Despite that, all three programs serve the same purpose, and can thus be bench-marked against each other. 

All test are run on an identical, Arch Virtual Machine. The raw numbers are not important—the difference between them are.

| Profile Hot | SB*   | SB++** | Antimony*** | Improvement |
| ----------- | ----- | ------ | ----------- | ----------- |
| Chromium    | 104.0 | 7.8    | 3.7         | 2.1X        |
| Zed         | 102.2 | 7.1    | 3.0         | 2.4X        |
| Okular      | 100.8 | 7.5    | 2.8         | 2.7X        |
| Syncthing   | 98.2  | 6.2    | 2.2         | 2.8X        |

*Comparison between Hot Startup, in Milliseconds. Each application has cached definitions, and this benchmark largely shows how quickly the program can read its caches and launch bubblewrap.*

| Profile Cold | SB*    | SB++** | Antimony*** | Improvement |
| ------------ | ------ | ------ | ----------- | ----------- |
| Chromium     | 862.5  | 633.8  | 521.1       | 1.2X        |
| Zed          | 418.2  | 177.8  | 45.6        | 3.9X        |
| Okular       | 3792.9 | 2107.6 | 1604.8      | 1.3X        |
| Syncthing    | 170.4  | 37.0   | 25.9        | 1.4X        |

*Comparison between Cold Startup, in Milliseconds. Each application has its cache removed prior to execution.*

\* SB is run via `benchmark.sh python main $PROFILE` from the [SB](https://github.com/kkernick/sb) repository.
\** SB++ is run via `benchmark.sh cpp main $PROFILE`.
\*** Antimony is run via `bench $PROFILE` from this repository, using a system installation from `deploy`.

## Techniques

Antimony needs to do a lot of things very quickly. Creating a sandbox, especially a secure one, takes time, but the primary objective of Antimony in terms of speed is being unnoticeable compared to running it natively. The most expensive tasks Antimony performs, in descending order, include:
1. SOF Library Resolution 
2. Shell Script Parsing
3. Proxy Setup
4. SECCOMP Setup

### Caching
Caching has been a hallmark of every implementation; spawning a new process (Such as `ldd`, or `find`), is very expensive for the speed that we hope to achieve, and as such storing the output in a file that can be subsequently fetched (Including if the same command is run multiple times during a single instance), dramatically speeds up performance.

Antimony’s departure from the Command Line and toward Profiles allows it to add an additional cache in feature/integration speedup. Whereas older versions had to parse the command line each time the program was run, Antimony can cache the end state of a Profile, after all the various operations have been performed. Profile’s are not only more versatile than the command line, but is faster.

Antimony’s caching is so aggressive that hot-startup is nothing more than loading a handful of files from disk, then executing the the sandbox. 
### Thread Pools
The most significant performance boost was implemented between the Python and C++ implementation, where a Thread Pool was used aggressively for almost every operation. Antimony uses *rayon* for its thread pool implementation.

### SOF
The SOF is arguably the most unique, and most expensive feature of Antimony. Running an application natively gives it access to the entire file-system, but most importantly the system’s library directory. Being able to access every binary and library on the system greatly increases the attack surface of the application, as horizontal movement on the system is trivial, and with the amount of packages installed on an average system all but guaranteeing that a vulnerability is available for exploit.

Flatpak’s Runtimes are for portability, not security, and they make no effort to reduce the attack surface outside of general, vast collections. It’s better than running it natively, but it could still be better.

Antimony only includes the libraries necessary for the program to function. There is no list of such dependencies, and collecting this information requires recursive calls to `ldd`, alongside Features that include the various runtime libraries that aren’t linked on startup. Once a list is collected, those libraries still need to be provided to the sandbox.

Generating the list has been optimized through the thread pool, and heavy cache usage. Antimony supports both wildcards and directories, and utilizes `find` to resolve all executable. These results are cached, and massive directories like `/usr/lib/qt6` can be directly mounted into the sandbox.

The SOF is a directory on the host that is mounted on the sandbox at `/usr/lib`. It contains all the libraries the sandbox needs. The original implementation created copies of the system libraries, leading to dreadful performance. C++ improved this with *hard-links*. By placing the SOF on the same file-system as `/usr/lib` (In this case `/usr/share`), Antimony makes setting up the SOF a no-op; all that needs to be done is reference an existing `inode`. By utilizing the `CAP_FOWNER` capability, Antimony can make this more reliable while still able to fall back to hard-links (Such as when `AT_HOME` is not on the same filesystem). 

### Shell Scripts
Antimony strives to work with as little configuration as possible; ideally, Profiles would not even need to exist, and a sandbox could be created for any application and work without providing unnecessary resources.

The most valuable source of information given to Antimony is the executable itself. While ELF binaries can be resolved with `ldd`, Shell Scripts cannot—yet they contain a trove of valuable information about needed binaries, paths, and libraries. Interpreting shell scripts is done by the shell, with a standardized syntax for the `sh` shell that most shells obey. To parse these, Antimony uses `bash`, specifically for resolving internal variables with their various esoteric, complicated syntax. 

This parsing does execute portions of the shell script, but only those which defined a variable that is used elsewhere, such as `HOST=$(cat /etc/hostname)`. Running untrusted software in a sandbox is not safe, and Antimony presumes that you trust anything you run within it.

Digesting a shell script in this manner can only be done linearly, with Antimony resolving and replacing variables to uncover paths and binaries. It generally does a good job, but it’s not a proper shell interpreter, and it’s not efficient. By using caching and threads, this process can be significantly speed up.

### Proxy Setup
If you use IPC, Antimony mediates a subset of the user’s bus via `xdg-dbus-proxy`. The sandbox cannot be created until the proxy has set itself up, and while it’s a fast process this is still the most expensive part of sandbox startup. Rather than naively looping until the proxy exposes its bus, Antimony leverages `inotify` to wait only for as long as necessary, while also launching the proxy as soon as possible in the startup process, letting it setup in the background as Antimony continues with other components.

### SECCOMP Setup
SB++ introduced support for SECCOMP Filters, leveraging `bubblewrap`’s ability to apply provided filters to the sandbox. Because Bubblewrap expected a filter in BPF format, caching was part of the requirement as  files needed to be created on disk. 

Antimony’s SECCOMP support is [much better](SECCOMP.md) than SB++’s, leveraging a custom built interface for the Notify framework to ensure more reliable, permission-less capturing. Part of this required using SECCOMP directly, rather than passing it to Bubblewrap, which allowed Bubblewrap itself to be confined. 

Antimony uses an SQLite database to store syscall information—far faster than the naive file-based solution in earlier implementations—and can produce Filters faster than SB++ could, despite not caching it to disk. This is on top of superior logging and generation, and a more reliable storage medium via a database.
