# CLI

Antimony’s command line is broken up into distinct sub-commands that you invoke immediately after the executable (e.g `antimony integrate`). Each of these sub-commands exist as a discrete file in this directory. 

Additionally, each sub-command implements the `Run` trait, which uses `enum_dispatch` so that Antimony can simply call `.run()` on the CLI command, rather that having to match each variant. 