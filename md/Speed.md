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

|       | Chromium | Zed   | Okular | Syncthing | Sh    |
| ----- | -------- | ----- | ------ | --------- | ----- |
| 2.4.1 | 121.0    | 121.3 | 120.5  | 113.1     | 112.1 |
| 2.4.2 | 121.4    | 120.4 | 120.7  | 112.5     | 112.4 |
| 2.4.3 | 120.6    | 120.2 | 121.1  | 112.6     | 112.0 |
| 2.5.0 | 120.4    | 122.0 | 121.9  | 112.6     | 111.4 |
| 2.6.0 | 34.0     | 24.8  | 27.2   | 14.9      | 14.5  |
| 3.0.0 | 32.1     | 25.1  | 26.2   | 16.1      | 14.6  |
| 4.0.0 | 33.1     | 26.5  | 28.3   | 15.3      | 16.0  |
| 4.1.0 | 34.0     | 27.8  | 28.9   | 15.5      | 15.3  |
| 4.1.1 | 33.3     | 27.4  | 28.4   | 14.7      | 16.3  |
| 4.2.0 | 33.6     | 27.9  | 28.5   | 16.4      | 16.0  |
| 4.2.1 | 33.1     | 27.2  | 29.0   | 15.6      | 15.9  |
| 5.0.0 | 31.0     | 23.5  | 28.5   | 13.7      | 13.3  |
| 5.0.1 | 26.5     | 21.3  | 25.9   | 11.5      | 10.8  |
| 5.1.0 | 28.4     | 21.0  | 25.5   | 11.9      | 11.3  |
| 5.1.1 | 27.3     | 22.1  | 25.2   | 11.6      | 11.3  |
| 5.2.0 | 27.8     | 23.5  | 24.8   | 11.5      | 11.2  |
| 5.2.1 | 27.5     | 22.6  | 24.7   | 11.2      | 11.0  |
^HistoryHot

>[!info]
>Versions prior to 2.6.0 had a busy loop that would sleep for 100ms. This is why these versions are significantly skewed. 

|       | Chromium | Zed  | Okular | Syncthing | Sh   |
| ----- | -------- | ---- | ------ | --------- | ---- |
| 2.6.0 | 1.00     | 1.00 | 1.00   | 1.00      | 1.00 |
| 3.0.0 | 0.94     | 1.01 | 0.96   | 1.08      | 1.01 |
| 4.0.0 | 0.97     | 1.07 | 1.04   | 1.03      | 1.10 |
| 4.1.0 | 1.00     | 1.12 | 1.06   | 1.04      | 1.06 |
| 4.1.1 | 0.98     | 1.10 | 1.04   | 0.99      | 1.12 |
| 4.2.0 | 0.99     | 1.13 | 1.05   | 1.10      | 1.10 |
| 4.2.1 | 0.97     | 1.10 | 1.07   | 1.05      | 1.10 |
| 5.0.0 | 0.91     | 0.95 | 1.05   | 0.92      | 0.92 |
| 5.0.1 | 0.78     | 0.86 | 0.95   | 0.77      | 0.75 |
| 5.1.0 | 0.84     | 0.85 | 0.94   | 0.80      | 0.80 |
| 5.1.1 | 0.80     | 0.89 | 0.93   | 0.78      | 0.78 |
| 5.2.0 | 0.82     | 0.95 | 0.91   | 0.77      | 0.77 |
| 5.2.1 | 0.81     | 0.91 | 0.91   | 0.75      | 0.76 |
^HotNormalized


```chart
type: line
id: HotNormalized
tension: 0.5
spanGaps: true
```

#### Cold

`cargo bencher chromium zed okular syncthing sh --recipe release --system --bench cold --checkout tags/VERSION`

