//! File system scanner for media files

use crate::cache::{file_mtime, file_size, global_cache, CachedMetadata};
use crate::metadata::extract_metadata;
use dlna_core::{Error, MediaFile, Result};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Once;
use tracing::{debug, info, trace, warn};

/// Initialize rayon thread pool with optimal settings
/// Uses half of CPU cores (min 2, max 8) to avoid overloading the system
static INIT_RAYON: Once = Once::new();

fn init_rayon_pool() {
    INIT_RAYON.call_once(|| {
        let num_cpus = num_cpus::get();
        // Use half of CPUs, minimum 2, maximum 8
        let num_threads = (num_cpus / 2).clamp(2, 8);

        if let Err(e) = rayon::ThreadPoolBuilder::new()
            .num_threads(num_threads)
            .build_global()
        {
            warn!("Failed to configure rayon thread pool: {}", e);
        } else {
            info!(
                "Rayon thread pool configured: {} threads (CPUs: {})",
                num_threads, num_cpus
            );
        }
    });
}

/// Supported video extensions
const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "3gp", "mpg", "mpeg", "ts", "m2ts",
];

/// Supported audio extensions
const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "wav", "aac", "ogg", "wma", "m4a", "opus", "aiff",
];

/// Supported image extensions
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "gif", "bmp", "tiff", "webp", "svg"];

/// Check if a file extension is a supported media type
fn is_supported_media(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let e_lower = e.to_lowercase();
            VIDEO_EXTENSIONS.contains(&e_lower.as_str())
                || AUDIO_EXTENSIONS.contains(&e_lower.as_str())
                || IMAGE_EXTENSIONS.contains(&e_lower.as_str())
        })
        .unwrap_or(false)
}

/// Scan a directory for media files (parallel implementation with caching)
pub fn scan_directory(path: &Path) -> Result<Vec<MediaFile>> {
    if !path.exists() {
        return Err(Error::FileNotFound(path.to_path_buf()));
    }

    if !path.is_dir() {
        return Err(Error::Config(format!("{path:?} is not a directory")));
    }

    // Phase 1: Quick pass to collect all file paths (no metadata extraction)
    let mut paths = Vec::new();
    collect_files_recursive(path, &mut paths)?;

    let total = paths.len();
    debug!(
        "Found {} media files, starting parallel metadata extraction",
        total
    );

    // Get cache reference (may be None if cache initialization failed)
    let cache = global_cache();
    let cache_hits = AtomicUsize::new(0);
    let cache_misses = AtomicUsize::new(0);

    // Phase 2: Parallel metadata extraction using rayon with caching
    let files: Vec<MediaFile> = paths
        .into_par_iter()
        .filter_map(|file_path| {
            let parent_id = file_path
                .parent()
                .map(|p| format!("dir_{:x}", hash_path(p)))
                .unwrap_or_else(|| "0".to_string());

            let mut media_file = MediaFile::from_path(file_path.clone(), &parent_id)?;

            // Try cache first
            let size = file_size(&file_path)?;
            let mtime = file_mtime(&file_path)?;

            if let Some(cache) = cache {
                if let Some(cached) = cache.get(&file_path, size, mtime) {
                    // Cache hit - apply cached metadata
                    apply_cached_metadata(&mut media_file, &cached);
                    cache_hits.fetch_add(1, Ordering::Relaxed);
                    return Some(media_file);
                }
            }

            // Cache miss - extract metadata
            cache_misses.fetch_add(1, Ordering::Relaxed);
            extract_metadata(&mut media_file);

            // Store in cache for next time
            if let Some(cache) = cache {
                let cached = extract_to_cached(&media_file);
                let _ = cache.set(&file_path, size, mtime, &cached);
            }

            Some(media_file)
        })
        .collect();

    let hits = cache_hits.load(Ordering::Relaxed);
    let misses = cache_misses.load(Ordering::Relaxed);
    info!(
        "Parallel scan complete: {} files ({} cache hits, {} misses)",
        files.len(),
        hits,
        misses
    );
    Ok(files)
}

/// Apply cached metadata to a MediaFile
fn apply_cached_metadata(file: &mut MediaFile, cached: &CachedMetadata) {
    file.duration = cached.duration;
    file.resolution = cached.width.zip(cached.height);
    file.video_codec = cached.video_codec.clone();
    file.audio_codec = cached.audio_codec.clone();
    if cached.title.is_some() {
        file.title = cached.title.clone();
    }
    file.artist = cached.artist.clone();
    file.album = cached.album.clone();
    file.year = cached.year;
}

/// Extract metadata from MediaFile into CachedMetadata
fn extract_to_cached(file: &MediaFile) -> CachedMetadata {
    CachedMetadata {
        duration: file.duration,
        width: file.resolution.map(|(w, _)| w),
        height: file.resolution.map(|(_, h)| h),
        video_codec: file.video_codec.clone(),
        audio_codec: file.audio_codec.clone(),
        title: file.title.clone(),
        artist: file.artist.clone(),
        album: file.album.clone(),
        year: file.year,
    }
}

