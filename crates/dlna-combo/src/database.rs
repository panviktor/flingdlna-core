//! Database module for persistent storage
//!
//! Uses SQLite for storing playback positions, watch history, and device cache.
//! Database location:
//! - macOS: ~/Library/Application Support/FlingDLNA/flingdlna.db
//! - Linux: ~/.local/share/flingdlna/flingdlna.db

use rusqlite::{params, Connection, Result as SqliteResult};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

/// Get the application data directory (App Store compatible)
pub fn data_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Library/Application Support/FlingDLNA")
    }
    #[cfg(target_os = "linux")]
    {
        dirs::data_local_dir()
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".local/share")
            })
            .join("flingdlna")
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("flingdlna")
    }
}

/// Get the Unix socket path
pub fn socket_path() -> PathBuf {
    data_dir().join("flingdlna.sock")
}

/// Get the database file path
pub fn db_path() -> PathBuf {
    data_dir().join("flingdlna.db")
}

/// Database wrapper for thread-safe access
pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    /// Open or create the database
    pub fn open() -> SqliteResult<Self> {
        let path = db_path();

        // Ensure directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        let conn = Connection::open(&path)?;

        let db = Self {
            conn: Mutex::new(conn),
        };

        db.init_schema()?;
        Ok(db)
    }

    /// Initialize database schema
    fn init_schema(&self) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();

        // Playback positions / watch history
        conn.execute(
            "CREATE TABLE IF NOT EXISTS watch_history (
                id INTEGER PRIMARY KEY,
                uri TEXT NOT NULL,
                normalized_key TEXT NOT NULL UNIQUE,
                title TEXT,
                position_secs INTEGER DEFAULT 0,
                duration_secs INTEGER DEFAULT 0,
                device TEXT,
                watched_at INTEGER NOT NULL,
                play_count INTEGER DEFAULT 1,
                completed INTEGER DEFAULT 0
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_watch_history_watched_at
             ON watch_history(watched_at DESC)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_watch_history_key
             ON watch_history(normalized_key)",
            [],
        )?;

        // Device cache
        conn.execute(
            "CREATE TABLE IF NOT EXISTS devices (
                name TEXT PRIMARY KEY,
                location TEXT NOT NULL,
                device_type TEXT DEFAULT 'dlna',
                manufacturer TEXT,
                model TEXT,
                mac_address TEXT,
                last_seen INTEGER NOT NULL
            )",
            [],
        )?;

        Ok(())
    }

    // === Watch History / Positions ===

    /// Save or update playback position
    pub fn save_position(
        &self,
        uri: &str,
        position: Duration,
        duration: Duration,
        device: &str,
    ) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        let key = normalize_uri(uri);
        let now = current_timestamp();
        let title = extract_title(uri);

        // Check if completed (watched > 90%)
        let completed = if duration.as_secs() > 0 {
            (position.as_secs() as f64 / duration.as_secs() as f64) > 0.9
        } else {
            false
        };

        conn.execute(
            "INSERT INTO watch_history (uri, normalized_key, title, position_secs, duration_secs, device, watched_at, play_count, completed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8)
             ON CONFLICT(normalized_key) DO UPDATE SET
                 uri = excluded.uri,
                 title = excluded.title,
                 position_secs = excluded.position_secs,
                 duration_secs = excluded.duration_secs,
                 device = excluded.device,
                 watched_at = excluded.watched_at,
                 play_count = play_count + 1,
                 completed = excluded.completed",
            params![
                uri,
                key,
                title,
                position.as_secs() as i64,
                duration.as_secs() as i64,
                device,
                now as i64,
                completed as i32,
            ],
        )?;

        Ok(())
    }

    /// Get saved position for a URI
    pub fn get_position(&self, uri: &str) -> SqliteResult<Option<SavedPosition>> {
        let conn = self.conn.lock().unwrap();
        let key = normalize_uri(uri);

        let mut stmt = conn.prepare(
            "SELECT uri, position_secs, duration_secs, device, watched_at
             FROM watch_history WHERE normalized_key = ?1",
        )?;

        let result = stmt.query_row([&key], |row| {
            Ok(SavedPosition {
                uri: row.get(0)?,
                position_secs: row.get::<_, i64>(1)? as u64,
                duration_secs: row.get::<_, i64>(2)? as u64,
                device: row.get(3)?,
                saved_at: row.get::<_, i64>(4)? as u64,
            })
        });

        match result {
            Ok(pos) => Ok(Some(pos)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Get recent watch history
    pub fn recent_history(&self, limit: usize) -> SqliteResult<Vec<SavedPosition>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT uri, position_secs, duration_secs, device, watched_at
             FROM watch_history
             ORDER BY watched_at DESC
             LIMIT ?1",
        )?;

        let rows = stmt.query_map([limit as i64], |row| {
            Ok(SavedPosition {
                uri: row.get(0)?,
                position_secs: row.get::<_, i64>(1)? as u64,
                duration_secs: row.get::<_, i64>(2)? as u64,
                device: row.get(3)?,
                saved_at: row.get::<_, i64>(4)? as u64,
            })
        })?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// Get all positions (for compatibility with existing code)
    pub fn all_positions(&self) -> SqliteResult<Vec<SavedPosition>> {
        self.recent_history(500)
    }

    /// Remove a position
    pub fn remove_position(&self, uri: &str) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        let key = normalize_uri(uri);

        conn.execute(
            "DELETE FROM watch_history WHERE normalized_key = ?1",
            [&key],
        )?;

        Ok(())
    }

    /// Cleanup old entries (older than 30 days)
    pub fn cleanup_old(&self) -> SqliteResult<usize> {
        let conn = self.conn.lock().unwrap();
        let cutoff = current_timestamp() - (30 * 24 * 60 * 60);

        let count = conn.execute(
            "DELETE FROM watch_history WHERE watched_at < ?1",
            [cutoff as i64],
        )?;

        Ok(count)
    }

    // === Device Cache ===

    /// Save device to cache
    pub fn cache_device(
        &self,
        name: &str,
        location: &str,
        device_type: &str,
        manufacturer: Option<&str>,
        model: Option<&str>,
        mac_address: Option<&str>,
    ) -> SqliteResult<()> {
        let conn = self.conn.lock().unwrap();
        let now = current_timestamp();

        conn.execute(
            "INSERT INTO devices (name, location, device_type, manufacturer, model, mac_address, last_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(name) DO UPDATE SET
                 location = excluded.location,
                 device_type = excluded.device_type,
                 manufacturer = COALESCE(excluded.manufacturer, manufacturer),
                 model = COALESCE(excluded.model, model),
                 mac_address = COALESCE(excluded.mac_address, mac_address),
                 last_seen = excluded.last_seen",
            params![name, location, device_type, manufacturer, model, mac_address, now as i64],
        )?;

        Ok(())
    }

    /// Get cached device
    pub fn get_cached_device(&self, name: &str) -> SqliteResult<Option<CachedDevice>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT name, location, device_type, manufacturer, model, mac_address, last_seen
             FROM devices WHERE name = ?1",
        )?;

        let result = stmt.query_row([name], |row| {
            Ok(CachedDevice {
                name: row.get(0)?,
                location: row.get(1)?,
                device_type: row.get(2)?,
                manufacturer: row.get(3)?,
                model: row.get(4)?,
                mac_address: row.get(5)?,
                last_seen: row.get::<_, i64>(6)? as u64,
            })
        });

        match result {
            Ok(device) => Ok(Some(device)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Get all cached devices
    pub fn all_cached_devices(&self) -> SqliteResult<Vec<CachedDevice>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT name, location, device_type, manufacturer, model, mac_address, last_seen
             FROM devices ORDER BY last_seen DESC",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(CachedDevice {
                name: row.get(0)?,
                location: row.get(1)?,
                device_type: row.get(2)?,
                manufacturer: row.get(3)?,
                model: row.get(4)?,
                mac_address: row.get(5)?,
                last_seen: row.get::<_, i64>(6)? as u64,
            })
        })?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }
}

