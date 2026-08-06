//! Media metadata extraction
//!
//! This module provides utilities for extracting metadata from media files:
//! - Audio files: title, artist, album, duration (using lofty)
//! - Video files: duration, title, year, resolution, codecs (using matroska, mp4)

use dlna_core::run_command_capture_stdout;
use dlna_core::MediaFile;
use std::path::Path;
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;
use tracing::{debug, trace};

/// Simple counting semaphore for limiting concurrent operations
struct CountingSemaphore {
    count: Mutex<usize>,
    max: usize,
    condvar: Condvar,
}

impl CountingSemaphore {
    fn new(max: usize) -> Self {
        Self {
            count: Mutex::new(0),
            max,
            condvar: Condvar::new(),
        }
    }

    fn acquire(&self) -> SemaphoreGuard<'_> {
        let mut count = self.count.lock().unwrap();
        while *count >= self.max {
            count = self.condvar.wait(count).unwrap();
        }
        *count += 1;
        SemaphoreGuard { semaphore: self }
    }
}

struct SemaphoreGuard<'a> {
    semaphore: &'a CountingSemaphore,
}

impl Drop for SemaphoreGuard<'_> {
    fn drop(&mut self) {
        let mut count = self.semaphore.count.lock().unwrap();
        *count -= 1;
        self.semaphore.condvar.notify_one();
    }
}

/// Semaphore to limit concurrent ffprobe processes (prevents system overload)
static FFPROBE_SEMAPHORE: OnceLock<CountingSemaphore> = OnceLock::new();

fn get_ffprobe_semaphore() -> &'static CountingSemaphore {
    FFPROBE_SEMAPHORE.get_or_init(|| CountingSemaphore::new(3))
}

// ============================================================================
// Trait and Types
// ============================================================================

/// Video metadata extraction result
#[derive(Debug, Default, Clone)]
pub struct VideoMetadata {
    pub duration: Option<Duration>,
    pub title: Option<String>,
    pub year: Option<u32>,
    pub resolution: Option<(u32, u32)>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
}

/// Error type for metadata extraction
#[derive(Debug)]
pub enum MetadataError {
    IoError(std::io::Error),
    ParseError(String),
    UnsupportedFormat,
}

impl From<std::io::Error> for MetadataError {
    fn from(e: std::io::Error) -> Self {
        MetadataError::IoError(e)
    }
}

/// Trait for format-specific metadata extractors
pub trait MetadataExtractor: Send + Sync {
    /// File extensions this extractor handles (lowercase, no dot)
    fn supported_extensions(&self) -> &[&str];

    /// Extract metadata from a file
    fn extract(&self, path: &Path) -> Result<VideoMetadata, MetadataError>;
}

// ============================================================================
// Matroska Extractor (MKV, WebM)
// ============================================================================

pub struct MatroskaExtractor;

impl MetadataExtractor for MatroskaExtractor {
    fn supported_extensions(&self) -> &[&str] {
        &["mkv", "webm"]
    }

    fn extract(&self, path: &Path) -> Result<VideoMetadata, MetadataError> {
        let mkv = matroska::open(path).map_err(|e| MetadataError::ParseError(e.to_string()))?;

        let mut meta = VideoMetadata {
            duration: mkv.info.duration,
            title: mkv.info.title.clone(),
            ..Default::default()
        };

        // Get video track info
        for track in &mkv.tracks {
            if let matroska::Tracktype::Video = track.tracktype {
                // Extract resolution from track settings
                if let matroska::Settings::Video(ref video) = track.settings {
                    meta.resolution = Some((video.pixel_width as u32, video.pixel_height as u32));
                }
                // Codec ID like "V_MPEG4/ISO/AVC" -> "H.264"
                meta.video_codec = Some(normalize_video_codec(&track.codec_id));
                break;
            }
        }

        // Get audio track info
        for track in &mkv.tracks {
            if let matroska::Tracktype::Audio = track.tracktype {
                meta.audio_codec = Some(normalize_audio_codec(&track.codec_id));
                break;
            }
        }

        Ok(meta)
    }
}

