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









