//! Metadata cache using SQLite
//!
//! Caches file metadata to avoid re-scanning unchanged files.
//! Cache entries are invalidated when file size or mtime changes.

use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{debug, info, trace, warn};

/// Schema version - increment when changing table structure
const SCHEMA_VERSION: i32 = 1;

/// SQL schema for metadata cache
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER PRIMARY KEY
);

CREATE TABLE IF NOT EXISTS metadata_cache (
    file_path TEXT PRIMARY KEY,
    file_size INTEGER NOT NULL,
    mtime INTEGER NOT NULL,
    duration_ms INTEGER,
    width INTEGER,
    height INTEGER,
    video_codec TEXT,
    audio_codec TEXT,
    title TEXT,
    artist TEXT,
    album TEXT,
    year INTEGER,
    cached_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_mtime ON metadata_cache(mtime);
"#;

/// Cached metadata for a media file
#[derive(Debug, Clone)]
pub struct CachedMetadata {
    pub duration: Option<Duration>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub year: Option<u32>,
}

/// SQLite-based metadata cache
pub struct MetadataCache {
    conn: Mutex<Connection>,
}

impl MetadataCache {
    /// Open or create the metadata cache database
    pub fn open(path: &Path) -> Result<Self, rusqlite::Error> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        let conn = Connection::open(path)?;

        // Enable WAL mode for better concurrent performance
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;

        // Check schema version - handle missing table gracefully
        let current_version: Option<i32> = conn
            .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| {
                row.get::<_, i32>(0)
            })
            .ok(); // Convert error (table doesn't exist) to None

        let needs_init = current_version.map(|v| v != SCHEMA_VERSION).unwrap_or(true);

        if needs_init {
            // Drop old tables and recreate
            conn.execute_batch(
                "DROP TABLE IF EXISTS metadata_cache; DROP TABLE IF EXISTS schema_version;",
            )?;
            conn.execute_batch(SCHEMA)?;
            conn.execute(
                "INSERT INTO schema_version (version) VALUES (?)",
                [SCHEMA_VERSION],
            )?;
            info!("Initialized metadata cache schema v{}", SCHEMA_VERSION);
        }

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Get cached metadata if file hasn't changed
    pub fn get(&self, path: &Path, size: u64, mtime: i64) -> Option<CachedMetadata> {
        let conn = self.conn.lock().ok()?;
        let path_str = path.to_string_lossy();

        conn.query_row(
            "SELECT duration_ms, width, height, video_codec, audio_codec, title, artist, album, year
             FROM metadata_cache
             WHERE file_path = ? AND file_size = ? AND mtime = ?",
            params![path_str.as_ref(), size as i64, mtime],
            |row| {
                Ok(CachedMetadata {
                    duration: row.get::<_, Option<i64>>(0)?.map(|ms| Duration::from_millis(ms as u64)),
                    width: row.get::<_, Option<i64>>(1)?.map(|w| w as u32),
                    height: row.get::<_, Option<i64>>(2)?.map(|h| h as u32),
                    video_codec: row.get(3)?,
                    audio_codec: row.get(4)?,
                    title: row.get(5)?,
                    artist: row.get(6)?,
                    album: row.get(7)?,
                    year: row.get::<_, Option<i64>>(8)?.map(|y| y as u32),
                })
            },
        )
        .optional()
        .ok()
        .flatten()
    }

    /// Store metadata in cache
    pub fn set(
        &self,
        path: &Path,
        size: u64,
        mtime: i64,
        metadata: &CachedMetadata,
    ) -> Result<(), rusqlite::Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        let path_str = path.to_string_lossy();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        conn.execute(
            "INSERT OR REPLACE INTO metadata_cache
             (file_path, file_size, mtime, duration_ms, width, height, video_codec, audio_codec, title, artist, album, year, cached_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                path_str.as_ref(),
                size as i64,
                mtime,
                metadata.duration.map(|d| d.as_millis() as i64),
                metadata.width.map(|w| w as i64),
                metadata.height.map(|h| h as i64),
                metadata.video_codec.as_deref(),
                metadata.audio_codec.as_deref(),
                metadata.title.as_deref(),
                metadata.artist.as_deref(),
                metadata.album.as_deref(),
                metadata.year.map(|y| y as i64),
                now,
            ],
        )?;
        trace!("Cached metadata for {:?}", path);
        Ok(())
    }

    /// Remove stale entries for files that no longer exist
    pub fn prune_stale(&self, existing_paths: &[PathBuf]) -> Result<usize, rusqlite::Error> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| rusqlite::Error::InvalidQuery)?;

        // Get all cached paths
        let mut stmt = conn.prepare("SELECT file_path FROM metadata_cache")?;
        let cached_paths: Vec<String> = stmt
            .query_map([], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        let existing_set: std::collections::HashSet<_> = existing_paths
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();

        let mut removed = 0;
        for cached_path in cached_paths {
            if !existing_set.contains(&cached_path) {
                conn.execute(
                    "DELETE FROM metadata_cache WHERE file_path = ?",
                    [&cached_path],
                )?;
                removed += 1;
            }
        }

        if removed > 0 {
            debug!("Pruned {} stale cache entries", removed);
        }

        Ok(removed)
    }

    /// Get cache statistics
    pub fn stats(&self) -> Option<CacheStats> {
        let conn = self.conn.lock().ok()?;
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM metadata_cache", [], |row| row.get(0))
            .ok()?;
        Some(CacheStats {
            entries: count as usize,
        })
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub entries: usize,
}

