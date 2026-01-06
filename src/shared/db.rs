use crate::shared::{
    Map, Set,
    env::{AT_HOME, USER_NAME},
};
use parking_lot::Mutex;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params, types::FromSql};
use serde::{Serialize, de::DeserializeOwned};
use std::{fmt, fs, path::PathBuf, sync::LazyLock};
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

pub type DatabaseCache = Result<Map<String, String>, Error>;

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
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

static WRITE_USER: LazyLock<Mutex<Connection>> = LazyLock::new(|| {
    Mutex::new(new_connection(Database::User, true).expect("Failed to access User Database"))
});
static WRITE_SYS: LazyLock<Mutex<Connection>> = LazyLock::new(|| {
    Mutex::new(new_connection(Database::System, true).expect("Failed to access User Database"))
});
static WRITE_CACHE: LazyLock<Mutex<Connection>> = LazyLock::new(|| {
    Mutex::new(new_connection(Database::Cache, true).expect("Failed to access User Database"))
});

thread_local! {
    pub static USER_DB: Connection = new_connection(Database::User, false).expect("Failed to access User Database");
    pub static SYS_DB: Connection = new_connection(Database::System, false).expect("Failed to access User Database");
    pub static CACHE_DB: Connection = new_connection(Database::Cache, false).expect("Failed to access User Database");
}

fn new_connection(db: Database, write: bool) -> Result<Connection, Error> {
    as_effective!({
        let path = db.path();
        if let Some(parent) = path.parent()
            && !parent.exists()
        {
            fs::create_dir(parent).map_err(|e| Error::Io("creating database", e))?;
        }
        let conn = if !path.exists() {
            let conn = if write {
                Connection::open(path)?
            } else {
                Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?
            };

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
            conn
        } else if write {
            Connection::open(path)?
        } else {
            Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?
        };

        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "temp_store", "MEMORY")?;
        conn.pragma_update(None, "cache_size", "-20000")?;
        conn.set_prepared_statement_cache_capacity(100);
        Ok(conn)
    })
    .map_err(|e| Error::Errno("user", e))?
}

pub fn execute<T, F>(db: Database, f: F) -> Result<T, Error>
where
    F: FnOnce(&Connection) -> Result<T, Error>,
{
    match db {
        Database::User => USER_DB.with(|c| f(c)),
        Database::System => SYS_DB.with(|c| f(c)),
        Database::Cache => CACHE_DB.with(|c| f(c)),
    }
}

pub fn write_execute<T, F>(db: Database, f: F) -> Result<T, Error>
where
    F: FnOnce(&Connection) -> Result<T, Error>,
{
    let mutex = match db {
        Database::User => WRITE_USER.lock(),
        Database::System => WRITE_SYS.lock(),
        Database::Cache => WRITE_CACHE.lock(),
    };
    f(&mutex)
}

pub fn exists(name: &str, db: Database, tb: Table) -> Result<bool, Error> {
    execute(db, |db| {
        Ok(db.query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM {tb} WHERE name = ?1)"),
            params![name],
            |row| row.get::<_, i32>(0).map(|v| v != 0),
        )?)
    })
}

pub fn dump<T: FromSql>(name: &str, db: Database, tb: Table) -> Result<Option<T>, Error> {
    execute(db, |db| {
        let mut stmt = db.prepare(&format!("SELECT value FROM {tb} WHERE name = ?1"))?;
        let result: Option<T> = stmt.query_row(params![name], |row| row.get(0)).optional()?;
        if let Some(str) = result {
            Ok(Some(str))
        } else {
            Ok(None)
        }
    })
}

pub fn dump_all(db: Database, tb: Table) -> Result<Map<String, String>, Error> {
    execute(db, |conn| {
        conn.execute_batch("BEGIN IMMEDIATE;")?;
        let mut map = Map::default();
        let mut stmt = conn.prepare_cached(&format!("SELECT name, value FROM {tb}"))?;
        let rows = stmt.query_map(params![], |row| {
            let name: String = row.get(0)?;
            let value: String = row.get(1)?;
            Ok((name, value))
        })?;
        for pair in rows {
            let (name, value) = pair?;
            map.insert(name, value);
        }
        conn.execute_batch("COMMIT;")?;
        Ok(map)
    })
}

pub fn get<T: DeserializeOwned>(name: &str, db: Database, tb: Table) -> Result<Option<T>, Error> {
    match dump::<String>(name, db, tb)? {
        Some(str) => Ok(Some(toml::from_str(&str)?)),
        None => Ok(None),
    }
}

pub fn store_str(name: &str, value: &str, db: Database, tb: Table) -> Result<(), Error> {
    write_execute(db, |db| {
        db.execute(
            &format!("INSERT OR REPLACE INTO {tb} (name, value) VALUES (?1, ?2)",),
            params![name, value],
        )?;
        Ok(())
    })
}

pub fn store_bytes(name: &str, value: &[u8], db: Database, tb: Table) -> Result<(), Error> {
    write_execute(db, |db| {
        db.execute(
            &format!("INSERT OR REPLACE INTO {tb} (name, value) VALUES (?1, ?2)",),
            params![name, value],
        )?;
        Ok(())
    })
}

pub fn save<T: Serialize>(name: &str, value: &T, db: Database, tb: Table) -> Result<(), Error> {
    write_execute(db, |db| {
        db.execute(
            &format!("INSERT OR REPLACE INTO {tb} (name, value) VALUES (?1, ?2)",),
            params![name, toml::to_string(value)?],
        )?;
        Ok(())
    })
}

pub fn delete(name: &str, db: Database, tb: Table) -> Result<(), Error> {
    write_execute(db, |db| {
        db.execute(&format!("DELETE FROM {tb} WHERE name = ?1"), params![name])?;
        Ok(())
    })?;
    Ok(())
}

pub fn all(db: Database, tb: Table) -> Result<Set<String>, Error> {
    execute(db, |db| {
        let mut things = Set::default();
        let mut stmt = db.prepare(&format!("SELECT name FROM {tb}"))?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        for name in rows {
            things.insert(name?);
        }
        Ok(things)
    })
}
