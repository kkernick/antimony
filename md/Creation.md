# Profile/Feature Creation

Inevitably, you will try to run a program that Antimony doesn’t have a profile for. If your program is very simple, this may work, but it will be very barebones. If you are using the program regularly, you’ll probably want to create a profile for it.

## The Basics

Call `antimony edit $NAME` to create a new document for the profile. The select of name is *important*:
1. It should be the name that the package uses for owned resources. Particularly:
	1. Any configurations in `/etc`, such as `/etc/chromium`
	2. Any libraries in `/usr/lib`, or `/usr/libexec` like `/usr/lib/chromium`

The above should be prioritized for the name, as:
* The binary name can be specified in `path` (Which can be looked up in your PATH).
* The desktop file name can be specified in `id`.

Consider `kiwix` as an example:

```toml
path = "kiwix-desktop"
id = "org.kiwix.desktop"
features = ["https", "qt6-webengine", "kf6"]
conflicts = ["hardened_malloc"]
binaries = ["aria2c"]

[home]
policy = "Enabled"
```

Some applications, like `chromium` on Arch, have a matching path, ID, and resource names. Use your package manager of choice to list the package files, and choose the name accordingly.

***

Next, you’ll want to define the [Features](./Features.md) the profile will need. You may not know all the features that are 
needed, and Antimony can help find them, but some basics you’ll probably want to include are:

Toolkits:
* `qt5`/`qt6` if your program is a Qt application. Additionally consider:
	* `kf6` if uses KDE’s libraries/you want better system integration (You’ll probably want this)
	* `qt6` is split into more isolated features, which you may need depending on the application: `qt6-qml`, `qt6-multimedia`, `qt6-sql`, and `qt6-webengine` is a non-exhaustive set. `qt6` is enough to get the program running, though it may fail afterwards, but debugging tools mentioned below will help determine which are needed. You can also look at the package dependencies.
* `gtk3`/`gtk4` if your program is a GTK application. `libadwaita` is a superset of the latter. These applications typically require additional IPC rules that will be detailed below. You may also need other features like `gio`, `gdk-pixbuf,` `glycin`, `gst`, etc. 
* `electron` if your program is Electron-based, or a browser like Chromium/Firefox. 

Languages:
* `python`, if the program is written-in or uses the Python interpreter.
* `rust-devel` if you need `rustc` and other rust tools.
* `gcc` if you need GCC

Libraries
* `hunspell` and `libspelling` for spell-correct.
* `pipewire` for microphone, speaker, screen-casting, and camera (Additional configuration is needed for the latter two)
* `sdl` for SDL
* `webkit` for WebKit
* `wine`/`proton` for running Windows applications

***

Next, ensure the profile has the correct ID. The identifier is used for desktop integration, and should match the application’s desktop file in `/usr/share/applications`.

*** 

Finally, you may want to consider defining a home policy. By default, the sandbox is entirely ephemeral—all changes will be lost. If you want to keep the application’s configurations/settings, consider defining a home policy by setting `home.policy`. This will create a home folder in `$XDG_DATA_HOME/antimony/$PROFILE`. Your options include:
1. `Enabled`  mounts the home `rw`. This is likely what you want in most situations.
2. `ReadOnly` mounts the home `ro`. Your program will almost certainly be irate if you do this, but it can be useful in choice circumstances.
3. `Overlay` mounts the home `rw`, but with all changes being discarded. This is useful if you’ve already set a good configuration in your home via `Enabled`, and no longer want the profile to make changes.

Some applications do not having more than one instance running. While some, like Gnome applications, can coordinate this using IPC, others—particularly Electron—cannot do their typical checks within the sandbox, and can cause the second instance to spew errors. To fix this, you can ask Antimony to enforce that single-instance policy itself. 

The `home.lock` boolean will ask Antimony to use a `futex` lock that other instances can cooperatively block on. By default, this will cause subsequent instances to prompt you and what do to. If you already know the default option, you can specify `home.lock-policy` to apply it automatically. See [Home](./Home.md) for more details.

## Runtime Fixes

With your profile created, its time to tune it and add anything discovered at Runtime.

