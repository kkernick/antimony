# Speed

Antimony is *fast*. This document outlines the techniques used to optimize a very slow process, and to highlight Antimony’s improvements over its predecessors.

>[!tip]
>This document uses extensive use of charts to visualize the data. It’s best viewed in Obsidian with the *Charts* plugin!

## Comparisons

### Configuration

The configuration of Antimony’s installation can have a profound effect on performance. These are divided into *Build Time* configuration, and *Run Time* configuration.

1. *Build Time*: Choosing to compile Antimony yourself, and using `-Ctarget-cpu=native` in your `RUSTFLAGS` optimizing the resulting binary for your architecture, and can drastically improve performance. Antimony publishes binaries for each release, but these are tailored to work on all x86 machines. Additionally, further optimization can be squeezed out of the binary for your particular workflow and profiles using `pgo`.
2. *Run Time*: The most important performance consideration is the privileges given to the `antimony` executable, and the location of `AT_HOME`. By default, Antimony creates hard-links for library files in a cache located within `AT_HOME`. If it cannot do this then it will create copies in `/tmp`. This has a drastic toll on performance.

>[!warning]
>Creating files in `/tmp` can have security considerations on top of performance if Antimony is not `setuid`. If running as a regular user, Antimony’s cache folder will globally accessible to all programs running as the user. With `setuid`, Antimony can protect write-access to to its temporary cache.

The following table illustrates the relative performance of applying various configurations. Note that the new-features column applies to all subsequent columns (So after `--system` is first mentioned, it is implied in all further tests).

| Profile (Hot)/ Configuration | Chromium | Zed | Okular | Syncthing | Sh  | New Feature        |
| ---------------------------- | -------- | --- | ------ | --------- | --- | ------------------ |
| Debug                        | 6.0      | 5.8 | 5.5    | 4.2       | 4.2 | `--recipe dev`     |
| Debug (System)               | 6.1      | 5.8 | 5.5    | 4.3       | 4.4 | `--system`         |
| Release                      | 4.3      | 4.2 | 3.9    | 2.9       | 3.3 | `--recipe release` |
| PGO                          | 4.2      | 4.1 | 3.8    | 2.8       | 3.1 | `--recipe pgo`     |
^ConfigHot

```chart
type: bar
select: [Chromium, Zed, Okular, Syncthing, Sh]
id: ConfigHot
```


| Profile (Real)/ Configuration | Chromium | Zed  | Okular | Syncthing | Sh   | New Feature        |
| ----------------------------- | -------- | ---- | ------ | --------- | ---- | ------------------ |
| Debug                         | 23.3     | 19.8 | 20.7   | 11.3      | 11.1 | `--recipe dev`     |
| Debug (System)                | 23.3     | 19.6 | 21.2   | 11.4      | 11.0 | `--system`         |
| Release                       | 18.3     | 14.8 | 15.9   | 7.5       | 7.2  | `--recipe release` |
| PGO                           | 17.8     | 14.7 | 15.6   | 7.4       | 7.6  | `--recipe pgo`     |
^ConfigReal

```chart
type: bar
select: [Chromium, Zed, Okular, Syncthing, Sh]
id: ConfigReal
```

| Profile (Cold)/ Configuration | Chromium | Zed  | Okular | Syncthing | Sh   | New Feature        |
| ----------------------------- | -------- | ---- | ------ | --------- | ---- | ------------------ |
| Debug (System)                | 0.27     | 0.17 | 0.37   | 0.88      | 0.76 | `--system`         |
| Release                       | 0.24     | 0.14 | 0.32   | 0.62      | 0.56 | `--recipe release` |
| PGO                           | 0.23     | 0.14 | 0.31   | 0.59      | 0.54 | `--recipe pgo`     |
^ConfigCold

```chart
type: bar
select: [Chromium, Zed, Okular, Syncthing, Sh]
id: ConfigCold
```
*Normalized to a Debug, Non-System Build*.
### Older Implementations