// ============================================================================
// MP4 Extractor (MP4, M4V, MOV, 3GP)
// ============================================================================

pub struct Mp4Extractor;

impl MetadataExtractor for Mp4Extractor {
    fn supported_extensions(&self) -> &[&str] {
        &["mp4", "m4v", "mov", "3gp", "m4a"]
    }

    fn extract(&self, path: &Path) -> Result<VideoMetadata, MetadataError> {
        use std::fs::File;
        use std::io::BufReader;

        let file = File::open(path)?;
        let size = file.metadata()?.len();
        let reader = BufReader::new(file);

        let mp4 = mp4::Mp4Reader::read_header(reader, size)
            .map_err(|e| MetadataError::ParseError(e.to_string()))?;

        let mut meta = VideoMetadata {
            duration: Some(mp4.duration()),
            ..Default::default()
        };

        // Get video track info
        for track in mp4.tracks().values() {
            if track
                .track_type()
                .map_err(|e| MetadataError::ParseError(e.to_string()))?
                == mp4::TrackType::Video
            {
                meta.resolution = Some((track.width() as u32, track.height() as u32));
                meta.video_codec = Some(format_mp4_video_codec(track));
                break;
            }
        }

        // Get audio track info
        for track in mp4.tracks().values() {
            if track
                .track_type()
                .map_err(|e| MetadataError::ParseError(e.to_string()))?
                == mp4::TrackType::Audio
            {
                meta.audio_codec = Some(format_mp4_audio_codec(track));
                break;
            }
        }

        Ok(meta)
    }
}

fn format_mp4_video_codec(track: &mp4::Mp4Track) -> String {
    // Try to get codec info from sample description
    match track.media_type() {
        Ok(mp4::MediaType::H264) => "H.264".to_string(),
        Ok(mp4::MediaType::H265) => "HEVC".to_string(),
        Ok(mp4::MediaType::VP9) => "VP9".to_string(),
        _ => "Unknown".to_string(),
    }
}

fn format_mp4_audio_codec(track: &mp4::Mp4Track) -> String {
    match track.media_type() {
        Ok(mp4::MediaType::AAC) => "AAC".to_string(),
        Ok(mp4::MediaType::TTXT) => "Text".to_string(),
        _ => "Unknown".to_string(),
    }
}

// ============================================================================
// Codec Normalization Helpers
// ============================================================================

fn normalize_video_codec(codec_id: &str) -> String {
    match codec_id {
        "V_MPEG4/ISO/AVC" => "H.264".to_string(),
        "V_MPEGH/ISO/HEVC" => "HEVC".to_string(),
        "V_VP8" => "VP8".to_string(),
        "V_VP9" => "VP9".to_string(),
        "V_AV1" => "AV1".to_string(),
        "V_MPEG4/ISO/SP" | "V_MPEG4/ISO/ASP" | "V_MPEG4/ISO/AP" => "MPEG-4".to_string(),
        "V_MPEG2" => "MPEG-2".to_string(),
        "V_MPEG1" => "MPEG-1".to_string(),
        "V_THEORA" => "Theora".to_string(),
        _ if codec_id.starts_with("V_MS/VFW/FOURCC") => {
            // Extract FourCC from "V_MS/VFW/FOURCC/XVID" etc.
            codec_id
                .split('/')
                .next_back()
                .unwrap_or("Unknown")
                .to_string()
        }
        _ => codec_id.replace("V_", "").to_string(),
    }
}

