use crate::shared::{
    Set,
    env::{AT_HOME, USER_NAME},
};
use common::cache::{self, CacheStatic};
use dashmap::DashMap;
use parking_lot::{Mutex, MutexGuard};
use rusqlite::{Connection, OptionalExtension, params, types::FromSql};
use serde::{Serialize, de::DeserializeOwned};
use std::{fmt, fs, path::PathBuf, sync::LazyLock, thread::ThreadId};
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
}

#[derive(Copy, Clone)]
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

#[derive(Copy, Clone)]
pub enum Table {
    Profiles,
    Features,
    Wildcards,
    Libraries,
    Binaries,
    Directories,
}
impl fmt::Display for Table {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Profiles => write!(f, "profiles"),
            Self::Features => write!(f, "features"),
            Table::Wildcards => write!(f, "wildcards"),
            Table::Libraries => write!(f, "libraries"),
            Table::Binaries => write!(f, "binaries"),
            Table::Directories => write!(f, "directories"),
        }
    }
}

static SYSTEM_POOL: CacheStatic<ThreadId, Mutex<Connection>> = LazyLock::new(DashMap::default);
pub static SYSTEM_CONNECTIONS: LazyLock<cache::Cache<ThreadId, Mutex<Connection>>> =
    LazyLock::new(|| cache::Cache::new(&SYSTEM_POOL));

static USER_POOL: CacheStatic<ThreadId, Mutex<Connection>> = LazyLock::new(DashMap::default);
pub static USER_CONNECTIONS: LazyLock<cache::Cache<ThreadId, Mutex<Connection>>> =
    LazyLock::new(|| cache::Cache::new(&USER_POOL));

static CACHE_POOL: CacheStatic<ThreadId, Mutex<Connection>> = LazyLock::new(DashMap::default);
pub static CACHE_CONNECTIONS: LazyLock<cache::Cache<ThreadId, Mutex<Connection>>> =
    LazyLock::new(|| cache::Cache::new(&CACHE_POOL));

fn new_connection(db: Database) -> Result<Connection, Error> {
    as_effective!({
        let path = db.path();
        if let Some(parent) = path.parent()
            && !parent.exists()
        {
            fs::create_dir(parent).map_err(|e| Error::Io("creating database", e))?;
        }
        let conn = if !path.exists() {
            let conn = Connection::open(path)?;
            if let Database::User | Database::System = db {
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
            conn
        } else {
            Connection::open(path)?
        };

        conn.pragma_update(None, "journal_mode", "WAL")?;
        Ok(conn)
    })
    .map_err(|e| Error::Errno("user", e))?
}

pub fn get_connection(db: Database) -> Result<MutexGuard<'static, Connection>, Error> {
    let id = std::thread::current().id();
    match db {
        Database::User => match USER_CONNECTIONS.get(&id) {
            Some(connection) => Ok(connection.lock()),
            None => {
                USER_CONNECTIONS.insert(id, Mutex::new(new_connection(db)?));
                Ok(USER_CONNECTIONS.get(&id).unwrap().lock())
            }
        },
        Database::System => match SYSTEM_CONNECTIONS.get(&id) {
            Some(connection) => Ok(connection.lock()),
            None => {
                SYSTEM_CONNECTIONS.insert(id, Mutex::new(new_connection(db)?));
                Ok(SYSTEM_CONNECTIONS.get(&id).unwrap().lock())
            }
        },
        Database::Cache => match CACHE_CONNECTIONS.get(&id) {
            Some(connection) => Ok(connection.lock()),
            None => {
                CACHE_CONNECTIONS.insert(id, Mutex::new(new_connection(db)?));
                Ok(CACHE_CONNECTIONS.get(&id).unwrap().lock())
            }
        },
    }
}

pub fn exists(name: &str, db: Database, tb: Table) -> Result<bool, Error> {
    let result = get_connection(db)?.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM {tb} WHERE name = ?1)"),
        params![name],
        |row| row.get::<_, i32>(0).map(|v| v != 0),
    )?;

    Ok(result)
}

pub fn dump<T: FromSql>(name: &str, db: Database, tb: Table) -> Result<Option<T>, Error> {
    let connection = get_connection(db)?;
    let mut stmt = connection.prepare(&format!("SELECT value FROM {tb} WHERE name = ?1"))?;
    let result: Option<T> = stmt.query_row(params![name], |row| row.get(0)).optional()?;
    if let Some(str) = result {
        Ok(Some(str))
    } else {
        Ok(None)
    }
}

pub fn get<T: DeserializeOwned>(name: &str, db: Database, tb: Table) -> Result<Option<T>, Error> {
    match dump::<String>(name, db, tb)? {
        Some(str) => Ok(Some(toml::from_str(&str)?)),
        None => Ok(None),
    }
}

pub fn store_str(name: &str, value: &str, db: Database, tb: Table) -> Result<(), Error> {
    get_connection(db)?.execute(
        &format!("INSERT OR REPLACE INTO {tb} (name, value) VALUES (?1, ?2)",),
        params![name, value],
    )?;
    Ok(())
}

pub fn store_bytes(name: &str, value: &[u8], db: Database, tb: Table) -> Result<(), Error> {
    get_connection(db)?.execute(
        &format!("INSERT OR REPLACE INTO {tb} (name, value) VALUES (?1, ?2)",),
        params![name, value],
    )?;
    Ok(())
}

pub fn save<T: Serialize>(name: &str, value: &T, db: Database, tb: Table) -> Result<(), Error> {
    get_connection(db)?.execute(
        &format!("INSERT OR REPLACE INTO {tb} (name, value) VALUES (?1, ?2)",),
        params![name, toml::to_string(value)?],
    )?;
    Ok(())
}

pub fn delete(name: &str, db: Database, tb: Table) -> Result<(), Error> {
    get_connection(db)?.execute(&format!("DELETE FROM {tb} WHERE name = ?1"), params![name])?;
    Ok(())
}

pub fn all(db: Database, tb: Table) -> Result<Set<String>, Error> {
    let connection = get_connection(db)?;
    let mut things = Set::default();
    let mut stmt = connection.prepare(&format!("SELECT name FROM {tb}"))?;
    let rows = stmt.query_map([], |row| row.get(0))?;
    for name in rows {
        things.insert(name?);
    }
    Ok(things)
}
