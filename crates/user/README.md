# User

This crate is for management of the Real and Effective Users of a SetUID program. On non-SetUID programs, all functions in this crate are no-ops. Management is controlled via the `Mode` enum, which contains the following values:

* `Real` mandates the Real User, or the user launching the process.
* `Effective` mandates the Effective User, or the user that owns the program.
* `Existing` means different things for different functions.

These values can be directly queried via the `USER` and `GROUP` static values.

## Non-Sync

The following functions are available in `user::`:

* `user::set` will set the operating mode to the desired mode. `Existing` will set the value to `USER` (IE restore to the original value). The existing mode will be returned.
* `user::revert` will set the operating mode back to the original value. This is typically used after `user::set`, when the desired operation for the given mode has been performed.
* `user::restore` returns the operating mode to the value prior to a `user::set` call.
* `user::drop` destructively sets the operating mode to the desired mode by overwriting the Saved User. The program will be unable to return to dual privileges.

On top of these primitive functions, a pair of macros exist which simplify usage:

* `user::run_as!` will execute the block of code under the desired user, then return to the user was in use prior to the macro call. If an error occurs on setting modes, the program aborts.
* `user::try_run_as!` operates identically to `run_as!`, but will throw an `Err` on user errors, and wrap the return value in `Ok`. This function can only be used in functions that return a `Result`.

## Synchronous

The User Mode of a program affects all threads. This can cause many issues in a multi-threaded program, as one thread can switch the mode from under another, to which a operation that would allowed under the prior mode (Such as creating a file) now causes an error. This issue is exasperated when all the threads are changing mode, as it can cause the mode to change between the `user::set` and `user::restore`.

If your program requires synchronization between multiple threads for switching users, the `sync` feature provides a synchronization mechanism in the `user::sync` namespace. These are available via the `run_as!`, and `user::sync_try_run_as!` macros. There are two caveats to using these variants:

1. These macros guarantee that the block will be executed in the provided mode *only* if all threads use the `sync` variants. If another thread uses a regular `user::` function, this guarantee breaks.
2. These macros cannot be nested. Doing so will cause a deadlock. These macros hold a global mutex at the start of the macro, and release it at the end. If the block tries to take ownership of that mutex, it will hang forever.