fn normalize_audio_codec(codec_id: &str) -> String {
    match codec_id {
        "A_AAC" | "A_AAC/MPEG4/LC" | "A_AAC/MPEG4/LC/SBR" => "AAC".to_string(),
        "A_AC3" => "AC3".to_string(),
        "A_EAC3" => "E-AC3".to_string(),
        "A_DTS" => "DTS".to_string(),
        "A_FLAC" => "FLAC".to_string(),
        "A_VORBIS" => "Vorbis".to_string(),
        "A_OPUS" => "Opus".to_string(),
        "A_TRUEHD" => "TrueHD".to_string(),
        "A_PCM/INT/LIT" | "A_PCM/INT/BIG" => "PCM".to_string(),
        "A_MP3" | "A_MPEG/L3" => "MP3".to_string(),
        _ => codec_id.replace("A_", "").to_string(),
    }
}

// ============================================================================
// Registry
// ============================================================================

/// Registry of all metadata extractors
pub struct MetadataRegistry {
    extractors: Vec<Box<dyn MetadataExtractor>>,
}

impl Default for MetadataRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl MetadataRegistry {
    pub fn new() -> Self {
        Self {
            extractors: vec![Box::new(MatroskaExtractor), Box::new(Mp4Extractor)],
        }
    }

    /// Extract video metadata using the appropriate extractor
    pub fn extract(&self, path: &Path) -> Result<VideoMetadata, MetadataError> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();

        for extractor in &self.extractors {
            if extractor.supported_extensions().contains(&ext.as_str()) {
                return extractor.extract(path);
            }
        }

        Err(MetadataError::UnsupportedFormat)
    }
}

// Global registry instance
static REGISTRY: OnceLock<MetadataRegistry> = OnceLock::new();

fn get_registry() -> &'static MetadataRegistry {
    REGISTRY.get_or_init(MetadataRegistry::new)
}

// ============================================================================
// Public API
// ============================================================================

/// Extract metadata from a media file and update it in place
pub fn extract_metadata(file: &mut MediaFile) {
    if file.mime_type.starts_with("audio/") {
        extract_audio_metadata(file);
    } else if file.mime_type.starts_with("video/") {
        extract_video_metadata(file);
    }
}

/// Extract metadata from an audio file using lofty
fn extract_audio_metadata(file: &mut MediaFile) {
    use lofty::file::{AudioFile, TaggedFileExt};
    use lofty::probe::Probe;
    use lofty::tag::Accessor;

    let path = &file.path;

    match Probe::open(path).and_then(|p| p.read()) {
        Ok(tagged_file) => {
            // Get duration from audio properties
            let duration = tagged_file.properties().duration();
            if !duration.is_zero() {
                file.duration = Some(duration);
                debug!("Audio duration for {:?}: {:?}", path, file.duration);
            }

            // Try to get tags
            if let Some(tag) = tagged_file
                .primary_tag()
                .or_else(|| tagged_file.first_tag())
            {
                if let Some(title) = tag.title() {
                    file.title = Some(title.to_string());
                    trace!("Audio title: {}", title);
                }
                if let Some(artist) = tag.artist() {
                    file.artist = Some(artist.to_string());
                    trace!("Audio artist: {}", artist);
                }
                if let Some(album) = tag.album() {
                    file.album = Some(album.to_string());
                    trace!("Audio album: {}", album);
                }
                if let Some(year) = tag.year() {
                    file.year = Some(year);
                    trace!("Audio year: {}", year);
                }
            }
        }
        Err(e) => {
            debug!("Failed to read audio metadata from {:?}: {}", path, e);
        }
    }
}

