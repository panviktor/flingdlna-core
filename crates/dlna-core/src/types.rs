//! Core types for DLNA library

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;
use url::Url;

/// Represents a discovered DLNA Media Renderer
#[derive(Debug, Clone)]
pub struct Renderer {
    /// Human-readable name of the device
    pub friendly_name: String,
    /// URL to the device description XML
    pub location: Url,
    /// Unique Device Name (UUID)
    pub udn: String,
    /// URL for AVTransport service control
    pub av_transport_url: Option<Url>,
    /// Service type for AVTransport (e.g. urn:schemas-upnp-org:service:AVTransport:1)
    pub av_transport_service_type: Option<String>,
    /// URL for RenderingControl service (volume, mute)
    pub rendering_control_url: Option<Url>,
    /// Service type for RenderingControl (e.g. urn:schemas-upnp-org:service:RenderingControl:1)
    pub rendering_control_service_type: Option<String>,
    /// URL for AVTransport event subscription
    pub av_transport_event_url: Option<Url>,
    /// URL for RenderingControl event subscription
    pub rendering_control_event_url: Option<Url>,
    /// Device manufacturer
    pub manufacturer: Option<String>,
    /// Device model name
    pub model_name: Option<String>,
    /// Device model number
    pub model_number: Option<String>,
    /// Device model description
    pub model_description: Option<String>,
    /// Device serial number
    pub serial_number: Option<String>,
    /// Presentation URL (web UI for device configuration)
    pub presentation_url: Option<Url>,
    /// MAC address for Wake-on-LAN (format: AA:BB:CC:DD:EE:FF)
    pub mac_address: Option<String>,
    /// Device icons in various sizes
    pub icons: Vec<DeviceIcon>,
    /// Firmware version (from Chromecast mDNS 've' field or DLNA device description)
    pub firmware_version: Option<String>,
    /// Capabilities string (from Chromecast mDNS 'ca' field)
    pub capabilities: Option<String>,
}

/// Represents a device icon from UPnP device description
#[derive(Debug, Clone)]
pub struct DeviceIcon {
    /// MIME type (e.g. "image/png", "image/jpeg")
    pub mime_type: String,
    /// Icon width in pixels
    pub width: u32,
    /// Icon height in pixels
    pub height: u32,
    /// Color depth in bits
    pub depth: u32,
    /// URL to the icon image
    pub url: Url,
}

impl fmt::Display for Renderer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({})",
            self.friendly_name,
            self.location.host_str().unwrap_or("unknown")
        )
    }
}

impl Renderer {
    /// Get the host address of this renderer
    pub fn host(&self) -> Option<&str> {
        self.location.host_str()
    }

    /// Check if this renderer matches a search query (case-insensitive)
    pub fn matches(&self, query: &str) -> bool {
        let query_lower = query.to_lowercase();
        self.friendly_name.to_lowercase().contains(&query_lower)
            || self
                .location
                .host_str()
                .map(|h| h.to_lowercase().contains(&query_lower))
                .unwrap_or(false)
            || self
                .location
                .to_string()
                .to_lowercase()
                .contains(&query_lower)
    }
}

/// Information about this server for SSDP announcements
#[derive(Debug, Clone)]
pub struct ServerInfo {
    /// Human-readable name of the server
    pub friendly_name: String,
    /// Unique Device Name (UUID format: uuid:xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx)
    pub udn: String,
    /// Manufacturer name
    pub manufacturer: String,
    /// Model name
    pub model_name: String,
}

impl ServerInfo {
    /// Create new server info with a generated UUID
    pub fn new(friendly_name: impl Into<String>) -> Self {
        Self {
            friendly_name: friendly_name.into(),
            udn: format!("uuid:{}", uuid::Uuid::new_v4()),
            manufacturer: "flingdlna".to_string(),
            model_name: "flingdlna DMS 1.0".to_string(),
        }
    }
}

/// Type of media file
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MediaType {
    Video,
    Audio,
    Image,
}

impl MediaType {
    /// Get the UPnP class for this media type
    pub fn upnp_class(&self) -> &'static str {
        match self {
            MediaType::Video => "object.item.videoItem",
            MediaType::Audio => "object.item.audioItem.musicTrack",
            MediaType::Image => "object.item.imageItem.photo",
        }
    }

    /// Determine media type from MIME type string
    pub fn from_mime(mime: &str) -> Option<Self> {
        if mime.starts_with("video/") {
            Some(MediaType::Video)
        } else if mime.starts_with("audio/") {
            Some(MediaType::Audio)
        } else if mime.starts_with("image/") {
            Some(MediaType::Image)
        } else {
            None
        }
    }
}

impl fmt::Display for MediaType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MediaType::Video => write!(f, "Video"),
            MediaType::Audio => write!(f, "Audio"),
            MediaType::Image => write!(f, "Image"),
        }
    }
}

