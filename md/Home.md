# Home

One of Antimony’s primary security methodologies is that of least-privilege and data minimization. Isolating an application from the rest of the system ensures that it only has access to the files it *needs*. By default, Linux runs your programs under your user—a rather banal fact—but this means that every program you run has access to *all* your files. Does your web-browser *need* to read your personal documents? Should your PDF viewer be allowed to just modify your `.bashrc` when it feels like it?

Of course not, but your remediation is limited. MAC frameworks can extend the default DAC permission controls, but they are exceptionally rigid. Antimony offers another approach using XDG Desktop Portals. When you open a file in your application, Antimony instead asks your Desktop Environment to provide a File Chooser dialog, and that file—and only that file—is exposed in the sandbox. This gives you the best of both worlds: You get access to every file in your home like traditional DAC, but the program only gets access to the files it needs in that exact instance. You can open a PDF in your web-browser without giving it access to every PDF on your computer.

However, simply passing through files from the host through Portals and explicit rules can be cumbersome, and all your applications filling up a shared `.config` folder makes a mess of things (Especially applications that don’t respect XDG Data Directories and puts dot files directly in your home). Antimony offers a solution to this: per-profile home folders.

Effectively, if you set `home.policy = "Enabled"`, Antimony will create a folder in `$XDG_DATA_HOME/antimony/$PROFILE`, and mount that into the sandbox. The program will create its configurations and caches in an silo, isolated from your real home, and they will persist between instances.

>[!note]
>Antimony will *never* mount your real home into a sandbox, even if you haven’t defined a profile home. Most of the time, you won’t need to (And shouldn’t), but you can pass it through in the `files` field.
>


But that is only only the tip of the iceberg.

## Mount Policy

By default, the profile home is mounted read-write (Technically, the default is not creating one in the first place, but I digress), however you have several options besides mere `Enabled`:
* `ReadOnly` mounts the home without write privilege. Your program will probably hate this, but it can be useful in choice circumstances.
* `Overlay` mounts the home without write privilege, then places a temporary RAM OverlayFS  on top. Any writes the profile makes will be sent to the RAM upper-dir, and subsequently discarded when the instance exits.
	* This is particularly useful for applications that enforce a single running instance, like Chromium/Electron. Due to the sandbox, these applications cannot simply join the existing instance, so often gives errors or merely fail to launch.

## Path, Name and Home Specialization

You also have considerable control on where the home exists in the host, and where it ends up in the sandbox. *Technically* speaking, the profile home does not even need to be mounted on `/home`. You can specify the `path` value to mount it wherever you like in the sandbox.

Additionally, you can change the path the home resides on the host using the `name` attribute. By default, it uses the profile name, but you can change it to anything—and it will be created in `$XDG_DATA_HOME/antimony/$NAME`. You can also provide paths, such as `name = profile/configuration_a`, and you’ll have the home at `$XDG_DATA_HOME/antimony/profile/configuration_a`.

That also brings us to Home Specialization. If you have a configuration for a profile, you can specify a separate home folder for that configuration. This can be useful to further silo off specific use-cases for your application. For example, you might have a regular profile for your text editor, but also a more powerful variant for running as an IDE. You might want to keep the settings and configuration between these two use-cases separate—as the IDE configuration will need various extensions and tweaks:

```toml
path = "/usr/lib/zed/zed-editor"  
  
[home]  
name = "zed/zed"  
policy = "Overlay"  
...
[configuration.ide.home]  
name = "zed/ide"  
policy = "Enabled"  
lock = true  
lock_policy = "Overlay"  
```

Or, you might want to have a dedicated “Clean” configuration that doesn’t mount any home folder—giving you a clean slate without any customization:

```toml
[home]  
name = "chromium/main"  
policy = "Enabled"  
lock = true  
lock_policy = "Overlay"  
  
[configuration.secure]  
features = ["gocryptfs"]  
  
[configuration.secure.home]  
name = "chromium/secure"  
lock_policy = "Abort"  
  
[configuration.clean.home]  
policy = "None"
```

The above example for Chromium has three separate homes (Albeit one that doesn’t persist on disk) for running in three very different use-cases.

## Profile Locks

As mentioned previously, some applications enforce a single instance—but Antimony kneecaps that by keeping instances from talking to each other. While you can use `Overlay` to create a shared base configuration, it requires some extra work; you need to first run the profile with `Enable` to actually write the base home, then switch back to it if you ever need to update it.

Antimony offers another solution by allowing you to lock the home folder to a single instance via the `lock` attribute. If another instance tries to run, it is blocked by the lock.

>[!note]
>Antimony uses the `flock` syscall, which merely applies a discretionary lock. In other words, the lock is not-enforcing; you—or any other application—can ignore it and write to the home, and only programs designed to check for the lock and act on it (i.e Antimony) are “restricted” by it.

By default, when Antimony detects a lock, it will prompt you with a Notification and ask what you would like to do:
* `Ignore`: Ignore the lock and mount the home anyways. 
* `Unlock`: Remove the lock, and mount the home. `flock` locks only persist between reboots, and Antimony is very good at gracefully cleaning up and removing the lock, but if an instance is forcefully shut down (i.e `SIGKILL`), that lock may persist even when the associated instance is no longer running. If you’re certain this is the case, you can remove the lock.
* `Skip`: Don’t mount the home for this instance, effectively running a clean instance.
* `Overlay`: Mount the home under a Temporary OverlayFS. While the two instances no longer clash with each other over a single home, the first instance can still make changes to the underlying folder.
* `Abort`: Error out from sandbox construction; this is the default action if you let the notification time out.

Additionally, you can modify what Antimony should do when it runs into a lock via the `lock_policy` attribute. This is useful if your profile has a known fallback strategy and you can’t be bothered to select it each time:
* `Notify`: By default, Antimony prompts you with the above notification and waits for a response or timeout.
* `Abort`: Immediately fail should the lock be detected.
* `Overlay`: Mount the home under a Temporary OverlayFS.

Consider the two example TOML files:
* For `zed`, the default home is mounted as `Overlay`, so a Lock is useless (And in fact, is ignored by Antimony). The `ide` configuration, however, needs to write extension data to disk, so needs to be `Enabled`. Zed also only wants a single instance running, and will refuse to open a second instance. To work around this, we lock the home, and open successive instances under an `Overlay`; the primary instance will have updated extensions, but we need to be cognizant that any modifications to the configuration in these instances will be lost.
* For `chromium` the default home is mounted as `Enabled`, as we want to persist login, extension updates, settings, etc. Unfortunately, Chromium does not appreciate multiple Antimony instances, and will spit out an avalanche of error messages should you try and open another one, hence our lock. We set the lock policy to `Overlay` for the same reason as Zed. Our `secure` configuration uses the `gocryptfs` feature, which means our home is actually mounted from an encrypted volume; running multiple instances would cause multiple mounts, and due to its specialized use-case there is no good reason to need multiple instances. Therefore, we abort immediately.

