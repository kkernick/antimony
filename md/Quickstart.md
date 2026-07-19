# Quick Start

Run a program under Antimony with `antimony run` (i.e `antimony run bash`).  Some programs require additional information (Contained within [Profiles](./Profiles.md) and [Features](./Features.md)). You can list all such profiles via `antimony list`, with the `--feature` flag to view available features.  If there is no Profile/Feature for your application, you can create one with `antimony edit`; this will open up a TOML document with documentation for all the available options.

You can *Integrate* a profile via `antimony integrate`. For command line applications, like `vim`, this will place a symlink in `$HOME/.local/bin` that points to Antimony. If you have your path configured such that this location takes precedence over the system binaries (e.g `PATH=/home/user/.local/bin:/usr/bin`), calling the binary will automatically run it underneath Antimony. If the Profile is a GUI-application, Antimony will create a desktop file to replace the original, such that launching the program in your Desktop Environment will run it underneath Antimony (A “Native” configuration in the file will allow you to run the system version, if you need it).

Occasionally, when you update your system, Antimony’s [internal library definitions](./SOF.md) will desync with your system. Run `antimony refresh` with either the profile that has errored, or omit the name to refresh every profile currently integrated.

