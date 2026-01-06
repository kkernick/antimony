# Antimony Source

This folder contains the Antimony Source Tree. Additional functionality exists within the `crates` folder within the root of this repository. This contains the core logic of the `antimony` executable, organized into the following folders:

* `cli`: Contains the definitions for the command line parsing.
* `fab`: Contains fabricators, which compose parts of the sandbox, and are cached together.
* `setup`: Contains the core logic for setting up the sandbox environment.
* `shared`: Contains shared functionality between the various sub-folders. Chiefly among these is the `profile` sub-folder, which contains the core definitions and logic of the Profile.

