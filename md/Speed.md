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

> [!info] 
> Versions 2.1.0 to 2.2.1 had a regression where both hot/dry implemented a naive wait for one second. Versions 2.2.2 to 2.4.3 reduced this to only 100ms. Both delays has been removed for consistency and to not skew the chart.

>[!note]
>Versions prior to 2.5.0 use a SECCOMP sandbox for compatibility. This *slighly* skews older benchmarks to have better results than later results (As there is no waiting for IPC,  SOF hardlinking/copying is effectively skipped, and the sandbox immediately terminates because of the SOF not existing, rather than using a shim). This is slightly counterbalanced by having to run a Notifier on the benchmark sandbox. Though later versions correctly support the sandboxed environments, versions immediately after 2.5.0 do not, thus the SECCOMP sandbox has been disabled after that point for consistency, even if there may be a slight difference in measurement pre 2.5.0 and post 2.5.0.
>
>Version 3.0.0 added notifying the user on error with a link to the standard error of the sandbox. This presents issue with the benchmarking framework, and has thus been omitted.
#### Hot

< 2.5.0: `cargo bencher chromium zed okular syncthing sh --recipe release --seccomp --checkout tags/VERSION -- --bench hot --manual-refresh`
2.5.0 and Later: `cargo bencher chromium zed okular syncthing sh --recipe release --checkout tags/VERSION -- --bench hot`

> [!note]
> A SECCOMP scaling factor has been applied to more accurately compare the SECCOMP sandboxed and non-SECCOMP runs. There are obtained as 6.0.0 can run under both modes. The below modifiers are subtracted to each value from 2.5.0 onward to account for the lack of IPC (Syncthing and Sh do not use IPC, and while there is some skewing due to `bwrap` never actually setting up, they bias—about 0.8—was not enough to constitute adjustment)

| Chromium | Zed | Okular |
| -------- | --- | ------ |
| 7.9      | 7.6 | 7.3    |

|       | Chromium | Zed  | Okular | Syncthing | Sh   |
| ----- | -------- | ---- | ------ | --------- | ---- |
| 1.0.0 | 13.0     | 11.5 | 11.1   | 12.0      | 10.3 |
| 1.0.1 | 12.6     | 11.5 | 11.3   | 11.4      | 10.4 |
| 1.1.0 | 12.6     | 11.5 | 10.7   | 11.9      | 10.0 |
| 1.1.1 | 13.2     | 11.6 | 10.5   | 12.1      | 10.7 |
| 1.1.2 | 12.8     | 11.9 | 10.5   | 11.6      | 10.5 |
| 1.2.0 | 12.4     | 11.3 | 10.8   | 12.0      | 10.4 |
| 1.3.0 | 12.6     | 11.7 | 10.6   | 11.7      | 10.5 |
| 2.0.0 | 12.6     | 11.9 | 11.1   | 11.8      | 11.0 |
| 2.0.1 | 12.4     | 12.4 | 11.0   | 11.7      | 10.9 |
| 2.1.0 | 20.0     | 20.0 | 17.0   | 17.0      | 17.0 |
| 2.2.0 | 20.0     | 17.0 | 17.0   | 14.0      | 15.0 |
| 2.2.1 | 19.0     | 18.0 | 19.0   | 16.0      | 15.0 |
| 2.2.2 | 23.0     | 22.1 | 20.6   | 20.2      | 21.4 |
| 2.3.0 | 23.7     | 22.0 | 21.6   | 20.1      | 20.3 |
| 2.4.0 | 24.8     | 30.0 | 30.1   | 22.8      | 21.3 |
| 2.4.1 | 23.5     | 22.6 | 22.1   | 22.7      | 22.6 |
| 2.4.2 | 24.0     | 22.9 | 24.2   | 22.3      | 21.0 |
| 2.4.3 | 23.7     | 25.0 | 22.0   | 22.1      | 20.9 |
| 2.5.0 | 26.2     | 22.2 | 24.5   | 12.2      | 10.6 |
| 2.6.0 | 12.4     | 22.0 | 21.6   | 11.0      | 10.4 |
| 4.0.0 | 6.2      | 6.7  | 7.1    | 6.9       | 7.6  |
| 4.1.0 | 6.0      | 7.0  | 7.4    | 6.8       | 7.3  |
| 4.1.1 | 7.7      | 8.7  | 9.4    | 6.7       | 7.3  |
| 4.2.0 | 5.8      | 6.0  | 6.6    | 7.0       | 7.2  |
| 4.2.1 | 6.1      | 6.0  | 6.3    | 7.1       | 7.4  |
| 5.0.0 | 5.2      | 5.5  | 6.3    | 5.8       | 6.1  |
| 5.0.1 | 5.5      | 5.8  | 5.9    | 6.2       | 6.6  |
| 5.1.0 | 6.1      | 6.1  | 7.0    | 6.0       | 6.2  |
| 5.1.1 | 5.9      | 7.1  | 6.7    | 6.1       | 6.6  |
| 5.2.0 | 5.8      | 5.9  | 6.4    | 6.1       | 6.5  |
| 5.2.1 | 5.9      | 5.8  | 6.7    | 6.2       | 6.6  |
| 6.0.0 | 5.6      | 5.9  | 6.1    | 5.4       | 5.6  |


