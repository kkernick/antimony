# Common

This crate contains various utility functions share across the repository:

1. `cache` contains a generic static container implemented used for caching.
2. `stream` contains functionality needed for sending a File Descriptor across a Unix Socket.
3. `singleton` contains an implementation of a Reentrant Synchronization Singleton for protecting critical paths from multiple threads modifying the global state. 

