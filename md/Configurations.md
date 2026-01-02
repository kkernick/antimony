# Configurations

A *Configuration* is a sub-profile that acts similarly to an inverted inherited profile. For example, you could create a “Clean” configuration, that disables the profile’s home directory—each launch would be a completely fresh, default state:

```toml
[configuration.clean.home]  
policy = "None"
```

Then, running the configuration is as easy as providing the name with the `--clean` argument:

```bash
antimony run profile --config clean
```

A configuration for a profile shares the name, id, and path of the main profile, but every other component can be modified—the inheritance rule is identical to as if you had specified them on the command line; the above configuration is identical to running: `antimony run profile --home-policy none`.

This allows specialized behavior for different ways you might want to run an application. Consider your text-editor. You use it for opening regular files, but you also use it for development. You could define the regular profile to include nothing but the bare minimum for opening files, with a specialized mode for running it as an IDE:

```toml
path = "/usr/lib/zed/zed-editor"  
id = "dev.zed.Zed"  
features = ["wayland", "dri", "vulkan", "fonts", "shell"]  
  
[home]  
policy = "Overlay"  
  
[ipc]  
portals = ["Settings", "FileChooser", "Documents"]  
  
[files]  
passthrough = "ReadWrite"  
  
[configuration.rust]  
features = ["rust-devel", "network"]  
  
[configuration.rust.home]  
name = "zed-rust"  
policy = "Enabled"
```

There’s a key feature of Configurations: Home Specialization. By merely changing the `home.name` attribute of the Configuration, you can create a separate home folder for the Configuration in `$XDG_DATA_HOME`.

Finally, Configuration can be integrated into your Desktop Environment in two ways:
1. Desktop File can specify *Actions*, such as to “Open a New Window,” or “Open Incognito Window.” They specify different ways to run the application. By default, Antimony will integrate each Configuration into a single desktop file as a Desktop Action. This keeps the profile in one file, and makes it easy to view all the configurations. The "Clean” configuration mentioned above would be a good choice for an Action.
2. One downside of the Desktop Action is that the Profile and its Configurations are treated as a single application (Which it is). This presents a problem for if you want to assign default applications to different configurations. Take our above example: We want text files to be associated with the regular Profile, but want to associate *Rust* files with the Rust profile. To accomplish this, you can use `antimony integrate profile -c file`. This will create a unique desktop file for each configuration, allowing you to treat them as separate applications and assign them to different file types.

