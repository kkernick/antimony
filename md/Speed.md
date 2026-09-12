# Speed

Antimony is *fast*. This document outlines the techniques used to optimize a very slow process, and to highlight Antimony’s improvements over its predecessors.

>[!tip]
>This document uses extensive use of charts to visualize the data. It’s best viewed in Obsidian with the *Charts* plugin!

## Comparisons

### Configuration

The configuration of Antimony’s installation can have a profound effect on performance. These are divided into *Build Time* configuration, and *Run Time* configuration.

1. *Build Time*: Choosing to compile Antimony yourself, and using `-Ctarget-cpu=native` in your `RUSTFLAGS` optimizing the resulting binary for your architecture, can drastically improve performance. Antimony publishes binaries for each release, but these are tailored to work on all x86 machines. Additionally, further optimization can be squeezed out of the binary for your particular workflow and profiles using `pgo`.
2. *Run Time*: The most important performance consideration is the privileges given to the `antimony` executable, and the location of `AT_HOME`. By default, Antimony creates hard-links for library files in a cache located within `AT_HOME`. If it cannot do this then it will create copies in `/tmp`. This has a drastic toll on performance.

>[!warning]
>Creating files in `/tmp` can have security considerations on top of performance if Antimony is not `setuid`. If running as a regular user, Antimony’s cache folder will globally accessible to all programs running as the user. With `setuid`, Antimony can protect write-access to to its temporary cache.

The following table illustrates the relative performance of applying various configurations. Note that the new-features column applies to all subsequent columns (So after `--system` is first mentioned, it is implied in all further tests).

| Profile (Hot)/ Configuration | Chromium | Zed | Okular | Syncthing | Sh  |
| ---------------------------- | -------- | --- | ------ | --------- | --- |
| Debug                        | 6.0      | 5.8 | 5.5    | 4.2       | 4.2 |
| Debug (System)               | 6.1      | 5.8 | 5.5    | 4.3       | 4.4 |
| Release                      | 4.3      | 4.2 | 3.9    | 2.9       | 3.3 |
| PGO                          | 4.2      | 4.1 | 3.8    | 2.8       | 3.1 |
^ConfigHot

```chart
type: bar
layout: rows
id: ConfigHot
```


| Profile (Real)/ Configuration | Chromium | Zed  | Okular | Syncthing | Sh   |
| ----------------------------- | -------- | ---- | ------ | --------- | ---- |
| Debug                         | 23.3     | 19.8 | 20.7   | 11.3      | 11.1 |
| Debug (System)                | 23.3     | 19.6 | 21.2   | 11.4      | 11.0 |
| Release                       | 18.3     | 14.8 | 15.9   | 7.5       | 7.2  |
| PGO                           | 17.8     | 14.7 | 15.6   | 7.4       | 7.6  |
^ConfigReal

```chart
type: bar
layout: rows
id: ConfigReal
```

| Profile (Cold)/ Configuration | Chromium | Zed  | Okular | Syncthing | Sh   |
| ----------------------------- | -------- | ---- | ------ | --------- | ---- |
| Debug (System)                | 0.27     | 0.17 | 0.37   | 0.88      | 0.76 |
| Release                       | 0.24     | 0.14 | 0.32   | 0.62      | 0.56 |
| PGO                           | 0.23     | 0.14 | 0.31   | 0.59      | 0.54 |
^ConfigCold

```chart
type: bar
layout: rows
id: ConfigCold
```
*Normalized to a Debug, Non-System Build*.
### Older Implementations

