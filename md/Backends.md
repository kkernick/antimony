# Backends

Antimony supports using two backends for storing configuration (IE Profiles and Features), and caches:

1. The *File Store*, where objects are stored as standard files. For example, the Configuration Datastore stores files in `$AT_HOME/config`, while the Cache Datastore stores files in `$AT_HOME/cache`
2. The *Database Store*, where objects are stored within a SQLite database, located in `$AT_HOME/db`.

Because SQLite is already a dependency for SECCOMP, there is no extra configuration required, and configuring the Backend can be done at run-time. The settings are controlled in the Global Configuration, specifically via  the `config_store` and `cache_store` keys, or the `AT_CONFIG_DB` and `AT_CACHE_DB` to toggle Database and File Stores.

However, directly modifying the Configuration is not recommended, as it does not handle converting an existing Store to the selected one. For that, you’ll want to use `antimony backend`, which not only handles digesting the existing Store into the desired format, but also setting the Configuration so the new store is used. 

Choosing between the different Backend depends on your specific setup—specifically your disk and filesystem. In general, slower disks without any sort of caching benefit from using a database, as it reduces reading and opening operations. Conversely, solid-state drives, or file systems that employ caching for hot data (IE ZFS) see little benefit from using a database, and the added overhead can actually incur a performance *penalty*.

Calling `antimony backend` without a desired backend to switch to will automate testing, performing a `refresh` and dry-run of all installed profiles (So make sure to use `antimony integrate` before hand). This will present you with quantifiable data on which Backend is best suited for your system and setup.