^HistoryHot

```chart
type: line
id: HistoryHot
tension: 0
spanGaps: true
```


#### Cold

< 2.5.0: `cargo bencher chromium zed okular syncthing sh --recipe release --seccomp --checkout tags/VERSION -- --bench cold --manual-refresh`
2.5.0 and Later: `cargo bencher chromium zed okular syncthing sh --recipe release --checkout tags/VERSION -- --bench cold`

|       | Chromium | C (N) | Zed | Z (N) | Okular | O (N) | Syncthing | Y (N) | Sh  | S (N) |
| ----- | -------- | ----- | --- | ----- | ------ | ----- | --------- | ----- | --- | ----- |
| 1.0.0 | 460      | 1.00  | 66  | 1.00  | 1213   | 1.00  | 47        | 1.00  | 21  | 1.00  |
| 1.0.1 | 468      | 1.02  | 66  | 1.00  | 1218   | 1.00  | 47        | 1.00  | 22  | 1.05  |
| 1.1.0 | 451      | 0.98  | 65  | 0.94  | 1237   | 1.02  | 48        | 1.02  | 22  | 1.05  |
| 1.1.1 | 473      | 1.03  | 67  | 1.02  | 1246   | 1.03  | 49        | 1.04  | 23  | 1.10  |
| 1.1.2 | 477      | 1.04  | 67  | 1.02  | 1244   | 1.03  | 49        | 1.04  | 23  | 1.10  |
| 1.2.0 | 475      | 1.03  | 67  | 1.02  | 1221   | 1.01  | 46        | 0.98  | 21  | 1.00  |
| 1.3.0 | 456      | 0.99  | 64  | 0.97  | 1210   | 1.00  | 46        | 0.98  | 21  | 1.00  |
| 2.0.0 | 462      | 1.00  | 67  | 1.02  | 1211   | 1.00  | 48        | 1.02  | 24  | 1.14  |
| 2.0.1 | 459      | 1.00  | 64  | 0.97  | 1225   | 1.01  | 48        | 1.02  | 23  | 1.10  |
| 2.1.0 | 473      | 1.03  | 68  | 1.03  | 1258   | 1.04  | 55        | 1.17  | 26  | 1.24  |
| 2.2.0 | 465      | 1.01  | 69  | 1.05  | 1260   | 1.04  | 50        | 1.06  | 26  | 1.24  |
| 2.2.1 | 467      | 1.02  | 71  | 1.08  | 1252   | 1.03  | 49        | 1.04  | 26  | 1.24  |
| 2.2.2 | 480      | 1.04  | 78  | 1.18  | 1249   | 1.03  | 58        | 1.23  | 33  | 1.57  |
| 2.3.0 | 479      | 1.04  | 75  | 1.14  | 1233   | 1.02  | 33        | 0.70  | 24  | 1.14  |
| 2.4.0 | 471      | 1.02  | 76  | 1.15  | 1233   | 1.02  | 59        | 1.23  | 30  | 1.43  |
| 2.4.1 | 489      | 1.06  | 93  | 1.41  | 1248   | 1.03  | 74        | 1.57  | 50  | 2.38  |
| 2.4.2 | 490      | 1.07  | 94  | 1.42  | 1264   | 1.04  | 78        | 1.66  | 54  | 2.57  |
| 2.4.3 | 482      | 1.05  | 78  | 1.18  | 1220   | 1.01  | 59        | 1.26  | 34  | 1.62  |
| 2.5.0 | 527      | 1.15  | 124 | 1.88  | 1339   | 1.10  | 48        | 1.02  | 25  | 1.19  |
| 2.6.0 | X        | X     | 130 | 1.97  | 1508   | 1.24  | 48        | 1.02  | 23  | 1.10  |
| 4.0.0 | 274      | 0.60  | X   | X     | 1489   | 1.23  | 19        | 0.40  | 18  | 0.86  |
| 4.1.0 | 364      | 0.79  | 119 | 1.80  | 2041   | 1.68  | 19        | 0.40  | 17  | 0.81  |
| 4.1.1 | 234      | 0.51  | 69  | 1.05  | 1381   | 1.14  | 19        | 0.40  | 18  | 0.86  |
| 4.2.0 | 251      | 0.55  | 73  | 1.11  | 1374   | 1.13  | 19        | 0.40  | 17  | 0.81  |
| 4.2.1 | 245      | 0.53  | 67  | 1.02  | 1679   | 1.38  | 19        | 0.40  | 17  | 0.81  |
| 5.0.0 | 234      | 0.51  | 66  | 1.00  | 367    | 0.30  | 13        | 0.28  | 13  | 0.62  |
| 5.0.1 | 232      | 0.50  | 67  | 1.02  | 363    | 0.30  | 13        | 0.28  | 13  | 0.62  |
| 5.1.0 | 175      | 0.38  | 49  | 0.74  | 480    | 0.40  | 12        | 0.26  | 13  | 0.62  |
| 5.1.1 | 175      | 0.38  | 49  | 0.74  | 481    | 0.40  | 12        | 0.26  | 13  | 0.62  |
| 5.2.0 | 175      | 0.38  | 50  | 0.76  | 483    | 0.40  | 13        | 0.28  | 13  | 0.62  |
| 5.2.1 | 172      | 0.37  | 51  | 0.77  | 478    | 0.39  | 12        | 0.26  | 13  | 0.62  |
| 6.0.0 | 62       | 0.13  | 26  | 0.39  | 211    | 0.17  | 9         | 0.19  | 9   | 0.43  |