Antimony is the final iteration of a several-year-long project to create fast, usable, and function sandboxes for Linux. This project initially started as a Shell Script, borrowing from an example provided on the [Arch Wiki](https://wiki.archlinux.org/title/Bubblewrap) describing a way to coordinate a `bubblewrap` invocation with `xdg-dbus-proxy` to get Portals to work outside of Flatpak. That script eventually became too complicated, and turned into SB, a Python program. Speed and complexity eventually lead to a re-implementation in C++. 

Antimony breaks off from SB (Only sharing a name and general goal), allowing a stark departure from the shell script roots. Despite that, all three programs serve the same purpose, and can thus be bench-marked against each other. 

All test are run on an identical, Arch Virtual Machine. The raw numbers are not important—the difference between them are.

| Profile Hot | SB  | SB++  | Antimony | Improvement |
| ----------- | --- | ----- | -------- | ----------- |
| Chromium    | 1   | 0.075 | 0.036    | 2.1X        |
| Zed         | 1   | 0.069 | 0.029    | 2.4X        |
| Okular      | 1   | 0.074 | 0.028    | 2.7X        |
| Syncthing   | 1   | 0.063 | 0.022    | 2.8X        |
^SBHot

```chart
type: bar
select: [SB++, Antimony]
id: SBHot
```

*Comparison between Hot Startup, normalized to SB. Each application has cached definitions, and this benchmark largely shows how quickly the program can read its caches and launch bubblewrap.*

| Profile Cold | SB  | SB++ | Antimony | Improvement |
| ------------ | --- | ---- | -------- | ----------- |
| Chromium     | 1   | 0.73 | 0.60     | 1.2X        |
| Zed          | 1   | 0.43 | 0.11     | 3.9X        |
| Okular       | 1   | 0.56 | 0.42     | 1.3X        |
| Syncthing    | 1   | 0.22 | 0.15     | 1.4X        |
^SBCold

```chart
type: bar
select: [SB++, Antimony]
id: SBCold
```
*Comparison between Cold Startup, normalized to SB. Each application has its cache removed prior to execution.*

\* SB is run via `benchmark.sh python main $PROFILE` from the [SB](https://github.com/kkernick/sb) repository.
\** SB++ is run via `benchmark.sh cpp main $PROFILE`.
\*** Antimony is run via `cargo bencher $PROFILE` from this repository, using a system installation from `deploy`.

### Older Versions

We can also see how the performance of Antimony has evolved over releases. Attached to this table is an Obsidian Chart block which can visualize the data in a line chart. Results are in milliseconds.

>[!note]
>These values provide a general gauge of performance over time, but do not take into consideration new features or the fact that earlier bugs may have allowed files to be missed, which could be seen here as better performance.

>[!warning]
> To try and normalize these benchmarks to have consistent and comparable metrics, often times newer benchmarks are normalized to an older benchmark. For example, when benching 5.0.0, 4.2.1 will be benched afterwards, and the former will be normalized based on the difference between the 4.2.1 numbers, and the numbers reported here. This means that benchmarks will periodically need to be redone, and risk becoming inaccurate, but the only other alternative is benching *every* version for each update.
> 
> This consideration does not mean the values here are inaccurate for the purposes of comparison, but they may not be accurate in isolation.


#### Hot

`cargo bencher chromium zed okular syncthing sh --recipe release --system --bench hot --checkout tags/VERSION`

|       | Chromium | Zed | Okular | Syncthing | Sh  |
| ----- | -------- | --- | ------ | --------- | --- |
| 2.4.1 |          |     |        |           |     |
| 2.4.2 |          |     |        |           |     |
| 2.4.3 |          |     |        |           |     |
| 2.5.0 |          |     |        |           |     |
| 2.6.0 |          |     |        |           |     |
| 3.0.0 |          |     |        |           |     |
| 4.0.0 |          |     |        |           |     |
| 4.1.0 |          |     |        |           |     |
| 4.1.1 |          |     |        |           |     |
| 4.2.0 |          |     |        |           |     |
| 4.2.1 |          |     |        |           |     |
| 5.0.0 |          |     |        |           |     |
| 5.0.1 |          |     |        |           |     |
| 5.1.0 |          |     |        |           |     |
| 5.1.1 |          |     |        |           |     |
| 5.2.0 |          |     |        |           |     |
| 5.2.1 |          |     |        |           |     |
| 5.3.0 |          |     |        |           |     |
^HistoryHot

>[!info]
>Versions prior to 2.6.0 had a busy loop that would sleep for 100ms. This is why these versions are significantly skewed. 

|       | Chromium | Zed | Okular | Syncthing | Sh  |
| ----- | -------- | --- | ------ | --------- | --- |
| 2.6.0 |          |     |        |           |     |
| 3.0.0 |          |     |        |           |     |
| 4.0.0 |          |     |        |           |     |
| 4.1.0 |          |     |        |           |     |
| 4.1.1 |          |     |        |           |     |
| 4.2.0 |          |     |        |           |     |
| 4.2.1 |          |     |        |           |     |
| 5.0.0 |          |     |        |           |     |
| 5.0.1 |          |     |        |           |     |
| 5.1.0 |          |     |        |           |     |
| 5.1.1 |          |     |        |           |     |
| 5.2.0 |          |     |        |           |     |
| 5.2.1 |          |     |        |           |     |
| 5.3.0 |          |     |        |           |     |
^HotNormalized


```chart
type: line
id: HotNormalized
tension: 0
spanGaps: true
```

```chart
type: bar
id: HotNormalized
layout: rows
select: ["5.0.1", "5.1.0", "5.1.1", "5.2.0", "5.2.1", "5.2.2"]
```
#### Cold

`cargo bencher chromium zed okular syncthing sh --recipe release --system --bench cold --checkout tags/VERSION`

|       | Chromium | Zed | Okular | Syncthing | Sh  |
| ----- | -------- | --- | ------ | --------- | --- |
| 2.4.1 |          |     |        |           |     |
| 2.4.2 |          |     |        |           |     |
| 2.4.3 |          |     |        |           |     |
| 2.5.0 |          |     |        |           |     |
| 2.6.0 |          |     |        |           |     |
| 3.0.0 |          |     |        |           |     |
| 4.0.0 |          |     |        |           |     |
| 4.1.0 |          |     |        |           |     |
| 4.1.1 |          |     |        |           |     |
| 4.2.0 |          |     |        |           |     |
| 4.2.1 |          |     |        |           |     |
| 5.0.0 |          |     |        |           |     |
| 5.0.1 |          |     |        |           |     |
| 5.1.0 |          |     |        |           |     |
| 5.1.1 |          |     |        |           |     |
| 5.2.0 |          |     |        |           |     |
| 5.2.1 |          |     |        |           |     |
| 5.3.0 |          |     |        |           |     |
^HistoryCold

|       | Chromium | Zed | Okular | Syncthing | Sh  |
| ----- | -------- | --- | ------ | --------- | --- |
| 2.6.0 |          |     |        |           |     |
| 3.0.0 |          |     |        |           |     |
| 4.0.0 |          |     |        |           |     |
| 4.1.0 |          |     |        |           |     |
| 4.1.1 |          |     |        |           |     |
| 4.2.0 |          |     |        |           |     |
| 4.2.1 |          |     |        |           |     |
| 5.0.0 |          |     |        |           |     |
| 5.0.1 |          |     |        |           |     |
| 5.1.0 |          |     |        |           |     |
| 5.1.1 |          |     |        |           |     |
| 5.2.0 |          |     |        |           |     |
| 5.2.1 |          |     |        |           |     |
| 5.3.0 |          |     |        |           |     |
^ColdNormalized

```chart
type: line
id: ColdNormalized
tension: 0
spanGaps: true
```

```chart
type: bar
id: ColdNormalized
layout: rows
select: ["5.0.1", "5.1.0", "5.1.1", "5.2.0", "5.2.1", "5.2.2"]
```

#### Refresh

>[!info]
>These benchmarks use an identical command to above, with the exception of excluding `sh`. It’s inclusion in the prior benchmarks was explicitly to test a non-profile application, and prior versions had a bug where integrated applications without profiles would cause `refresh` to error. 

`cargo bencher chromium zed okular syncthing --recipe release --system --bench refresh --checkout tags/VERSION`

|       | Refresh |
| ----- | ------- |
| 2.4.1 |         |
| 2.4.2 |         |
| 2.4.3 |         |
| 2.5.0 |         |
| 2.6.0 |         |
| 3.0.0 |         |
| 4.0.0 |         |
| 4.1.0 |         |
| 4.1.1 |         |
| 4.2.0 |         |
| 4.2.1 |         |
| 5.0.0 |         |
| 5.0.1 |         |
| 5.1.0 |         |
| 5.1.1 |         |
| 5.2.0 |         |
| 5.2.1 |         |
| 5.3.0 |         |
^Refresh

```chart
type: line
id: Refresh
tension: 0.5
spanGaps: true
```

```chart
type: bar
id: Refresh
layout: rows
select: [5.0.0, 5.0.1, 5.1.0, 5.1.1, 5.2.0, 5.2.1, 5.2.2]
tension: 0.5
spanGaps: true
```