/// Extract metadata from a video file
fn extract_video_metadata(file: &mut MediaFile) {
    // Try registry-based extraction first
    match get_registry().extract(&file.path) {
        Ok(meta) => {
            if let Some(d) = meta.duration {
                file.duration = Some(d);
            }
            if meta.title.is_some() {
                file.title = meta.title;
            }
            file.year = meta.year;
            file.resolution = meta.resolution;
            file.video_codec = meta.video_codec;
            file.audio_codec = meta.audio_codec;

            debug!(
                "Video metadata for {:?}: duration={:?}, resolution={:?}, video={:?}, audio={:?}",
                file.path, file.duration, file.resolution, file.video_codec, file.audio_codec
            );
        }
        Err(MetadataError::UnsupportedFormat) => {
            // Fallback to ffprobe for unsupported formats
            if let Some(duration) = get_duration_ffprobe(&file.path) {
                file.duration = Some(duration);
                debug!(
                    "Video duration (ffprobe) for {:?}: {:?}",
                    file.path, duration
                );
            }
        }
        Err(e) => {
            debug!(
                "Failed to extract video metadata from {:?}: {:?}",
                file.path, e
            );
            // Fallback to ffprobe
            if let Some(duration) = get_duration_ffprobe(&file.path) {
                file.duration = Some(duration);
            }
        }
    }
}

/// Get video duration using ffprobe (fallback)
/// Uses semaphore to limit concurrent ffprobe processes (max 3)
fn get_duration_ffprobe(path: &Path) -> Option<Duration> {
    // Acquire semaphore permit - blocks if too many ffprobe processes are running
    let _permit = get_ffprobe_semaphore().acquire();

    let path_str = path.to_string_lossy();
    let output = run_command_capture_stdout(
        "ffprobe",
        &[
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            path_str.as_ref(),
        ],
        Duration::from_secs(5),
        32 * 1024,
    )
    .ok()??;

    if !output.success {
        trace!("ffprobe failed for {:?}", path);
        return None;
    }

    let duration_str = String::from_utf8_lossy(&output.stdout);
    let seconds: f64 = duration_str.trim().parse().ok()?;
    Some(Duration::from_secs_f64(seconds))
}

/// Check if ffprobe is available
pub fn ffprobe_available() -> bool {
    run_command_capture_stdout("ffprobe", &["-version"], Duration::from_secs(2), 4 * 1024)
        .ok()
        .flatten()
        .map(|o| o.success)
        .unwrap_or(false)
}

/// Extract metadata from multiple files in parallel
pub async fn extract_metadata_batch(files: &mut [MediaFile]) {
    if files.len() < 10 {
        for file in files.iter_mut() {
            extract_metadata(file);
        }
        return;
    }

    for file in files.iter_mut() {
        let path = file.path.clone();
        let mime_type = file.mime_type.clone();

        let result = tokio::task::spawn_blocking(move || {
            let mut temp_file = MediaFile {
                id: String::new(),
                path,
                filename: String::new(),
                mime_type,
                media_type: dlna_core::MediaType::Video,
                size: 0,
                duration: None,
                title: None,
                artist: None,
                album: None,
                parent_id: String::new(),
                year: None,
                resolution: None,
                video_codec: None,
                audio_codec: None,
            };
            extract_metadata(&mut temp_file);
            (
                temp_file.duration,
                temp_file.title,
                temp_file.artist,
                temp_file.album,
                temp_file.year,
                temp_file.resolution,
                temp_file.video_codec,
                temp_file.audio_codec,
            )
        })
        .await;

        if let Ok((duration, title, artist, album, year, resolution, video_codec, audio_codec)) =
            result
        {
            file.duration = duration;
            if title.is_some() {
                file.title = title;
            }
            if artist.is_some() {
                file.artist = artist;
            }
            if album.is_some() {
                file.album = album;
            }
            file.year = year;
            file.resolution = resolution;
            file.video_codec = video_codec;
            file.audio_codec = audio_codec;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ffprobe_check() {
        let available = ffprobe_available();
        println!("ffprobe available: {available}");
    }

    #[test]
    fn test_codec_normalization() {
        assert_eq!(normalize_video_codec("V_MPEG4/ISO/AVC"), "H.264");
        assert_eq!(normalize_video_codec("V_MPEGH/ISO/HEVC"), "HEVC");
        assert_eq!(normalize_audio_codec("A_AAC"), "AAC");
        assert_eq!(normalize_audio_codec("A_AC3"), "AC3");
    }
}
