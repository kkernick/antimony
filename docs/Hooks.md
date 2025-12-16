# Hooks

Antimony supports *Hooks*, which are arbitrary commands run on the host before and after the sandbox is run. They can be used to prepare the host for sandbox execute, run an auxiliary program alongside the sandbox, and cleanup the system after the sandbox is finished. 

Hooks are run as the regular user (IE the user invoking Antimony).

>[!note] 
>Antimony already prepares and cleans up the sandbox; you do not need to use hooks to manage the sandbox state yourself.

Hooks are defined in the profile, particularly using either the `[[hooks.pre]]` or `[[hooks.post]]` header. Hooks are evaluated and executed in the order they appear in the profile. You can define as many hooks are necessary. For example:

```toml
[[hooks.pre]]  
path = "/usr/bin/prepare"  
attach = true  
system = "User"  
env = true  
can_fail = false  
  
[[hooks.pre]]  
content = """  
echo "Starting up!"
"""  
attach = false  
system = "User"  
env = true  
can_fail = false

[[hooks.post]]
content = """
echo "Shutting down!"
"""
attach = false
system = "System"
env = false
can_fail = true
```


## Hook Components

## Path/Content
Antimony can execute hooks in one of two ways:

1. Execute a binary provided via the `path` argument. You will need to ensure that the binary is executable, and available to the chosen user.
2. Execute a shell-script provided via the `content` argument. This script is parsed via `/usr/bin/bash`

>[!warning]
>You must provided *either* a `path`, or `content`. If both are provided, the `path` is used.

## Attach

Pre-Hooks can additionally toggle the `attach` option. By default, Hooks are blocking. Antimony will run each Pre-Hook in the order they appear, and will not launch the sandbox until all have finished executing. However, if you want a binary to run *alongside* the sandbox, such as keeping a service alive while the instance is running, or providing IPC to the host at large, you can toggle `attach`. Any hooks running in this state will be sent `SIGTERM` when the sandbox exits. Post-Hooks do not support `attach`.

This setting is optional, and defaults to `false`.

## Environment

The `env` setting toggles whether the environment is shared with the hook. By default, the environment is stripped before running the hook, meaning it has no access to environment variables. If you require variables like `AT_HOME`, `HOME`, or the XDG variables, enable this setting.

Regardless of this setting, Antimony will provide the hook with the following environment variables:
* `ANTIMONY_NAME` will be set to the name of the current profile.
* `ANTIMONY_CACHE` will point to the system directory of the current profile. Note, that because hooks run as the user, not the system, this directory will be read-only to the hook.
* `ANTIMONY_HOME` will point to the profile’s dedicated home folder, if it exists. The profile must have `home.policy` set to either `Enabled` or `Overlay`.

The setting is optional, and defaults to `false`.

## Can Fail

By default, if any hook fails, Antimony will bail execution. For Pre-Hooks, this means the sandbox will not launch. For Post-Hooks, this will abort before sandbox cleanup. 

If your hook is not essential, this behavior can be relaxed by setting the `no_fail` flag.  Failures in hooks will be ignored.

## Examples

### Managed Dependencies

One use for hooks it to spawn sub-processes in a pre-hook, which profiles can use to ensure such dependencies are gracefully cleaned after it has finished. For example, the `syncthingtray-qt6` profile needs Syncthing to be running. While a user could configure Syncthing to start on login, it can also be tied to the Profile, such that Syncthing only runs while the Tray does, and will cleanup when the Tray exits:

```toml
[[hooks.pre]]  
path = "antimony"  
args = ["run", "syncthing"]  
attach = true  
env = true
```


In this case, we’re running Syncthing itself under Antimony (Which requires `env`), and then attach it, such that the Profile won’t hang on Syncthing, and let the Tray start right after. When we close the tray, Syncthing closes with it.

### Parent For Web Services

Another use-case is when a service is sandboxed, to have a web-browser or similar application that interfaces with it launch in accordance. For example, we could invert the above Syncthing example, where the `syncthing` profile has a parent hook for `syncthingtray-qt6`, which would largely have the same behavior. Another example is `yarr`. It’s a simple RSS feeder that binds to port 7070. While we could again have this profile start in the background on user login, and then just open a web browser when we need to, we could instead make the web-browser a Parent hook. In this case, when we run the profile, it will launch the web-browser automatically, and when we close the web-browser, the sandbox will cleanup:

```toml
[hooks.parent]  
path = "antimony"  
args = ["run", "chromium", "http://localhost:7070"]  
env = true  
```

Again, we use Antimony to sandbox the browser as well. In this setup, the `yarr` profile automatically launches `chromium`, and when that instance of Chromium closes, the profile goes with it. You can use the `--create-desktop` argument for the `integrate` sub-command to create a `.desktop` entry for `yarr`, that way you can launch both service and browser from your DE.

### Encrypted Homes

With both Pre and Post hooks, its trivial to encrypt a profile’s home folder by simply:

1. Creating a Pre-Hook that decrypts the home and mounts it where Antimony expects.
2. Creating a Post-Hook that un-mounts the encrypted Root. 

For example, `gocryptfs` can be used to create an encrypted home with only user-permission. You’ll first need to create an encrypted folder via `gocryptfs -init`. This will need to be located somewhere accessible by your user, such as `~/antimony-enc/PROFILE`. If you want to launch the profile from your DE, you’ll also need some sort of dialog program to fetch the password, such as `kdialog`.

One good use for this would be a specialized configuration of your web-browser. The power of [[Configurations]] is that you can silo your various uses for an application, so you could have one configuration of your web-browser for general browsing, another one that’s locked down and ephemeral, and another one that saves your email credentials. For the latter case, you may want to encrypt it. Another power of Configurations is that they get their own Hooks, too!

```toml
[configuration.email.home]
name = "HOME_NAME"
policy = "Enabled"

[[configuration.email.hooks.pre]]  
content = 'kdialog --password "Enter the password" | gocryptfs PATH_TO_ENC $ANTIMONY_HOME'  
  
[[configuration.email.hooks.post]]  
content = "umount $ANTIMONY_HOME"  
```

If your `PATH_TO_ENC` needs to resolve the environment, such as `$HOME`, you’ll need to pass `env = true` to the Pre-Hook. But other than that, these two hooks will get the job done. In fact, you could make the Pre Hook check the existence of the encryption root, and through `kdialog` *initialize* the root via `gocryptfs` as well.
