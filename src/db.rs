use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::model::{FeedPage, Video};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Subscription {
    pub id: String,
    pub title: String,
    pub handle: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedItem {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub url: String,
    pub added_at: i64,
}

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA busy_timeout=5000;
             PRAGMA synchronous=NORMAL;
             PRAGMA foreign_keys=ON;",
        )?;
        Self::migrate(&conn)?;
        Ok(Self { conn })
    }

    fn migrate(conn: &Connection) -> anyhow::Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS subscriptions (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                handle TEXT,
                added_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS read_later (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                title TEXT NOT NULL,
                url TEXT NOT NULL,
                added_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS history (
                video_id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                channel TEXT,
                url TEXT NOT NULL,
                position REAL NOT NULL DEFAULT 0,
                duration REAL,
                watched_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS article_progress (
                url TEXT PRIMARY KEY,
                position REAL NOT NULL,
                updated_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS cache (
                key TEXT PRIMARY KEY,
                fetched_at INTEGER NOT NULL,
                payload TEXT NOT NULL
             );",
        )?;
        Ok(())
    }

    // ----- cache ---------------------------------------------------------

    pub fn cache_get(&self, key: &str) -> Option<(i64, String)> {
        self.conn
            .query_row(
                "SELECT fetched_at, payload FROM cache WHERE key = ?1",
                params![key],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .ok()
            .flatten()
    }

    pub fn cache_put(&self, key: &str, fetched_at: i64, payload: &str) {
        let _ = self.conn.execute(
            "INSERT INTO cache(key, fetched_at, payload) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET fetched_at = excluded.fetched_at,
                                            payload = excluded.payload",
            params![key, fetched_at, payload],
        );
    }

    pub fn cache_feed(&self, key: &str, page: &FeedPage) {
        if let Ok(payload) = serde_json::to_string(page) {
            self.cache_put(key, page.fetched_at, &payload);
        }
    }

    pub fn cached_feed(&self, key: &str) -> Option<FeedPage> {
        let (_, payload) = self.cache_get(key)?;
        serde_json::from_str(&payload).ok()
    }

    pub fn clear_cache(&self) {
        let _ = self.conn.execute("DELETE FROM cache", []);
    }

    // ----- subscriptions -------------------------------------------------

    pub fn subscriptions(&self) -> Vec<Subscription> {
        let mut stmt = match self
            .conn
            .prepare("SELECT id, title, handle FROM subscriptions ORDER BY lower(title)")
        {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([], |row| {
            Ok(Subscription {
                id: row.get(0)?,
                title: row.get(1)?,
                handle: row.get(2)?,
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }

    pub fn subscribe(&self, id: &str, title: &str, handle: Option<&str>) {
        let _ = self.conn.execute(
            "INSERT INTO subscriptions(id, title, handle, added_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET title = excluded.title,
                                            handle = excluded.handle",
            params![id, title, handle, crate::util::time::now_unix()],
        );
    }

    pub fn unsubscribe(&self, id: &str) {
        let _ = self
            .conn
            .execute("DELETE FROM subscriptions WHERE id = ?1", params![id]);
    }

    pub fn is_subscribed(&self, id: &str) -> bool {
        self.conn
            .query_row(
                "SELECT 1 FROM subscriptions WHERE id = ?1",
                params![id],
                |_| Ok(()),
            )
            .optional()
            .ok()
            .flatten()
            .is_some()
    }

    // ----- read later ----------------------------------------------------

    pub fn read_later_add(&self, item: &SavedItem) {
        let _ = self.conn.execute(
            "INSERT INTO read_later(id, kind, title, url, added_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET title = excluded.title, url = excluded.url",
            params![item.id, item.kind, item.title, item.url, item.added_at],
        );
    }

    pub fn read_later_remove(&self, id: &str) {
        let _ = self
            .conn
            .execute("DELETE FROM read_later WHERE id = ?1", params![id]);
    }

    pub fn read_later_contains(&self, id: &str) -> bool {
        self.conn
            .query_row(
                "SELECT 1 FROM read_later WHERE id = ?1",
                params![id],
                |_| Ok(()),
            )
            .optional()
            .ok()
            .flatten()
            .is_some()
    }

    pub fn read_later(&self) -> Vec<SavedItem> {
        let mut stmt = match self
            .conn
            .prepare("SELECT id, kind, title, url, added_at FROM read_later ORDER BY added_at DESC")
        {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([], |row| {
            Ok(SavedItem {
                id: row.get(0)?,
                kind: row.get(1)?,
                title: row.get(2)?,
                url: row.get(3)?,
                added_at: row.get(4)?,
            })
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }

    // ----- history -------------------------------------------------------

    pub fn history_upsert(&self, video: &Video, position: f64, duration: Option<f64>) {
        let _ = self.conn.execute(
            "INSERT INTO history(video_id, title, channel, url, position, duration, watched_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(video_id) DO UPDATE SET
                title = excluded.title,
                channel = excluded.channel,
                url = excluded.url,
                position = excluded.position,
                duration = excluded.duration,
                watched_at = excluded.watched_at",
            params![
                video.id,
                video.title,
                video.channel.name,
                video.watch_url(),
                position,
                duration,
                crate::util::time::now_unix()
            ],
        );
    }

    pub fn video_position(&self, video_id: &str) -> Option<f64> {
        self.conn
            .query_row(
                "SELECT position FROM history WHERE video_id = ?1",
                params![video_id],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten()
    }

    // ----- article progress ---------------------------------------------

    pub fn article_position(&self, url: &str) -> Option<f64> {
        self.conn
            .query_row(
                "SELECT position FROM article_progress WHERE url = ?1",
                params![url],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten()
    }

    pub fn set_article_position(&self, url: &str, position: f64) {
        let _ = self.conn.execute(
            "INSERT INTO article_progress(url, position, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(url) DO UPDATE SET position = excluded.position,
                                             updated_at = excluded.updated_at",
            params![url, position, crate::util::time::now_unix()],
        );
    }
}