^HistoryCold


```chart
type: line
select: ["C (N)", "Z (N)", "O (N)", "Y (N)", "S (N)"]
id: HistoryCold
tension: 0
spanGaps: true
```


#### Refresh

>[!info]
> Version 2.1.0 broke refreshing
> 
> Versions 4.1.0 to 5.0.1 omitted `sh` as refreshing could only work on binaries with a profile defined.

< 2.5.0: `cargo bencher chromium zed okular syncthing sh --recipe release --seccomp --checkout tags/VERSION -- --bench refresh`
2.5.0 and Later: `cargo bencher chromium zed okular syncthing sh --recipe release --checkout tags/VERSION -- ---bench refresh`

|       | Refresh |
| ----- | ------- |
| 1.0.0 | 1520    |
| 1.0.1 | 1548    |
| 1.1.0 | 1564    |
| 1.1.1 | 1588    |
| 1.1.2 | 1570    |
| 1.2.0 | 1534    |
| 1.3.0 | 1607    |
| 2.0.0 | 1524    |
| 2.0.1 | 1545    |
| 2.1.0 | X       |
| 2.2.0 | 1573    |
| 2.2.1 | 1569    |
| 2.2.2 | 1599    |
| 2.3.0 | 1620    |
| 2.4.0 | 1560    |
| 2.4.1 | 1608    |
| 2.4.2 | 1618    |
| 2.4.3 | 1601    |
| 2.5.0 | 1733    |
| 2.6.0 | 1880    |
| 4.0.0 | 1691    |
| 4.1.0 | 2395    |
| 4.1.1 | 1690    |
| 4.2.0 | 1768    |
| 4.2.1 | 1875    |
| 5.0.0 | 562     |
| 5.0.1 | X       |
| 5.1.0 | 687     |
| 5.1.1 | 689     |
| 5.2.0 | 688     |
| 5.2.1 | 684     |
| 6.0.0 | 333     |

^Refresh

```chart
type: line
id: Refresh
tension: 0.5
spanGaps: true
```


