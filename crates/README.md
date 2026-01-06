# Antimony Crates

This directory contains a collection of private crates used within Antimony. There is no guarantee on versioning and stability. Use at your own risk:

1. `common` includes shared, common functionality used throughout the project.
2. `notify` contains a `log::Log` implementation and FreeDesktop Notification Portal interface.
3. `seccomp` contains a wrapper for `libseccomp`, with particular focus on the Notify component.
4. `spawn` contains a process spawner with focus on SECCOMP, SetUID, and FD functionality.
5. `temp` contains a simple temporary file/folder generator.
6. `user` contains SetUID related functionality.
7. `utilities` contains both build-time and install-time binaries used throughout the project.
8. `which` contains a multi-threaded, cached executable lookup. 