If your program uses IPC (e.g it’s a graphical application), it will likely be spawned with a helper process—`xdg-dbus-proxy`. This program spoofs the user bus used for IPC communication and only allows a subset of whitelisted options. 

>[!note]
>If your profile has not brought in the proxy via features, you can do so by adding to the `owns`, `talks`, `sees`, `calls` or `portals` fields in `ipc`. 

Run the profile with debug logging to print out the proxy’s output:

`RUST_LOG=debug antimony run $NAME`

You should see three categories of output:

Permitted traffic:

```
[DEBUG spawn] proxy: B43: <- org.freedesktop.DBus return from C25  
[DEBUG spawn] proxy: C26: -> org.freedesktop.DBus call org.freedesktop.DBus.AddMatch at /org/freedesktop/DBus
```

Filtered traffic:

```
[DEBUG spawn] proxy: Filtering message due to arg0 org.kde.kwalletd, policy: 0 (required 1)
```

Denied traffic:

```
[DEBUG spawn] proxy: C16: -> org.freedesktop.portal.Desktop call org.freedesktop.host.portal.Registry.Register at /org/freedesktop/portal  
/desktop  
[DEBUG spawn] proxy: *DENIED* (ping)
```

For filtered and denied traffic, you can permit the IPC if you want the application to access it. For example, if you want the program to be able to access `kwallet`, then you can add `talks = ["org.kde.kwalletd"]` to the `ipc` section of the profile (Though be aware of what this entails).

For `org.freedesktop.portal` addresses, you can enable the associated Desktop Portal via `portals`, such as `portals = ["Desktop"]`.

Typically, you’ll want to set the busses in the `talks` field, which allows for bi-directional communication. `sees` only allows the sandbox to listen, but unable to send messages itself, and `owns` allows the sandbox to create new addresses under the bus. Gnome applications, for example, take ownership of their identifier, but some programs can slip through if their identifier does not match their bus name. You will see this in the log, and the program will likely abort telling you this. This should be the only bus you set to `owns` unless you know what you’re doing (And you shouldn’t touch `calls` at all unless you *especially* know what you’re doing).

***

If your program does not launch, your next step is running it under the tracer:

`antimony run $NAME --feature trace`

This runs the application underneath `strace`, collecting every path accessed (And, more importantly, those *not* accessed), and will provide a helpful report when the sandbox exits. 

> [!warning]
> This will be slow, and will print a lot of text to the screen.


For example:

```
/etc/passwd can be provided with the following features  
       - etc (via /etc) as ro  
       - passwd (via /etc/passwd) as ro  
/var can be provided with the following features  
       - var (via /var) as ro  
/etc/nsswitch.conf can be provided with the following features  
       - etc (via /etc) as ro  
       - network (via /etc/nsswitch.conf) as ro  
/etc/gnutls/config can be provided with the following features  
       - gnutls (via /etc/gnutls) as ro  
       - etc (via /etc) as ro  
/etc/glvnd/egl_vendor.d can be provided with the following features  
       - etc (via /etc) as ro  
/usr/lib/x86_64-linux-gnu/libpipewire-0.3.so.0 can be provided with the following features  
       - pipewire (via libpipewire*) as rx  
/usr/local/share/fonts
```

You are presented with paths that the program tried to access, but that did not exist within the sandbox or that it was not allowed to access. If a feature provides that path, it will be displayed; otherwise, the feature will be bold.

Some general guidance:

1. Do not blindly use features to make the list smaller. The `/etc` feature brings in the entire host `/etc`. `wine` is for running Windows applications underneath Wine, and includes many other files.
2. If you only need `passwd`, use the `passwd` feature; if you only need a small handful of files, provide them directly, or their parent. 
3. Read about the feature with `antimony info --feature $FEATURE` for its description and the resources it provides to see if it may be a good fit.

*** 

You can additionally use the `debug-shell` feature to drop into a debugging shell within the environment:

`antimony run $NAME --feature debug-shell`

For here you can:
* Inspect the filesystem to see if required resources are absent.
* Execute the binary directly.
* Test opening/reading/writable to see if the sandbox is configured correctly.

