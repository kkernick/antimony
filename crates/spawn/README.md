# Spawn

This crate is used to spawn and manage processes. It is conceptually similar to `std::process::Command`, but deviates in that:

1. It is Linux specific.
2. It asynchronously manages the process via a `Handle`.
3. It treats the processes’ standard output and error as a `Stream` which can be asynchronously retrieved.
4. It supports SetUID and SECCOMP.
5. It supports passing File Descriptors.
6. It supports in-place calls, such as `mode` to consume and return the object, and also `mode_i` to modify it in-place without consuming it.
7. It supports caching.

## Spawner

Setting up a process is done via the `Spawner` object. The name of the process is defined in `Spawner::new`, and the process can be configured before launching. This includes:

* `input/input_i`: Control how Standard Input is handled.
* `output/output_i`: Control how Standard Output is handled.
* `error/error_i` Control how Standard Error is handled.

These three options take a `StreamMode` value, which can be:

* `Pipe`: Collect the contents in a buffer that can be asynchronously queried by the parent.
* `Share`: Use the parent’s stream (IE child output will be displayed on the parent’s Standard Out)
* `Log(level)`: Send the stream to the program `log::Log` implementation. If the log level is lower than the level specified, the stream is discarded.
* `Discard`: Send the contents to `/dev/null`.

Other options include (With a `_i` variant for a non-consuming version):

* `mode` (Requires `user` feature): Set the user mode of the child process. The Spawner utilizes `user::drop` to ensure the child cannot revert their user mode.
* `elevate` (Requires `elevate` feature): Call the process with `pkexec` to prompt and provide administrative privilege. 
* `preserve_env`: Preserve the parent’s environment with the child.
* `env`: Pass a key-pair to the child for its environment. 
* `pass_env`: Pass the environment variable as defined in the environment.
* `seccomp` (Requires `seccomp` feature): Run the child under a specific SECCOMP Policy.
* `fd` (Requires `fd` feature): Give ownership of a FD, or FD-like object, to the Spawner, and provide it to the Child.
* `associate`: Associate another process `Handle` to the `Spawner`, such that they are dropped together.
* `cap`: Permit a capability in the child
* `caps:` Permit a capability set for the child.
* `new_privileges`: Allow the child to assume new privileges.

And finally: 

 * `cache_start/cache_write/cache_read` (Requires `cache` feature): Record arguments, save them to a file, then restore them on subsequent usage.

Arguments can be passed via three methods (With `_i` options):

* `arg` passes a single C-String-like value.
* `args` passes a list of C-String-like values.
* `fd_arg` passes a FD-like object, a C-String-like value, and passes the string `arg FD` to the child.

The in-place and consuming variants of these functions can be used interchangeably. For example:

```rust
let proxy = spawn::Spawner::abs("/usr/bin/bwrap")
.name("proxy")
.mode(user::Mode::Real).args([
		"--new-session",
		"--ro-bind", "/usr/bin/xdg-dbus-proxy", "/usr/bin/xdg-dbus-proxy",
]).unwrap();

let sof_str = "sof";
proxy.args_i(["--ro-bind-try", &format!("{sof_str}/lib"), "/usr/lib"]).unwrap();
let path = &format!("{sof_str}/lib64");
if std::path::Path::new(path).exists() {
		proxy.args_i(["--ro-bind-try", path, "/usr/lib64"]).unwrap();
} else {
		proxy.args_i(["--symlink", "/usr/lib", "/usr/lib64"]).unwrap();
}
```

Once a process is ready to launch, `Spawner::spawn()` will consume the structure and return a `Handle`.

## Handle

The `Handle` object represents a running or finished process, defined by the `Spawner` that created it. Internally, a Handle is composed of three primary values:

1. The PID of the child.
2. A `Stream` for each file stream enabled in the `Spawner` via `output/error`.
3. A `File` for Standard Input, if defined via `Spawner::input`

### Stream

The `Stream` object represents either the Standard Output or Standard Error of a child. You can interact with it in two ways, and only if you set the corresponding flag to `Pipe` on the `Spawner`:

* Retrieve the `Stream` directly via `Handle::output` or `Handle::error`.
* Block until the child exits, then return the full contents of the `Stream` via `Handle::error_all` and `Handle::output_all`.

Retrieving the `Stream` directly allows asynchronous, direct communication with the read end of the Pipe. This includes:

* `Stream::read_line`: Collect a single line from the child. It’s only blocking if a full line is not yet available.
* `Stream::read_bytes`: Collect the specified amount of bytes from the child. It’s only blocking if the required amount is not available.
* `Stream::read_all`: Block and collect all bytes until the child closes their end of the Pipe.
* `Stream::wait`: Wait for the child to close their end. This does not consume the object.

### Input

The `Handle` exposes a child’s Standard Input as a simple `File`. It also implements the `Write` trait, which means you can call `write!` directly on the `Handle` to write to the child’s input, such as:

```rust
use std::io::Write;
let mut handle = spawn::Spawner::new("cat").unwrap()
		.input(spawn::StreamMode::Pipe)
		.output(spawn::StreamMode::Pipe)
		.spawn().unwrap();

let string = "Hello, World!";
write!(handle, "{string}").unwrap();
handle.close().unwrap();

let output = handle.output().unwrap().read_all().unwrap();
assert!(output.trim() == string);
```

The `Handle::close` function will close the Standard Input pipe, which is necessary as many programs will continue to listen on the Pipe until it closes.

### Communication

Communicating with the process can be done with the following methods:

* `Handle::wait`: Wait for the child to exit (Or the parent was interrupted by a signal), and return the exit code. This is blocking. You can also use `wait_blocking` for signal-unsafe settings, and `wait_timeout` for a timeout option.
* `Handle:alive`: Check if the child is still alive. This function is non-blocking.
* `Handle::signal`: Send the specified signal to the child.

### Management

The `Handle` itself can be managed in several ways:

* `Handle::name` will return the name of the child.
* `Handle::pid` will return the PID.
* `Handle::detach` will consume the `Handle` and return the PID without performing any cleanup. Usually, when the `Handle` falls out of scope, it will send `SIGTERM` to the child, and wait for it to exit. This function detaches the child entirely, which means it will not block cleanup, and requires manual management.

## Association

A `Handle` can be *associated* with another `Handle`, such that they will be dropped and cleaned up together. This can be useful for managing a group of processes. The process is as follows:

1. Use `Spawner::associate` to move a `Handle` into the `Spawner`. 
2. Use either `Spawner::get_associate` or `Handle::get_associate` after `Spawner::spawn` to get a mutable reference to the associated `Handle`, allowing you to modify it as you would normally.
3. When the main `Handle` drops, its associated processes are cleaned up with it.

A `Handle` can be given a unique, memorable name via `Spawner::name`, which is used in the `get_associate` of both objects. If no such name is provided, the string passed to `Spawner::new` is used instead (IE the path will not be resolved).

## Features

### `fd`

The FD feature gates access to the `Spawner::fd`, `Spawner::fds`, and `Spawner::fd_arg` functions, along with their in-place versions. These functions allow you to pass a select set of File Descriptors to the child, ensuring that they will remain open and mapped to the same number.

### `elevate`

The Elevate feature gates the `Spawner::elevate` function, which preprends the command with `pkexec` to run it with administrative privilege. The program must be run under a user that can authorize such privilege via Pol-Kit.

### `cache`

The Cache feature gates control to the `Spawner::cache_read`, `Spawner::cache_write` and `Spawner::cache_init` functions for caching the arguments passed to the `Spawner`, such that they can be reused on subsequent runs.

### `user`

The User feature gates control of the `Spawner::mode` function, which allows the child to be run under a specific operating mode for SetUID applications, while also ensuring that cleanup and signal works within this privileged context. 

If you are not using SetUID, this feature is useless. 

### `fork`

The Fork feature gates the highly dangerous and unsafe `Fork` structure for running Rust closures within the child, rather than calling `execve`. 