/// Saved playback position
#[derive(Debug, Clone)]
pub struct SavedPosition {
    pub uri: String,
    pub position_secs: u64,
    pub duration_secs: u64,
    pub device: String,
    pub saved_at: u64,
}

/// Cached device info
#[derive(Debug, Clone)]
pub struct CachedDevice {
    pub name: String,
    pub location: String,
    pub device_type: String,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub mac_address: Option<String>,
    pub last_seen: u64,
}

/// Get current Unix timestamp
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Normalize URI to create a consistent key
fn normalize_uri(uri: &str) -> String {
    // For file paths, extract just the filename
    if uri.starts_with("file://") || uri.starts_with('/') {
        let path = uri.strip_prefix("file://").unwrap_or(uri);
        return path.rsplit('/').next().unwrap_or(path).to_string();
    }

    // For URLs, extract the path component (filename)
    if let Some(last_segment) = uri.rsplit('/').next() {
        // Remove query parameters
        last_segment
            .split('?')
            .next()
            .unwrap_or(last_segment)
            .to_string()
    } else {
        uri.to_string()
    }
}

/// Extract title from URI
fn extract_title(uri: &str) -> String {
    let filename = normalize_uri(uri);
    // Remove extension
    filename
        .rsplit('.')
        .skip(1)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_uri() {
        assert_eq!(normalize_uri("/path/to/video.mp4"), "video.mp4");
        assert_eq!(normalize_uri("file:///path/to/video.mp4"), "video.mp4");
        assert_eq!(
            normalize_uri("http://example.com/media/video.mp4"),
            "video.mp4"
        );
    }

    #[test]
    fn test_extract_title() {
        assert_eq!(extract_title("/path/to/My Movie.mp4"), "My Movie");
        assert_eq!(extract_title("video.mkv"), "video");
    }

    #[test]
    fn test_data_dir() {
        let dir = data_dir();
        #[cfg(target_os = "macos")]
        assert!(dir
            .to_string_lossy()
            .contains("Application Support/FlingDLNA"));
        #[cfg(target_os = "linux")]
        assert!(dir.to_string_lossy().contains("flingdlna"));
    }
}
