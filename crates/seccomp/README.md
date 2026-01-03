# SECCOMP

This crate allows for the creation and enforcement of SECCOMP filters. It is a wrapper on the C `libseccomp` library, and as such you will need the library installed at build-time. Though built for Antimony, this crate can be used in other projects.

For most intents and purposes, you’ll be interacting with this crate via `Filter`, which handles initialization, configuration, and enforcement of SECCOMP policies. For example:

```rust
use seccomp::{filter::Filter, action::Action, attribute::Attribute, syscall::Syscall};

// Create a new filter that kills the process by default
let mut filter = Filter::new(Action::KillProcess).unwrap();

// Deny new privileges
filter.set_attribute(Attribute::NoNewPrivileges(true)).unwrap();

// Allow execve.
filter.add_rule(Action::Allow, Syscall::from_name("execve").unwrap());

// Load the policy, effective immediately
filter.load();
```

The `notify` feature can be optionally enabled to support the Notify framework in SECCOMP. You’ll want to look at Antimony’s implementation for more details.

## Overview

SECCOMP is a security mechanism within the Linux Kernel that allows for a process to voluntarily restrict the syscalls that it can use. A SECCOMP policy persists through `execve` calls, and as such the typical usage of SECCOMP is:

1. Define a policy for a child process in a parent handler.
2. `fork`
3. Call `seccomp_load` in the child clone.
4. Call `execve`.

The child will then be governed by that profile.

Note: The parent is still confined by the profile between `load` and `execve`, so unless the process is creating a filter for itself, a policy will require `execve` and other syscalls necessary for the parent to execute the child.

## Actions

*Actions* are what the Kernel performs when it runs into a given syscall. When you create a new *Filter*, you must supply a default action for if a syscall is requested that does not have an associated rule. Unless you have good reason, its generally best to adopt a whitelisting approach where the default action is something like *Kill Process*, and then the Filter explicitly allows a subset of syscalls in. This crate exposes all supported actions, including:

* *Kill Process*: Kill the entire process
* *Kill Thread*: Kill the offending thread.
* *Trap*: Raise a `SIGTRAP` signal. This crate does not provide signal handling.
* *Log*: Log to the `audit` log.
* *Allow*: Allow the syscall.
* *Notify*: Send the request to a notifier (Requires the `notify` feature and additional work)
* *Error*: Send a `errno` code back to the caller. Most programs do not expect syscalls to fail, so this often causes the program to terminate.

Actions are also supplied via `Filter::add_rule`

## Attributes

*Attributes* are values that you can configure for a given Filter that modify its behavior. You can set Attributes via `Filter::set_attribute`. This includes:

* *Bad Arch Action*: The *Action* to perform when a syscall is from an architecture that filter did not expect (Filters are not architecture specific, though this crate largely operates under the assumption the Filter will be used for the native architecture).
* *No New Privileges*: This denies the possibility of a process re-enabling privileges, such as through creating and loading a more permissive policy. Unless you are cooperatively creating a profile for a willing application, you should turn this on.
* *Thread Sync*: Force all threads to synchronize when the policy is loaded. This ensures that the policy will be enforced across all threads immediately.
* *Negative Syscalls*: Allow negative syscalls. I do not know why you would want this.
* *Log*: Log all decisions to `audit`, regardless of whether the Action is *Log*.
* *Disable SSB*:  Disables security mitigation. 
* *Optimize*: Define how rules are structured within the Filter. Choices include *Binary Tree* and *Priority + Complexity*. Note that because this crate does not expose the rule arguments (Instead only allowing an *Action*), these will make little difference, as the rules are not particularly complicated.
* *Return System Return Codes*: What it says on the tin.

## Syscalls

*Syscalls* are privileged operations that are performed in kernel-space, such as reading files, forking the process, etc. SECCOMP deals with these operations, and enforces rules on how the kernel should act when specific syscalls are requested. 

Internally, syscalls are 32 bit numbers. However, they also have names, such as `execve`, `fork`, and `fread` The mapping between numbers and names are not consistent between architectures. This crate exposes these operations in the `Syscall` object.

A syscall can be defined either through an explicit numerical value for the native architecture via `Syscall::from_number`, through a name on the native architecture via `Syscall::from_name`, or through a name on a desired architecture via `Syscall::with_arch`. You can then query the number through `Syscall::get_number`, the name through `Syscall::get_name`, or the name with a desired architecture via `Syscall::get_name_arch`.

Syscalls are used in the `Filter::add_rule` function.

## Filters

*Filters* are the programs that contain *Rules* and *Attributes* for a particular SECCOMP policy. They can be enforced immediately, or written as a BPF file. Filters are the primary means of interacting with both SECCOMP as a whole, and this crate in particular.

A Filter is firstly created through `Filter::new`, which takes a default action. From there, Attributes can be set via `Filter::set_attribute`, and Rules can be added via `Filter::add_rule`. This crate only exposes a subset of Rules, taking only a Syscall, and an Action for that Syscall.

The Filter can then be written in BPF format via `Filter::write`, or consumed and immediately loaded for the current program via `Filter::load`.

## Features

### `notify`

With the `notify` feature, Filters can also take advantage of the Notify feature of `libseccomp`. To use this, use `set_notify` on the filter to provide an object that implements the `Notifier` trait. Whenever a `Action::Notify` rule is hit, the Kernel will suspend the calling process, and send a packet across the FD to a monitoring process, which the kernel will then wait for. The monitor can then analyze the syscall, and return instructions for what the Kernel can do.

Antimony uses this feature in the `antimony-monitor` process. It uses Notify to log Syscalls, and also prompt the user on what action should be taken for a given Syscall.

Notify is incredibly powerful, but requires extra effort on your part. There are two challenges:

1. Because the FD is provided on `seccomp_load`, getting the FD to the monitor is difficult. Because SECCOMP applies across threads, creating the monitor within the confined process can cause deadlock—where a Syscall requested by the monitor itself causes the process to suspend as the kernel waits for a response. This leaves you with two options:
	1. Create your monitor such that you know exactly what syscalls it uses, and carve those out in your policy, and run the monitor in the same process. This has the advantage of not needing to transmit the FD, but suffers in that it reduces the efficacy of the policy.
	2. Send the FD across a socket to a dedicated, unconfined monitoring process. This requires carving out the needed syscalls for transmitting the FD (`sendmsg`), but ensures the monitor is not confined by its own notifying policy.
2. You are expected to talk to the Kernel via the FD, and must make decisions for *every* Syscall to which the Notify rule applies. This can be a lot of Syscalls, and can cause performance to slow to a crawl.

`antimony-monitor` is an excellent reference for using this feature, both in the context of this crate, and SECCOMP as a whole. In the case of the former:

1. You’ll want to create structure that implements the `Notifier` trait. This abstracts away most of the above problems. In it, you need to define three functions:
	1. `exempt` returns a list of Syscalls that your Notifier needs to transmit the FD to the monitor. This could be `sendmsg` for a socket.
	2. `prepare` is called immediately before `seccomp_load`. You’ll use this to make any preparations needed to transmit the FD. This could include creating the socket, spawning auxiliary processes, etc.
	3. `handle` is called immediately after `seccomp_load`, where you actually transmit the FD.

Once defined, you can pass an instance of the *Notifier* in the `Filter::set_notifier` function. If you're using `spawn`, simply pass the Filter to `seccomp`. Otherwise, you'll want to call:

1. Call `Filter::prepare` to run your preparation and exemption functions
2. Call `seccomp_load`
3. Call `Filter::handle`
4. Return.
