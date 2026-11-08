// SPDX-License-Identifier: AGPL-3.0-or-later

//! The local SQLite cache. The services stay the source of truth: what is here is either a copy of
//! their last answer, or what only exists locally (reports of PRs, notifications already sent).

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS snoozes (item_id TEXT PRIMARY KEY, until INTEGER NOT NULL);
    CREATE TABLE IF NOT EXISTS notified (key TEXT PRIMARY KEY, at INTEGER NOT NULL);
";

pub struct Cache {
    connection: Mutex<Connection>,
}

impl Cache {
    /// # Errors
    /// A file that cannot be created or opened.
    pub fn open(path: &Path) -> Result<Self, CacheError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Self::with_connection(Connection::open(path)?)
    }

    /// # Errors
    /// `SQLite` failing to start.
    pub fn in_memory() -> Result<Self, CacheError> {
        Self::with_connection(Connection::open_in_memory()?)
    }

    fn with_connection(connection: Connection) -> Result<Self, CacheError> {
        connection.execute_batch(SCHEMA)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the lock cannot leave SQLite inconsistent.
        self.connection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// # Errors
    /// A database failure, or a stored value of another shape.
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, CacheError> {
        let text: Option<String> = self
            .lock()
            .query_row("SELECT value FROM kv WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(text.map(|text| serde_json::from_str(&text)).transpose()?)
    }

    /// # Errors
    /// A database failure.
    pub fn put<T: Serialize>(&self, key: &str, value: &T) -> Result<(), CacheError> {
        let text = serde_json::to_string(value)?;
        self.lock().execute(
            "INSERT INTO kv (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, text],
        )?;
        Ok(())
    }

    /// Forgets what came from the sources whose key starts with `prefix`.
    ///
    /// # Errors
    /// A database failure.
    pub fn purge(&self, prefix: &str) -> Result<(), CacheError> {
        self.lock()
            .execute("DELETE FROM kv WHERE key LIKE ?1 || '%'", [prefix])?;
        Ok(())
    }

    /// # Errors
    /// A database failure.
    pub fn snooze(&self, item_id: &str, until: DateTime<Utc>) -> Result<(), CacheError> {
        self.lock().execute(
            "INSERT INTO snoozes (item_id, until) VALUES (?1, ?2)
             ON CONFLICT (item_id) DO UPDATE SET until = excluded.until",
            params![item_id, until.timestamp()],
        )?;
        Ok(())
    }

    /// # Errors
    /// A database failure.
    pub fn snoozes(&self) -> Result<HashMap<String, DateTime<Utc>>, CacheError> {
        let connection = self.lock();
        let mut statement = connection.prepare("SELECT item_id, until FROM snoozes")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut snoozes = HashMap::new();
        for row in rows {
            let (id, until) = row?;
            if let Some(until) = DateTime::from_timestamp(until, 0) {
                snoozes.insert(id, until);
            }
        }
        Ok(snoozes)
    }

    /// # Errors
    /// A database failure.
    pub fn was_notified(&self, key: &str) -> Result<bool, CacheError> {
        Ok(self
            .lock()
            .query_row("SELECT 1 FROM notified WHERE key = ?1", [key], |_| Ok(()))
            .optional()?
            .is_some())
    }

    /// # Errors
    /// A database failure.
    pub fn mark_notified(&self, key: &str, at: DateTime<Utc>) -> Result<(), CacheError> {
        self.lock().execute(
            "INSERT OR IGNORE INTO notified (key, at) VALUES (?1, ?2)",
            params![key, at.timestamp()],
        )?;
        Ok(())
    }

    /// # Errors
    /// A database failure.
    pub fn has_notification_history(&self) -> Result<bool, CacheError> {
        Ok(self
            .lock()
            .query_row("SELECT 1 FROM notified LIMIT 1", [], |_| Ok(()))
            .optional()?
            .is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_come_back_as_stored() {
        let cache = Cache::in_memory().unwrap();
        assert_eq!(cache.get::<Vec<u32>>("x").unwrap(), None);
        cache.put("x", &vec![1, 2]).unwrap();
        cache.put("x", &vec![3]).unwrap();
        assert_eq!(cache.get::<Vec<u32>>("x").unwrap(), Some(vec![3]));
    }

    #[test]
    fn purge_only_forgets_its_prefix() {
        let cache = Cache::in_memory().unwrap();
        cache.put("github.prs", &1).unwrap();
        cache.put("todoist.state", &2).unwrap();
        cache.purge("github.").unwrap();
        assert_eq!(cache.get::<u32>("github.prs").unwrap(), None);
        assert_eq!(cache.get::<u32>("todoist.state").unwrap(), Some(2));
    }

    #[test]
    fn snoozes_and_notifications_are_remembered() {
        let cache = Cache::in_memory().unwrap();
        let at = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
        cache.snooze("a", at).unwrap();
        assert_eq!(cache.snoozes().unwrap()["a"], at);
        assert!(!cache.has_notification_history().unwrap());
        assert!(!cache.was_notified("k").unwrap());
        cache.mark_notified("k", at).unwrap();
        cache.mark_notified("k", at).unwrap();
        assert!(cache.was_notified("k").unwrap());
        assert!(cache.has_notification_history().unwrap());
    }
}
