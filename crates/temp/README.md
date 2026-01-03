# Temp

This crate implements a factory for creating Temporary Objects, currently Files and Directories, and ensuring their deletion after falling out of scope.

`temp::Builder` is used to create any struct that implements the `temp::Object` and `temp::BuilderCreate` trait.  This crate defines two such implementations:

* `temp::File`: uses `fs::File::create` and `fs::remove_file`. 
* `temp::Directory` uses `fs::create_dir_all` and `fs::remove_dir_all`.

All Objects are held within the `Temp` structure, which mediates access to the underlying Object, as well as ensuring its deletion upon leaving scope. Instances of `Temp` are constructed through the `Builder` struct containing the following member functions:

* `owner` (Requires the `user` feature) sets the owner of the Object, ensuring it is created by this user, and also deleted by it.
* `within` dictates the path the Temporary Object is created within. By default, it will use `/tmp`.
* `name` dictates the name of the Temporary Object. By default, it will use a randomized string.
* `extension` dictates an optional file extension, useful for syntax highlighting, that is appended to the name.
* `make` dictates whether the Object should actually be created upon instantiation. In cases of objects like Sockets, it may be needed to have another call, such as `bind` create the object, while still relying on `Temp` to clean it up.

The Builder is consumed with the `create` call, which assembles the `Temp` instance. It accepts a template generic to define which instance of the Object we are creating. For example:

```rust
let file = temp::Builder::new().name("new_file").create::<temp::File>().unwrap();
assert!(std::path::Path::new("/tmp/new_file").exists());
```


## Features

### `user`

The User Feature gates the `Builder::owner` function to allow creating/deleting the Temporary Object with a specified user mode. This is useless unless your application is SetUID.