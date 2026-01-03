# User

This crate is for management of the Real and Effective Users of a SetUID program. On non-SetUID programs, all functions in this crate are no-ops. Management is controlled via the `Mode` enum, which contains the following values:

* `Real` mandates the Real User, or the user launching the process.
* `Effective` mandates the Effective User, or the user that owns the program.
* `Original` mandates the Operating Mode the program was launched with
* `Existing` mandates whatever Mode is currently in use.

These values can be directly queried via the `USER` and `GROUP` static values.

You can interact with these modes with the following functions:

* `set()` will set the Operating Mode to the one selected, and return what the Mode was prior to the call.
* `current()` returns the current Operating Mode.
* `revert()` will set the Operating Mode to `Original`
* `restore() `will return the Operating Mode to the value returned by `set()`.

## Dropping

`drop()` differs from the above operations in that it destructively sets the Saved UID to the desired mode as well. This means that the program will be unable to return to the dropped mode (IE Calling `drop(Real)` will prevent you from ever returning to Effective, and vice versa.

This is useful in `fork+exec` semantics, where the child can have its privileges dropped to the desired User, rather than allowing it to continue running under the dual SetUID mode of the parent.

## Thread Safety

None of the above functions are thread-safe. If you have multiple threads that are all changing the current Operating Mode, it is entirely possible that the code between a `set` and `restore` call can change from the desired Mode, causing unpredictable and erroneous behavior. For example:

```rust
fn worker(name: &str) {
	let _ = user::as_effective!({
		std::fs::File::create(format!("/tmp/{name}"));
		std::fs::remove_file(format!("/tmp/{name}"));
	});
}

for name in ["t1", "t2", "t3"] {
	std::thread::spawn(|| worker(name));
}

user::set(user::Mode::Real);
std::fs::File::create("/tmp/parent");
std::fs::remove_file("/tmp/parent");
```

There is no guarantee that the parent’s call to `File::create` will run as the Real User, as the detached threads may have changed the mode between `set` and `create`. Additionally, the file may not be removed as the Real user for the same reason, which could cause a Permission Error.

For these kinds of situations, the `run_as` macro (And the `as_effective` and `as_real` wrappers) is required. They utilized a Synchronization Singleton to ensure that only a single thread using these functions can set the user mode at any one time. When the macro finishes, the Singleton is dropped, and another thread can acquire it to change modes. If you don’t need the added overhead of synchronization, such as in single threaded environments, or where you set the mode in the parent before spawning the threads, you can safely use the functions directly.

A safe implementation of the above the example would be:

```rust
fn worker(name: &str) {
	let _ = user::as_effective!({
		std::fs::File::create(format!("/tmp/{name}"));
		std::fs::remove_file(format!("/tmp/{name}"));
	});
}

for name in ["t1", "t2", "t3"] {
	std::thread::spawn(|| worker(name));
}

user::as_real!({
	std::fs::File::create("/tmp/parent");
	std::fs::remove_file("/tmp/parent");
});
```