/// Represents a media file in the server's library
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaFile {
    /// Unique identifier for this file
    pub id: String,
    /// Full path to the file
    pub path: PathBuf,
    /// File name without path
    pub filename: String,
    /// MIME type (e.g., "video/mp4")
    pub mime_type: String,
    /// Media type category
    pub media_type: MediaType,
    /// File size in bytes
    pub size: u64,
    /// Media duration (for video/audio)
    pub duration: Option<Duration>,
    /// Title metadata
    pub title: Option<String>,
    /// Artist metadata (for audio)
    pub artist: Option<String>,
    /// Album metadata (for audio)
    pub album: Option<String>,
    /// Parent directory ID
    pub parent_id: String,

    // Video-specific metadata
    /// Release year
    pub year: Option<u32>,
    /// Video resolution (width, height)
    pub resolution: Option<(u32, u32)>,
    /// Video codec name (e.g., "H.264", "HEVC")
    pub video_codec: Option<String>,
    /// Audio codec name (e.g., "AAC", "AC3")
    pub audio_codec: Option<String>,
}

impl MediaFile {
    /// Create a new MediaFile from a path
    pub fn from_path(path: PathBuf, parent_id: impl Into<String>) -> Option<Self> {
        let filename = path.file_name()?.to_string_lossy().to_string();
        let mime_type = mime_guess::from_path(&path)
            .first()
            .map(|m| m.to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string());
        let media_type = MediaType::from_mime(&mime_type)?;
        let size = std::fs::metadata(&path).ok()?.len();

        // Generate ID from path hash
        let id = format!("{:x}", path_hash(path.to_string_lossy().as_bytes()));

        // Try to get duration via ffprobe
        let duration = get_duration_ffprobe(&path);

        Some(Self {
            id,
            path,
            filename,
            mime_type,
            media_type,
            size,
            duration,
            title: None,
            artist: None,
            album: None,
            parent_id: parent_id.into(),
            year: None,
            resolution: None,
            video_codec: None,
            audio_codec: None,
        })
    }

    /// Get the display title (title metadata or filename)
    pub fn display_title(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.filename)
    }
}

/// Fast non-cryptographic hash for generating media file IDs
fn path_hash(data: &[u8]) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish()
}

/// Get media duration using ffprobe (if available)
fn get_duration_ffprobe(path: &std::path::Path) -> Option<Duration> {
    use crate::process::run_command_capture_stdout;
    use std::time::Duration as StdDuration;

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
        StdDuration::from_secs(5),
        32 * 1024,
    )
    .ok()??;

    if !output.success {
        return None;
    }

    let duration_str = String::from_utf8_lossy(&output.stdout);
    let seconds: f64 = duration_str.trim().parse().ok()?;
    Some(Duration::from_secs_f64(seconds))
}

/// Transport state of a renderer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportState {
    Stopped,
    Playing,
    Paused,
    Transitioning,
    NoMediaPresent,
    Unknown,
}

impl TransportState {
    /// Parse from AVTransport response string
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "STOPPED" => TransportState::Stopped,
            "PLAYING" => TransportState::Playing,
            "PAUSED_PLAYBACK" | "PAUSED" => TransportState::Paused,
            "TRANSITIONING" => TransportState::Transitioning,
            "NO_MEDIA_PRESENT" => TransportState::NoMediaPresent,
            _ => TransportState::Unknown,
        }
    }
}

impl std::str::FromStr for TransportState {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s.to_uppercase().as_str() {
            "STOPPED" => TransportState::Stopped,
            "PLAYING" => TransportState::Playing,
            "PAUSED_PLAYBACK" | "PAUSED" => TransportState::Paused,
            "TRANSITIONING" => TransportState::Transitioning,
            "NO_MEDIA_PRESENT" => TransportState::NoMediaPresent,
            _ => TransportState::Unknown,
        })
    }
}

impl fmt::Display for TransportState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportState::Stopped => write!(f, "Stopped"),
            TransportState::Playing => write!(f, "Playing"),
            TransportState::Paused => write!(f, "Paused"),
            TransportState::Transitioning => write!(f, "Transitioning"),
            TransportState::NoMediaPresent => write!(f, "No Media"),
            TransportState::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Playback information from a renderer
#[derive(Debug, Clone)]
pub struct PlaybackInfo {
    /// Current playback position
    pub position: Duration,
    /// Total media duration
    pub duration: Duration,
    /// Current transport state
    pub state: TransportState,
    /// Currently playing URI
    pub current_uri: Option<String>,
}

/// Media information from GetMediaInfo (UPnP AVTransport action)
#[derive(Debug, Clone)]
pub struct MediaInfo {
    /// Number of tracks in the current media
    pub nr_tracks: u32,
    /// Duration of the current media (if provided by device)
    pub media_duration: Option<Duration>,
    /// Current URI being played
    pub current_uri: Option<String>,
    /// Play medium type (NETWORK, HDD, CD-DA, etc.)
    pub play_medium: Option<String>,
    /// Next URI in queue (if applicable)
    pub next_uri: Option<String>,
}

/// Device capabilities from GetDeviceCapabilities (UPnP AVTransport action)
#[derive(Debug, Clone)]
pub struct DeviceCapabilities {
    /// Supported play media types (e.g., ["NETWORK", "HDD", "CD-DA"])
    pub play_media: Vec<String>,
    /// Supported record media types (usually ["NOT_IMPLEMENTED"])
    pub rec_media: Vec<String>,
    /// Supported record quality modes (usually ["NOT_IMPLEMENTED"])
    pub rec_quality_modes: Vec<String>,
}