Antimony is the final iteration of a several year long object to create fast, usable, and function sandboxes for Linux. This project initially started as a Shell Script, borrowing from an example provided on the [Arch Wiki](https://wiki.archlinux.org/title/Bubblewrap) describing a way to coordinate a `bubblewrap` invocation with `xdg-dbus-proxy` to get Portals to work outside of Flatpak. That script eventually became too complicated, and turned into SB, a Python program. Speed and complexity eventually lead to a re-implementation in C++. 

Antimony breaks off from SB (Only sharing a name and general goal), allowing a stark departure from the shell script roots. Despite that, all three programs serve the same purpose, and can thus be bench-marked against each other. 

All test are run on an identical, Arch Virtual Machine. The raw numbers are not important—the difference between them are.

| Profile Hot | SB    | SB++ | Antimony | Improvement |
| ----------- | ----- | ---- | -------- | ----------- |
| Chromium    | 104.0 | 7.8  | 3.7      | 2.1X        |
| Zed         | 102.2 | 7.1  | 3.0      | 2.4X        |
| Okular      | 100.8 | 7.5  | 2.8      | 2.7X        |
| Syncthing   | 98.2  | 6.2  | 2.2      | 2.8X        |
^SBHot

```chart
type: bar
select: [SB, SB++, Antimony]
id: SBHot
```

*Comparison between Hot Startup, in Milliseconds. Each application has cached definitions, and this benchmark largely shows how quickly the program can read its caches and launch bubblewrap.*

| Profile Cold | SB  | SB++ | Antimony | Improvement |
| ------------ | --- | ---- | -------- | ----------- |
| Chromium     | 1   | 0.73 | 0.60     | 1.2X        |
| Zed          | 1   | 0.43 | 0.11     | 3.9X        |
| Okular       | 1   | 0.56 | 0.42     | 1.3X        |
| Syncthing    | 1   | 0.22 | 0.15     | 1.4X        |
^SBCold

```chart
type: bar
select: [SB, SB++, Antimony]
id: SBCold
```
*Comparison between Cold Startup, in Milliseconds. Each application has its cache removed prior to execution.*

\* SB is run via `benchmark.sh python main $PROFILE` from the [SB](https://github.com/kkernick/sb) repository.
\** SB++ is run via `benchmark.sh cpp main $PROFILE`.
\*** Antimony is run via `cargo bencher $PROFILE` from this repository, using a system installation from `deploy`.

### Older Versions

We can also see how the performance of Antimony has evolved over releases. Attached to this table is an Obsidian Chart block which can visualize the data in a line chart. Results are in milliseconds.

>[!note]
>These values provide a general gauge of performance over time, but do not take into consideration new features or the fact that earlier bugs may have allowed files to be missed, which could be seen here as better performance.

>[!warning]
>While these benchmarks can be informative, and are immensely useful in finding regressions, there is a risk of over-fitting the benchmark by taking them at face value. For example, version 4.2.1 introduced using the memory backend for single-profile refreshing, such that cache information would be stored in memory until the application was launched, and would then silently commit the data to the actual backend in the background. From a real-world perspective, this effectively eliminates the cost of writing to disk, as it is done *after* the application is launched, and is not resource-intensive to the point that it would degrade the sandbox’s performance. 
>
>*However*, when the sandbox only runs a dummy profile (IE the `dry` feature), or not at all (IE `--dry`), the writing *does* become noticeable, since there is nothing else to do, and as such this would appear as a performance regression in these benchmarks. Note this isn’t an issue with the global refresh, since the memory backend is expressly used to allow the profiles to read/write directly from memory, and then commit the entire cache in one transaction.
>
> This, and the previous note, is all to say take these numbers with as grain of salt. Has Okular really regressed 40% on refreshing? Looking at this table, yes. Taking into account bug fixes, features, and the fact that these numbers invariably become nonsensical as they have to be normalized off each other? Not so clear.

#### Hot

| Profile Hot / Release | Chromium | Zed | Okular | Syncthing | Sh  |
| --------------------- | -------- | --- | ------ | --------- | --- |
| 1.0.0                 | 3.4      | 3.1 | 2.9    | 2.1       | 2.0 |
| 1.0.1                 | 3.4      | 3.1 | 2.9    | 2.2       | 1.9 |
| 1.1.0                 | 3.4      | 3.1 | 2.9    | 2.2       | 1.9 |
| 1.1.1                 | 3.4      | 3.1 | 2.9    | 2.1       | 1.9 |
| 1.1.2                 | 3.4      | 3.1 | 2.9    | 2.1       | 1.9 |
| 1.2.0                 | 3.4      | 3.1 | 3.0    | 2.2       | 2.0 |
| 1.3.0                 | 3.4      | 3.1 | 3.0    | 2.2       | 1.9 |
| 2.0.0                 | 3.5      | 3.2 | 3.1    | 2.3       | 2.1 |
| 2.0.1                 | 3.4      | 3.1 | 2.9    | 2.2       | 1.9 |
| 2.1.0                 | 3.6      | 3.2 | 3.1    | 2.3       | 2.1 |
| 2.2.0                 | 3.7      | 3.3 | 3.1    | 2.3       | 2.2 |
| 2.2.1                 | 3.8      | 3.3 | 3.2    | 2.4       | 2.2 |
| 2.2.2                 | 3.7      | 3.4 | 3.2    | 2.4       | 2.2 |
| 2.3.0                 | 3.6      | 3.3 | 3.2    | 2.4       | 2.2 |
| 2.4.0                 | 4.0      | 3.7 | 3.5    | 2.2       | 2.0 |
| 2.4.2                 | 4.2      | 4.1 | 3.7    | 2.8       | 3.1 |
| 2.4.3                 | 3.6      | 3.2 | 3.3    | 2.3       | 2.0 |
| 2.5.0                 | 3.5      | 3.2 | 3.2    | 2.3       | 2.1 |
| 2.6.0                 | 3.7      | 3.3 | 3.4    | 2.4       | 2.2 |
| 3.0.0                 | 2.9      | 2.6 | 2.7    | 2.3       | 2.3 |
| 4.0.0                 | 4.0      | 3.8 | 3.6    | 2.7       | 3.0 |
| 4.1.0                 | 4.1      | 4.0 | 3.7    | 2.7       | 3.1 |
| 4.1.1                 | 4.1      | 4.1 | 3.7    | 2.7       | 3.1 |
| 4.2.0                 | 4.2      | 4.1 | 3.9    | 2.9       | 3.2 |
| 4.2.1                 | 4.3      | 4.1 | 3.9    | 2.9       | 3.3 |
^HistoryHot

```chart
type: line
id: HistoryHot
tension: 0.5
spanGaps: true
```

#### Cold

| Profile Cold / Release | Chromium | Zed  | Okular | Syncthing | Sh  |
| ---------------------- | -------- | ---- | ------ | --------- | --- |
| 1.0.0                  | 262.2    | 37.7 | 807.6  | 19.3      | 6.6 |
| 1.0.1                  | 254.5    | 38.0 | 805.2  | 19.3      | 6.6 |
| 1.1.0                  | 255.9    | 38.2 | 800.8  | 19.3      | 6.5 |
| 1.1.1                  | 254.7    | 38.7 | 799.6  | 19.5      | 6.6 |
| 1.1.2                  | 257.0    | 37.6 | 791.9  | 19.3      | 6.5 |
| 1.2.0                  | 255.1    | 38.3 | 800.6  | 19.5      | 6.7 |
| 1.3.0                  | 255.8    | 37.7 | 803.2  | 19.5      | 6.6 |
| 2.0.0                  | 256.7    | 38.0 | 797.8  | 19.6      | 6.8 |
| 2.0.1                  | 255.1    | 38.3 | 795.6  | 19.4      | 6.7 |
| 2.1.0                  | 253.3    | 38.1 | 788.8  | 19.4      | 6.8 |
| 2.2.0                  | 258.6    | 39.1 | 793.0  | 19.6      | 6.9 |
| 2.2.1                  | 264.4    | 38.1 | 795.4  | 19.8      | 7.0 |
| 2.2.2                  | 256.6    | 38.8 | 803.1  | 19.8      | 6.9 |
| 2.3.0                  | 256.5    | 39.7 | 791.3  | 19.6      | 7.0 |
| 2.4.0                  | 254.4    | 38.8 | 789.5  | 19.2      | 6.5 |
| 2.4.2                  | 225.7    | 60.8 | 1205.2 | 12.8      | 9.5 |
| 2.4.3                  | 260.3    | 42.2 | 803.1  | 23.5      | 9.9 |
| 2.5.0                  | 261.0    | 42.4 | 808.2  | 23.3      | 9.7 |
| 2.6.0                  | 293.5    | 44.4 | 895.1  | 24.2      | 9.8 |
| 3.0.0                  | 319.6    | 64.3 | 1501.8 | 38.6      | 7.9 |
| 4.0.0                  | 201.4    | 64.0 | 1220.7 | 13.3      | 9.8 |
| 4.1.0                  | 316.2    | 91.0 | 1727.0 | 12.7      | 9.6 |
| 4.1.1                  | 212.4    | 58.8 | 1196.3 | 13.0      | 9.6 |
| 4.2.0                  | 189.9    | 56.5 | 911.9  | 13.1      | 9.5 |
| 4.2.1                  | 188.3    | 55.9 | 1136.3 | 12.5      | 9.1 |
^HistoryCold

```chart
type: line
select: [Chromium]
id: HistoryCold
tension: 0.5
spanGaps: true
```

```chart
type: line
select: [Zed]
id: HistoryCold
tension: 0.5
spanGaps: true
```

```chart
type: line
select: [Okular]
id: HistoryCold
tension: 0.5
spanGaps: true
```

```chart
type: line
select: [Syncthing]
id: HistoryCold
tension: 0.5
spanGaps: true
```

```chart
type: line
select: [Sh]
id: HistoryCold
tension: 0.5
spanGaps: true
```
#### Real

| Profile Real / Release | Chromium | Zed  | Okular | Syncthing | Sh  |
| ---------------------- | -------- | ---- | ------ | --------- | --- |
| 3.0.0                  | 17.1     | 12.8 | 14.0   | 7.0       | 6.6 |
| 4.0.0                  | 17.7     | 14.4 | 15.1   | 7.2       | 7.5 |
| 4.1.0                  | 17.6     | 14.5 | 15.2   | 7.2       | 7.5 |
| 4.1.1                  | 18.1     | 14.6 | 15.5   | 7.3       | 7.7 |
| 4.2.0                  | 18.8     | 15.3 | 13.5   | 7.6       | 7.9 |
| 4.2.1                  | 19.0     | 15.0 | 13.5   | 7.6       | 7.9 |
^HistoryReal

```chart
type: line
id: HistoryReal
tension: 0.5
spanGaps: true
```


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
