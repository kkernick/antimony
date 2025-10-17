# Default and Inheritance

Antimony Profiles are largely composed of Features, which coalesce to create a single definition of files, libraries, binaries, and IPC needed for the sandbox to function. This idea equally applies to profiles themselves, where profiles can be composed of other profiles. There are three such cases:  Inherited Profiles, the Default Profile, and the Command Line Profile.

## Inherited Profiles

You can define a list of other profiles that the current profile should inherit in the `inherit` attribute:
```toml
inherit = ["A", "B", "C"]
```

Inheritance works by filling in the gaps of the calling profile; if an attribute is not defined, it takes the value from the inheritee. Inherited profiles are processed left-to-right, so a feature defined in an earlier profile takes precedence over later ones.

For example, the most common use of this functionality is to create a base profile to which others derive themselves. Consider the relationship between the `zed` profile, and the `zed-preview` profile, where the latter is defined as:
```toml
path = "/usr/lib/zed/zed-editor"
id =  "dev.zed.Zed-Preview"
inherits = ["default", "zed"]
```

`zed-preview` borrows everything from `zed` save the `path`, and `id` because they’re literally the same application—merely a different version. `zed-preview` will take the features, IPC, and all other attributes from `zed`, eliminating duplication. Only three values are not inherited, and they are the three fields defined in `zed-preview`: The path, the ID, and the inherits field itself. Inheritance is *not* recursive, there is at most a single level of inheritance.
## The Default Profile

Each user can create a *Default* Profile that defines a set of common definitions that should be applied to all profiles run by the user.  It operates identically to an inherited profile, and indeed is even defined the same way. You can edit the Default profile by calling `antimony default`.

By default, all applications inherit the Default profile. You can change this by explicitly defining the `inherits` attribute. If you don’t require any inherited profiles, you can set it as an empty list:

```toml
inherits = []
```

This will exclude the Profile from inheriting default values.

## The Command Line Profile

Antimony’s `run` command provides arguments for every component of a profile. You can even run applications without profiles defined:

```bash
antimony run zsh --binaries cat ls vim
```

Relevant to this document, however, is that the Command Line defines its own profile, separate from the profile that may or may not exist attached to the name you provide—`zsh` in this case. The important distinction is that the order of operations is reversed in this case: the command line takes precedence.

So, if your Default profile defines `seccomp = "Permissive"`, and the profile itself defined it as `"Enforcing"`, but you ran the profile with:

```bash
antimony run profile --seccomp disabled
```

It would run without SECCOMP.
