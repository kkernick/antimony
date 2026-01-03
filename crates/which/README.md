# Which 

```rust
// Resolve a path
let time = std::time::Instant::now();
assert!(which::which("antimony").unwrap() == "/usr/bin/antimony");
let elapsed = time.elapsed();

// Return from the cache immediately.
let time = std::time::Instant::now();
which::which("antimony");
assert!(time.elapsed() < elapsed);
```

This crate implements the `which` command, hyper-optimized and specific for Antimony. This differs from the `which` crate on Cargo, and the shell built-in in three ways:

1. This implementation prioritizes *speed*, and is expected to be run multiple times on the same command. Internally, results are cached such that repeated calls to `which(binary)` will resolve immediately. Antimony makes a lot of calls to this function with the same argument.
2. This implementation uses `rayon` to search the PATH with multiple threads, and does not respect the ordering of the variable. It is searching for *a* match, not the first.
3. It assumes the user wants an executable, so if an explicit path is provided, it is assumed to be executable and returned immediately. 