|       | Chromium | Zed   | Okular | Syncthing | Sh    |
| ----- | -------- | ----- | ------ | --------- | ----- |
| 2.4.1 | 732.4    | 258.2 | 1291.6 | 225.2     | 205.3 |
| 2.4.2 | 726.0    | 255.3 | 1207.6 | 224.6     | 202.5 |
| 2.4.3 | 647.5    | 176.4 | 1188.5 | 147.9     | 128.4 |
| 2.5.0 | 645.4    | 177.9 | 1207.6 | 148.6     | 128.2 |
| 2.6.0 | 640.1    | 85.9  | 1223.7 | 50.5      | 30.0  |
| 3.0.0 | 500.1    | 109.1 | 2024.3 | 67.5      | 31.4  |
| 4.0.0 | 325.1    | 109.1 | 1571.2 | 36.2      | 30.1  |
| 4.1.0 | 514.7    | 171.6 | 2127.1 | 35.5      | 29.5  |
| 4.1.1 | 338.8    | 112.2 | 1603.6 | 35.7      | 29.8  |
| 4.2.0 | 291.1    | 102.8 | 1131.7 | 34.9      | 28.6  |
| 4.2.1 | 279.4    | 99.2  | 1326.1 | 32.8      | 26.8  |
| 5.0.0 | 213.2    | 76.8  | 364.5  | 24.2      | 21.5  |
| 5.0.1 | 240.7    | 85.4  | 384.7  | 24.2      | 19.4  |
| 5.1.0 | 193.6    | 69.7  | 345.4  | 19.1      | 17.5  |
| 5.1.1 | 201.2    | 57.8  | 346.2  | 19.8      | 18.1  |
| 5.2.0 | 194.8    | 54.9  | 344.1  | 19.6      | 18.2  |
| 5.2.1 | 194.0    | 55.1  | 343.6  | 19.6      | 17.9  |
^HistoryCold

|       | Chromium | Zed   | Okular | Syncthing | Sh   |
| ----- | -------- | ----- | ------ | --------- | ---- |
| 2.6.0 | 1.00     | 1.00  | 1.00   | 1.00      | 1.00 |
| 3.0.0 | 0.78     | 1.27  | 1.65   | 1.34      | 1.05 |
| 4.0.0 | 0.51     | 1.27  | 1.28   | 0.72      | 1.00 |
| 4.1.0 | 0.80     | 2.00  | 1.74   | 0.70      | 0.98 |
| 4.1.1 | 0.53     | 1.31  | 1.31   | 0.71      | 0.99 |
| 4.2.0 | 0.45     | 1.20  | 0.92   | 0.69      | 0.95 |
| 4.2.1 | 0.44     | 1..15 | 1.08   | 0.65      | 0.89 |
| 5.0.0 | 0.33     | 0.89  | 0.30   | 0.48      | 0.72 |
| 5.0.1 | 0.38     | 0.99  | 0.31   | 0.48      | 0.65 |
| 5.1.0 | 0.30     | 0.81  | 0.28   | 0.38      | 0.58 |
| 5.1.1 | 0.31     | 0.67  | 0.28   | 0.39      | 0.60 |
| 5.2.0 | 0.30     | 0.64  | 0.28   | 0.39      | 0.61 |
| 5.2.1 | 0.30     | 0.64  | 0.28   | 0.39      | 0.60 |
^ColdNormalized

```chart
type: line
id: ColdNormalized
tension: 0.5
spanGaps: true
```

#### Refresh

>[!info]
>These benchmarks use an identical command to above, with the exception of excluding `sh`. It’s inclusion in the prior benchmarks was explicitly to test a non-profile application, and prior versions had a bug where integrated applications without profiles would cause `refresh` to error. 

`cargo bencher chromium zed okular syncthing --recipe release --system --bench refresh --checkout tags/VERSION`

|       | Refresh |
| ----- | ------- |
| 2.4.1 | 1631.4  |
| 2.4.2 | 1627.4  |
| 2.4.3 | 1564.3  |
| 2.5.0 | 1571.8  |
| 2.6.0 | 1728.4  |
| 3.0.0 | 2310.3  |
| 4.0.0 | 1756.4  |
| 4.1.0 | 2376.6  |
| 4.1.1 | 1510.2  |
| 4.2.0 | 1785.7  |
| 4.2.1 | 1983.9  |
| 5.0.0 | 426.3   |
| 5.0.1 | 456.4   |
| 5.1.0 | 433.2   |
| 5.1.1 | 426.1   |
| 5.2.0 | 421.3   |
| 5.2.1 | 420.5   |
^Refresh

```chart
type: line
id: Refresh
tension: 0.5
spanGaps: true
```

