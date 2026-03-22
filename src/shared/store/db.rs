//! Functions for interfacing with the databases used by Antimony.
//! There are three:
//!
//! 1. The System Database (antimony.db) contains static definitions of profiles and features
//! 2. The User Database (USER_NAME.db) contains user profiles and features.
//! 3. The Cache Database (cache.db) is a dumping ground for caching used through the project.

use crate::shared::env::{AT_HOME, USER_NAME};
use rusqlite::{Connection, params, types::FromSql};
use std::{any::Any, cell::UnsafeCell, collections::HashMap, fs, path::PathBuf};
use thiserror::Error;
use user::as_effective;

#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O Error: {0}: {1}")]
    Io(&'static str, std::io::Error),

    #[error("System Error: {0}: {1}")]
    Errno(&'static str, nix::errno::Errno),

    #[error("Database Error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Failed to deserialize TOML: {0}")]
    Deserialize(#[from] toml::de::Error),

    #[error("Failed to serialize TOML: {0}")]
    Serialize(#[from] toml::ser::Error),

    #[error("Database could not be initialized: {0}")]
    Initialize(String),
}

/// What Database we're targeting.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum Database {
    User,
    System,
    Cache,
}
impl Database {
    pub fn path(&self) -> PathBuf {
        AT_HOME.join("db").join(match self {
            Self::User => format!("{}.db", USER_NAME.as_str()),
            Self::System => "antimony.db".to_string(),
            Self::Cache => "cache.db".to_string(),
        })
    }
}

pub struct Store {
    connection: UnsafeCell<Connection>,
}
impl Store {
    /// Get a new connection.
    pub fn new(db: Database) -> Result<Self, Error> {
        let connection: Connection = as_effective!(Result<Connection, Error>, {
            let path = db.path();
            if let Some(parent) = path.parent()
                && !parent.exists()
            {
                fs::create_dir_all(parent).map_err(|e| Error::Io("creating database", e))?;
            }

            let conn = Connection::open(&path)?;
            if let Database::System | Database::User = db {
                conn.execute_batch(
                    r#"
                        CREATE TABLE IF NOT EXISTS profiles (
                            name TEXT PRIMARY KEY,
                            value TEXT NOT NULL
                        );
                        CREATE TABLE IF NOT EXISTS features (
                            name TEXT PRIMARY KEY,
                            value TEXT NOT NULL
                        );
                        "#,
                )?;
            } else {
                conn.execute_batch(
                    r#"
                        CREATE TABLE IF NOT EXISTS profiles (
                            name TEXT PRIMARY KEY,
                            value BLOB NOT NULL
                        );
                        CREATE TABLE IF NOT EXISTS features (
                            name TEXT PRIMARY KEY,
                            value BLOB NOT NULL
                        );
                        CREATE TABLE IF NOT EXISTS wildcards (
                            name TEXT PRIMARY KEY,
                            value BLOB NOT NULL
                        );
                        CREATE TABLE IF NOT EXISTS directories (
                            name TEXT PRIMARY KEY,
                            value BLOB NOT NULL
                        );
                        CREATE TABLE IF NOT EXISTS libraries (
                            name TEXT PRIMARY KEY,
                            value BLOB NOT NULL
                        );
                        CREATE TABLE IF NOT EXISTS binaries (
                            name TEXT PRIMARY KEY,
                            value BLOB NOT NULL
                        );
                        "#,
                )?;
            }

            conn.pragma_update(None, "journal_mode", "WAL")?;
            conn.pragma_update(None, "synchronous", "NORMAL")?;
            conn.pragma_update(None, "temp_store", "MEMORY")?;
            conn.pragma_update(None, "cache_size", "-20000")?;
            conn.set_prepared_statement_cache_capacity(100);
            Ok(conn)
        })
        .map_err(|e| Error::Errno("user", e))??;

        Ok(Self {
            connection: UnsafeCell::new(connection),
        })
    }

    /// Get a mutable connection.
    ///
    /// Because all Backing Stores are thread-unique via thread_local!,
    /// and none of them share an underlying connection, there is no
    /// risk in utilizing interior-mutability for the underlying connection.
    ///
    /// We could also fix this by placing the Connection in a Mutex,
    /// but--again--each instance is thread-local, so that would only
    /// impose an unnecessary penalty.
    ///
    #[allow(clippy::mut_from_ref)]
    fn get_connection(&self) -> &mut Connection {
        unsafe { &mut *self.connection.get() }
    }

    fn retrieve<T: FromSql>(&self, name: &str, object: super::Object) -> Result<T, super::Error> {
        let mut stmt = self
            .get_connection()
            .prepare(&format!("SELECT value FROM {object} WHERE name = ?1"))?;
        Ok(stmt.query_row(params![name], |row| row.get(0))?)
    }
}
impl super::BackingStore for Store {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn resident(&self) -> bool {
        false
    }

    fn fetch(&self, name: &str, object: super::Object) -> Result<String, super::Error> {
        self.retrieve(name, object)
    }

    fn bytes(&self, name: &str, object: super::Object) -> Result<Vec<u8>, super::Error> {
        self.retrieve(name, object)
    }

    fn get(&self, object: super::Object) -> Result<Vec<String>, super::Error> {
        let mut things = Vec::default();
        let mut stmt = self
            .get_connection()
            .prepare(&format!("SELECT name FROM {object}"))?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        for name in rows {
            things.push(name?);
        }
        Ok(things)
    }

    fn exists(&self, name: &str, object: super::Object) -> bool {
        self.get_connection()
            .query_row(
                &format!("SELECT EXISTS(SELECT 1 FROM {object} WHERE name = ?1)"),
                params![name],
                |row| row.get::<_, i32>(0).map(|v| v != 0),
            )
            .unwrap_or(false)
    }

    fn store(&self, name: &str, object: super::Object, content: &str) -> Result<(), super::Error> {
        self.get_connection().execute(
            &format!("INSERT OR REPLACE INTO {object} (name, value) VALUES (?1, ?2)",),
            params![name, content],
        )?;
        Ok(())
    }

    fn bulk(
        &self,
        entries: HashMap<String, Vec<u8>>,
        object: super::Object,
    ) -> Result<(), super::Error> {
        // Start a transaction – all inserts succeed or all fail together.
        let tx = self.get_connection().transaction()?;

        {
            let mut stmt = tx.prepare(&format!(
                "INSERT OR IGNORE INTO {object} (name, value) VALUES (?1, ?2)"
            ))?;
            for (name, content) in entries {
                stmt.execute(params![name, content])?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    fn dump(&self, name: &str, object: super::Object, content: &[u8]) -> Result<(), super::Error> {
        self.get_connection().execute(
            &format!("INSERT OR REPLACE INTO {object} (name, value) VALUES (?1, ?2)",),
            params![name, content],
        )?;
        Ok(())
    }

    fn remove(&self, name: &str, object: super::Object) -> Result<(), super::Error> {
        self.get_connection().execute(
            &format!("DELETE FROM {object} WHERE name = ?1"),
            params![name],
        )?;
        Ok(())
    }
}
