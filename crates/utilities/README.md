# Antimony Utilities

>[!warning]
> This crate is not intended for public use.

This crate contains various auxiliary utilities for Antimony. There are two categories:

1. `antimony_` prefixed binaries are intended for use within the repo. They should not be installed with Antimony for a system installation. This includes:
	1. `antimony_bench` to benchmark Antimony against older versions and configurations.
	2. `antimony_completions` to generate shell completions for Antimony at compile-time.
2. `antimony-` prefixed binaries should be installed along with antimony in `/usr/share/antimony/utilities`. This includes:
	1. `antimony-dumper`
	2. `antimony-monitor`.
	3. `antimony-open`
	4. `antimony-spawn`
