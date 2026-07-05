# SOF

One of Antimony’s novel features is that each profile is run with its own copy of your system’s `/usr/lib`, including only the shared libraries the application needs to run. This significantly reduces attack surface, denying horizontal movement by restricting the binaries and libraries a Zero-Day could take advantage of. You can use the `stats` feature to visualize the magnitude of the reduction:

```bash
[~]: antimony run chromium --features stats | grep -e "/usr/bin" -e "/usr/lib"  
/usr/bin => 9  
/usr/lib => 869  
[~]: find /usr/lib -type f | wc -l  
59806  
[~]: find /usr/bin -type f | wc -l  
2458
```

An enormous amount of work has gone into optimizing how Antimony calculates these dependencies, to which this document outlines the general design.

>[!info]
>You can disable the SOF and simply mount your system libraries using `libraries.no_sof`, but you should only do this if your profile fails to work otherwise. The SOF is a core part of Antimony’s design.

## Library Roots

One particular challenge Antimony faces is determining where your system libraries *are*. While there are general some standards, they—like much of the Linux ecosystem—are rather fragmented:
1. The [Filesystem Hierarchy Standard](https://refspecs.linuxfoundation.org/FHS_3.0/fhs/index.html) stipulates that:
	1. `/lib` is a standalone directory that contains essential shared libraries and kernel modules. 
	2. `/usr/lib` is a separate directory for shared libraries.
	3. `/lib<qual>`  and `/usr/lib<qual>`, may exist as separate folders, such as `lib32` or `lib64`.
2. systemd has its [own](https://manpages.debian.org/bullseye/systemd/file-hierarchy.7.en.html) standard:
	1. `/usr/lib` contains architecture agnostic shared libraries.
	2. `/usr/lib/_arch-id_` contains architecture specific libraries, such as Ubuntu’s `/usr/lib/x86_64-linux-gnu`, 
	3. `/lib64` and `/lib` are legacy symlinks to their respective `/usr/` directory. 
`
For Arch, everything points to `/usr/lib`. For Ubuntu, application install their own library folders (i.e `/usr/lib/chromium`), directly to `/usr/lib`, whereas shared libraries are placed in their `arch-id` sub-folder. Additionally, `/usr/libexec` is used by some applications. For Fedora, `lib64` is a distinct folder. 

While Antimony attempted in the past to try and ameliorate Library Root discovery to be an automatic process, it proved either non-exhaustive, or too slow. A particular issue that cropped up was `lib32`. Some applications, like Steam, use `lib32` libraries in an otherwise 64 bit system. An automatic discovery method would require all applications to search this location, despite never using anything inside of it.

The solution ended up being a static list of library roots installed for each distribution, with profiles and feature able to add to this list. Every version of Antimony ships with the base library root of `library_roots = ["/usr/lib"]`. For Arch, this is all that is needed. For Debian/Ubuntu, we add additional roots:

```toml
library_roots = [
  "/usr/lib64",
  "/usr/libexec",
  "/usr/lib/x86_64-linux-gnu",
]
```

Profiles can then extend this list when needed, such as Steam:

```toml
[libraries]
roots = ["/usr/lib32"]
```

## Discovery 

Antimony uses `ldd` to discovery shared libraries. There have been numerous attempts to move to something better, as `ldd`:
1. Requires an `execve` call.
2. Executes the library/binary.

This has included:
1. Trying `objdump`, which only performs static analysis
2. Using `goblin` and manually parsing the files in Rust
3. Using `antimony-dumper` and capturing `dlopen` calls.

However, `ldd` has proven to be the fastest and most comprehensive method available—and one almost universally installed by all systems.

> [!warning]
> As if this still needs to be said: Sandboxing cannot protect you from running malicious software. Antimony will execute your binary on the host using `ldd`.

Antimony has a custom directory crawler that indexes your library roots and their sub-folders, collecting a list of shared libraries that a profile needs. It then runs `ldd` against all of them, coalescing the results into a single list.

### Directories and Application Libraries

Antimony treats library directories like files. Its parser, `find::dir`, crawls the entire directory for libraries and binaries, uses `ldd` against all of them, then produces a list of libraries needed by its contents. It then mounts the folder directly, treating it like a single object.

There are also several places Antimony checks outside your typical roots for application-specific libraries:
1. `/opt` often contains shared libraries for proprietary software, such as `/opt/Obsidian`
2. `/usr/lib` often includes application folders, such as `/usr/lib/chromium`.
3. `/etc` and `/usr/share` usually contain configuration and static files, but some poorly packaged apps may stick libraries inside of them.

## Fabrication

Part of the advantage of using Antimony is you are using your own system libraries, unlike solutions like Flatpak or Snap which ship an entire set of system libraries for sandboxed environments. However, while we now have a list of shared libraries the program needs, getting them exposed to the sandbox in an isolated window is difficult. We cannot simply `--bind` each shared library, as it would slow `bwrap` to a crawl (And often times exceed its maximum argument count). The solution is the titular SOF, a physical folder in Antimony’s cache.

If you have the correct permissions and setup (i.e the Cache folder is located on the same partition as your library roots, and `fs.protected_hardlinks` is not set), Antimony will create hard-links from the original files to the SOF; in other words: it costs nothing. 

If Antimony cannot do that, it does the next best thing and de-duplicates copies across your profiles. It creates a copy of the library in `$CACHE_DIR/shared`, which it then hard-links to; because Antimony owns the copy, you don’t need to tweak `sysctl` to get it working. Then, if another profile has already copied a needed library over, other profiles can freely hard-link to that copy as well.

Antimony does this transparently, and can be mixed for a given profile. If you have a separate home partition and have libraries Antimony needs to provide in a sandbox, it will hard-link to what it can (`/usr/lib`), and de-duplicate what it can’t. The result of which is:

```bash
[~]: du -hc /usr/share/antimony/cache/run | grep "/usr/share/antimony/cache/run$"         
600M    /usr/share/antimony/cache/run
[~]: du -hc /usr/lib /usr/share/antimony/cache/run | grep "/usr/share/antimony/cache/run$"
23M     /usr/share/antimony/cache/run
```

## Cooperative Caching

Because the SOF is a physical directory on the drive, it only needs to be assembled a single time. Subsequent instances can simply mount that existing directory—bringing a “Cold” run measured in the hundreds of milliseconds down to mid-twenties. Of course, this also presents an opportunity for the system libraries and SOF libraries to de-synchronize—but because the SOF already constituted a functioning sandbox, such desync would only prove a problem should the profile binary—or one of the binaries it needs—now links to a newer version of a library.

For these purposes, simply calling `antimony refresh` with your profile is sufficient to regenerated the SOF. Antimony intelligently caches almost every operation it performs, which means that if one instance has already done the work for an expensive operation, other instances—and even other profiles—can benefit from it. For example:

1. Once a profile has created a copy of a library in the shared cache, all other profiles relying on that library can simply make hard-links—mirroring optimal performance.
2. Once a profile has crawled a directory and determined library dependencies, the result is cached on disk. If you run `okular` and calculate the libraries needed for `/usr/lib/qt6`, `gwenview` can borrow that definition.
3. Once a profile has found all matching objects for a particular wildcard—such as `libLLVM*` in your library roots, other profiles can borrow that definition.

This optimization extends even further when you call `antimony refresh` without an argument. This refreshes every profile that you’ve integrated with `antimony integrate`, but rather than sharing a disk cache—which is what would happen if you called `antimony refresh` subsequently for each profile—the cache is shared in *Memory*, and then the entire profile-set’s cache is flushed to disk. This has enormous performance gains—See [here](./Speed.md). For version 5.1.1, running each refresh in isolation would require 643.1 ms, where as running them together only takes 426.1. 