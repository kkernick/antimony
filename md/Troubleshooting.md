## Troubleshooting

Antimony is a complicated program that runs on many different distributions with different system configurations. Fortunately, Antimony is extremely flexible and it is very easy to adapt system definitions to your specific system. This document outlines the most common errors that may occur when using Antimony, and how to fix them.

## Antimony Cannot Find the Application

> [!note]
> Antimony sandboxes programs **already** installed on the system. It is not a means of distributing packages.

Antimony has a suite of system profiles defined that encompass several commonly-used programs to ease usage and having to define your own. However, different distributions package things different, and these changes may not be accounted for. One example is the *name* of the program to run. Consider `ungoogled-chromium`:

* Under Arch, it’s packaged as `chromium`, and conflicts with the upstream project.
* Under Ubuntu, it’s packaged as `ungoogled-chromium`.
* Under Gentoo, it’s packages as `ungoogled-chromium-browser` or `ungoogled-chromium-browser-$SLOT`

Antimony uses inheritance to prevent duplication, and has a small profile for each of these cases, for example:

```toml
path = "/usr/lib64/ungoogled-chromium-browser/chrome"  
inherits = ["chromium"]  
```

However, *your* particular package may use a name/path Antimony doesn’t expect. You have two means of resolving this:

1. Create a *new* profile with the name of the binary on your system, and specify `inherits = ["default", "other_profile"]`
2. Add an explicit `path` to the original profile pointing to the correct location.

> [!note]
> Antimony creates a per-user directory for storing modifications to system profiles; you can always return to the original changes via `antimony remove $MODIFIED`

> [!warning]
> Antimony uses the *name* of the profile for searching for other application folders. For example, `/usr/lib/chromium`. If you simply change the `path`, it may not include the other system resources you need. Use your package manager of choice to list the installed files and ensure the profile name matches the installed files, or add them explicitly.


## The Program Fails to Run / Does Not Work Correctly

There are three culprits here:

### There isn’t a profile defined

If you run `antimony edit $NAME` and you get a markdown document full of comments without any actual definitions, then you’ll need to make a profile by hand. See [Creation](./Creation.md) for details.


### You’re missing a system resource.

#### Trace

Run `antimony run $NAME --feature trace`, and look at the generated report when the program closes. This will detail files that the program asked for that did not exist in the sandbox *but* exist on the host. Additionally, any features that pull that file in will be listed that you can add to the `features` field. Pay special attention to the output. For example:

```
/etc/passwd can be provided with the following features  
       - etc (via /etc) as ro  
       - passwd (via /etc/passwd) as ro
```

Adding the `etc` feature will add the *entire* `/etc` directory to the sandbox. You likely **do not want this**.

```
/dev/pts/1 can be provided with the following features  
       - proton (via /dev) as rw  
       - dev (via /dev) as rw  
       - wine (via /dev) as rw
```

Features like `wine` and `proton` are designed for specific applications, and include more than just this path; particularly for this case, they are extremely permissive features that disable much of the sandbox. If the name of the feature doesn’t make sense for your application, it may not be a good fit.

```
/usr/lib/x86_64-linux-gnu/libgtk-4.so.1 can be provided with the following features  
       - gtk4 (via libgtk-4*) as rx
```

`rx` and especially `wx` can be dangerous. In this case, a library needs to be executed, but allowing profiles to both modify *and* execute (i.e `wx`), can potentially allow the program to run things it shouldn’t.

#### Debug Shell

Run `antimony run $NAME --feature debug-shell` to drop into a shell within the sandboxed environment. This also includes the application and all the resources Antimony added. Some general debugging guidance:

* Run the program from the shell and see if there’s any obvious output that may suggest a problem. For example, if you’re running a graphical application, but there is no Wayland socket, you may see something like `qt.qpa.plugin: Could not load the Qt platform plugin "wayland" in "" even though it was found.`
* Check the filesystem for obvious omissions. Use your package manager on the host to list the files installed by the package and its dependencies, and ensure they are present. If you’re missing a library, try using `--library 'libXYZ*'` from the command line to add it. If that works, add it to the profile (Or find a feature that does it for you).

### The Profile is Using an Unsupported/Unconfigured Security Feature

Particularly:
* If you’re using `seccomp = "Enforcing"`, ensure you have properly generated a complete profile with `Notifying`/`Permissive` first.
* Some features, like `hardened_malloc` can cause programs to misbehave—especially when enforced globally as Antimony does. Many features, like `glycin` explicitly conflict with this, but you may need to provide a `conflicts = ["hardened_malloc"]`  to the profile.
* Your program executes programs outside the typical binary roots, or fetches and runs applications. For example, Zed downloads extensions/LSPs to `.local/share/zed` and executes them; Antimony blocks this using Landlock. If you trust the program, you can allow this behavior in the `files.permissions` field:

```toml
[files.permissions]  
"~/.local/share/zed" = "wx"
```

> [!note]
> `wx` is not strictly necessary here, as the profile’s home is already mounted `rw`; the important addition here is `x`. Additionally, remember that Landlock only specifies MAC permission, it cannot overrule DAC (i.e If you provided the home via `ReadOnly`, it would be bind-mounted as `ro`, and `wx` would not allow writes.


### Integration Does not Do Anything

Your application probably ships a different desktop file than Antimony expects. Find the `.desktop` file corresponding to the application within `/usr/share/applications` and ensure `id` is set to that value in the Profile. 

### Applications Cannot Connect via HTTP/$PORT 

This is by design. If your kernel supports Landlock, the majority of web-enabled profiles use the `https` feature, which only permits connecting over `443`. You can either:

1. Switch `https` for `network`.
2. Add the port to `ipc.ports.connect`.