/// Quickly collect all media file paths without extracting metadata
fn collect_files_recursive(dir: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            warn!("Skipping directory (permission denied): {:?}", dir);
            return Ok(());
        }
        Err(err) => return Err(err.into()),
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
                warn!("Skipping entry (permission denied) in {:?}: {}", dir, err);
                continue;
            }
            Err(err) => return Err(err.into()),
        };
        let path = entry.path();

        if path.is_dir() {
            // Recursively scan subdirectories
            if let Err(err) = collect_files_recursive(&path, paths) {
                match err {
                    Error::Io(ref io_err)
                        if io_err.kind() == std::io::ErrorKind::PermissionDenied =>
                    {
                        warn!("Skipping directory (permission denied): {:?}", path);
                        continue;
                    }
                    _ => return Err(err),
                }
            }
        } else if path.is_file() && is_supported_media(&path) {
            trace!("Found media file: {:?}", path);
            paths.push(path);
        }
    }

    Ok(())
}

/// Scan a single file and return MediaFile if it's a supported media type
pub fn scan_file(path: &Path) -> Option<MediaFile> {
    if !path.is_file() || !is_supported_media(path) {
        return None;
    }

    // Determine parent ID from directory
    let parent_id = path
        .parent()
        .map(|p| format!("dir_{:x}", hash_path(p)))
        .unwrap_or_else(|| "0".to_string());

    let mut file = MediaFile::from_path(path.to_path_buf(), &parent_id)?;

    // Try cache first
    let cache = global_cache();
    let size = file_size(path)?;
    let mtime = file_mtime(path)?;

    if let Some(cache) = cache {
        if let Some(cached) = cache.get(path, size, mtime) {
            apply_cached_metadata(&mut file, &cached);
            return Some(file);
        }
    }

    // Cache miss - extract metadata
    extract_metadata(&mut file);

    // Store in cache
    if let Some(cache) = cache {
        let cached = extract_to_cached(&file);
        let _ = cache.set(path, size, mtime, &cached);
    }

    Some(file)
}

/// Simple path hashing for generating IDs
fn hash_path(path: &Path) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

// =============================================================================
// Progress-aware scanning
// =============================================================================

/// Progress information during scanning
#[derive(Debug, Clone)]
pub struct ScanProgress {
    /// Number of files scanned so far
    pub files_scanned: usize,
    /// Total number of files to scan
    pub files_total: usize,
    /// Current file path being scanned (if available)
    pub current_path: Option<PathBuf>,
}

/// Scan a directory with progress reporting
///
/// The progress callback is called periodically (not for every file) to avoid overhead.
/// The callback receives progress information and should return quickly.
pub fn scan_directory_with_progress<F>(path: &Path, progress_callback: F) -> Result<Vec<MediaFile>>
where
    F: Fn(&ScanProgress) + Send + Sync,
{
    // Initialize rayon with optimal thread count
    init_rayon_pool();

    if !path.exists() {
        return Err(Error::FileNotFound(path.to_path_buf()));
    }

    if !path.is_dir() {
        return Err(Error::Config(format!("{path:?} is not a directory")));
    }

    // Phase 1: Quick pass to collect all file paths
    let mut paths = Vec::new();
    collect_files_recursive(path, &mut paths)?;

    let total = paths.len();
    debug!(
        "Found {} media files, starting parallel metadata extraction with progress",
        total
    );

    // Report initial progress
    progress_callback(&ScanProgress {
        files_scanned: 0,
        files_total: total,
        current_path: None,
    });

    let cache = global_cache();
    let scanned = AtomicUsize::new(0);

    // Phase 2: Parallel metadata extraction with progress tracking
    let files: Vec<MediaFile> = paths
        .into_par_iter()
        .filter_map(|file_path| {
            // Report progress at START of processing (shows real parallel progress)
            let current = scanned.fetch_add(1, Ordering::Relaxed) + 1;
            progress_callback(&ScanProgress {
                files_scanned: current,
                files_total: total,
                current_path: Some(file_path.clone()),
            });

            let parent_id = file_path
                .parent()
                .map(|p| format!("dir_{:x}", hash_path(p)))
                .unwrap_or_else(|| "0".to_string());

            let mut media_file = MediaFile::from_path(file_path.clone(), &parent_id)?;

            // Try cache first
            let size = file_size(&file_path)?;
            let mtime = file_mtime(&file_path)?;

            if let Some(cache) = cache {
                if let Some(cached) = cache.get(&file_path, size, mtime) {
                    apply_cached_metadata(&mut media_file, &cached);

                    return Some(media_file);
                }
            }

            // Cache miss - extract metadata
            extract_metadata(&mut media_file);

            // Store in cache
            if let Some(cache) = cache {
                let cached = extract_to_cached(&media_file);
                let _ = cache.set(&file_path, size, mtime, &cached);
            }

            Some(media_file)
        })
        .collect();

    // Report final progress
    progress_callback(&ScanProgress {
        files_scanned: files.len(),
        files_total: total,
        current_path: None,
    });

    info!(
        "Parallel scan with progress complete: {} files",
        files.len()
    );
    Ok(files)
}

/// Count media files in a directory (quick pass, no metadata)
pub fn count_media_files(path: &Path) -> Result<usize> {
    if !path.exists() {
        return Err(Error::FileNotFound(path.to_path_buf()));
    }

    if !path.is_dir() {
        return Err(Error::Config(format!("{path:?} is not a directory")));
    }

    let mut paths = Vec::new();
    collect_files_recursive(path, &mut paths)?;
    Ok(paths.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_supported_media() {
        assert!(is_supported_media(Path::new("video.mp4")));
        assert!(is_supported_media(Path::new("audio.mp3")));
        assert!(is_supported_media(Path::new("image.jpg")));
        assert!(!is_supported_media(Path::new("document.pdf")));
        assert!(!is_supported_media(Path::new("no_extension")));
    }
}