/// Get the default cache path
/// macOS: ~/Library/Caches/com.flingdlna/metadata.db
/// Linux: ~/.cache/flingdlna/metadata.db
pub fn default_cache_path() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("com.flingdlna")
        .join("metadata.db")
}

/// Global cache instance - Option to handle initialization failures
static GLOBAL_CACHE: OnceLock<Option<MetadataCache>> = OnceLock::new();

/// Get or initialize the global metadata cache
/// Returns None if cache initialization fails - scanning will proceed without caching
pub fn global_cache() -> Option<&'static MetadataCache> {
    GLOBAL_CACHE
        .get_or_init(|| {
            let path = default_cache_path();
            match MetadataCache::open(&path) {
                Ok(cache) => {
                    info!("Opened metadata cache at {:?}", path);
                    Some(cache)
                }
                Err(e) => {
                    warn!(
                        "Failed to open metadata cache at {:?}: {} - scanning will proceed without cache",
                        path, e
                    );
                    None
                }
            }
        })
        .as_ref()
}

/// Get file mtime as Unix timestamp
pub fn file_mtime(path: &Path) -> Option<i64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

/// Get file size
pub fn file_size(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|m| m.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::tempdir;

    #[test]
    fn test_cache_roundtrip() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let cache = MetadataCache::open(&db_path).unwrap();

        let test_path = Path::new("/test/video.mp4");
        let metadata = CachedMetadata {
            duration: Some(Duration::from_secs(120)),
            width: Some(1920),
            height: Some(1080),
            video_codec: Some("H.264".to_string()),
            audio_codec: Some("AAC".to_string()),
            title: Some("Test Video".to_string()),
            artist: None,
            album: None,
            year: Some(2024),
        };

        // Store
        cache.set(test_path, 1000, 12345, &metadata).unwrap();

        // Retrieve with matching size/mtime
        let retrieved = cache.get(test_path, 1000, 12345).unwrap();
        assert_eq!(retrieved.duration, metadata.duration);
        assert_eq!(retrieved.width, metadata.width);
        assert_eq!(retrieved.video_codec, metadata.video_codec);

        // Retrieve with different mtime should return None
        assert!(cache.get(test_path, 1000, 99999).is_none());
    }
}
