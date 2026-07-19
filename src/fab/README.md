# Fabricators

Fabricators are specialized routines that construct a specific root folder or functionality within the sandbox. Unlike the general Setup routines, they are cached so subsequent runs simply load the cached arguments.

* `bin` is responsible for constructing `/usr/bin` and its symlinks, and resolving binaries used in shell scripts.
* `dev` is responsible for constructing `/usr/dev`.
* `files` is responsible for constructing all files provided in the `files` field (Except for `runtime` and `direct`), which must be constructed on each run
* `lib` is responsible for constructing `/usr/lib`, its symlinks, and all library folders.
* `ns` is responsible for constructing namespaces.
