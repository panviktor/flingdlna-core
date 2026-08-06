//! Daemon protocol - command and response definitions
//!
//! Simple text-based protocol for Unix socket communication:
//! - Commands: single-line JSON objects
//! - Responses: single-line JSON objects
//!
//! Easy to test with: echo '{"cmd":"list"}' | nc -U ~/Library/Application\ Support/FlingDLNA/flingdlna.sock

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayRequestPayload {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Commands sent from CLI to daemon
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    /// Discover renderers on the network
    List {
        #[serde(default = "default_timeout")]
        timeout_secs: u64,
    },

    /// Get cached renderers (instant, no network discovery)
    ListCached,

    /// Force refresh renderers (network discovery)
    Refresh {
        #[serde(default = "default_timeout")]
        timeout_secs: u64,
    },

    /// List media files from server library
    ListMedia,

    /// Push a file to a renderer
    Push {
        file: PathBuf,
        device: String,
        #[serde(default = "default_port")]
        port: u16,
    },

    /// Play a URL on a renderer
    PlayUrl { url: String, device: String },

    /// Play a request on a renderer (url + optional metadata)
    PlayRequest {
        request: PlayRequestPayload,
        device: String,
    },

    /// Pause playback
    Pause { device: String },

    /// Resume playback (same as Play after pause)
    Resume { device: String },

    /// Stop playback
    Stop { device: String },

    /// Wake device using Wake-on-LAN
    Wake {
        device: String,
        /// Optional MAC address (overrides cached)
        #[serde(default)]
        mac: Option<String>,
    },

    /// Seek to position (seconds)
    Seek { device: String, position_secs: u64 },

    /// Get playback status
    Status { device: String },

    /// Get volume level
    GetVolume { device: String },

    /// Set volume level (0-100)
    SetVolume { device: String, volume: u8 },

    /// Adjust volume by delta (reads current from device first)
    AdjustVolume {
        device: String,
        /// Positive to increase, negative to decrease
        delta: i8,
    },

    /// Toggle or set mute
    Mute {
        device: String,
        #[serde(default)]
        mute: Option<bool>,
    },

    /// Get daemon info
    Info,

    /// Ping (health check)
    Ping,

    /// Shutdown the daemon
    Shutdown,

    // === Queue Commands ===
    /// Add item to queue
    QueueAdd {
        /// File path or URL
        source: String,
        /// Optional display title (primarily for URL sources)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Optional content type for URL sources
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content_type: Option<String>,
        /// Optional subtitle URL for URL sources
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subtitle_url: Option<String>,
        /// Device to play on
        device: String,
        /// Position in queue (None = end)
        #[serde(default)]
        position: Option<usize>,
    },

    /// Remove item from queue
    QueueRemove { device: String, index: usize },

    /// Clear queue
    QueueClear { device: String },

    /// List queue items
    QueueList { device: String },

    /// Play next in queue
    Next { device: String },

    /// Play previous in queue
    Prev { device: String },

    /// Set shuffle mode
    Shuffle { device: String, enabled: bool },

    /// Set repeat mode (none, one, all)
    Repeat { device: String, mode: String },

    // === Resume/Position Commands ===
    /// Save current playback position
    SavePosition { device: String },

    /// Get saved position for a URI
    GetPosition { uri: String },

    /// List all saved positions
    ListPositions,

    /// Clear saved position for a URI
    ClearPosition { uri: String },

    /// Resume playback from saved position
    ResumeFrom { device: String, uri: String },

    // === Event Subscription Commands ===
    /// Subscribe to UPnP events for a device
    Subscribe { device: String },

    /// Unsubscribe from UPnP events for a device
    Unsubscribe { device: String },

    /// Poll for pending UPnP events
    PollEvents {
        /// Optional device filter (None = all events)
        #[serde(default)]
        device: Option<String>,
    },

    // === Media Server Directory Commands ===
    /// Set media directory (clears existing files and scans new directory)
    SetMediaDir {
        /// Path to media directory
        path: PathBuf,
    },

    /// Add media directory (keeps existing files, adds new)
    AddMediaDir {
        /// Path to media directory
        path: PathBuf,
    },

    /// Remove media directory
    RemoveMediaDir {
        /// Path to media directory
        path: PathBuf,
    },

    /// Clear all media files
    ClearMedia,

    /// Rescan all media directories
    RescanMedia,

    /// Set auto-refresh (file watching) enabled/disabled
    SetAutoRefresh { enabled: bool },

    /// Get auto-refresh (file watching) status
    GetAutoRefresh,
}

fn default_timeout() -> u64 {
    5
}

fn default_port() -> u16 {
    9000
}

/// Response from daemon to CLI
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Response {
    /// Success response
    Ok {
        #[serde(skip_serializing_if = "Option::is_none")]
        data: Option<ResponseData>,
    },

    /// Error response
    Error {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        recoverable: Option<bool>,
    },
}

impl Response {
    pub fn ok() -> Self {
        Response::Ok { data: None }
    }

    pub fn ok_with(data: ResponseData) -> Self {
        Response::Ok { data: Some(data) }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Response::Error {
            message: message.into(),
            recoverable: None,
        }
    }

    pub fn error_recoverable(message: impl Into<String>) -> Self {
        Response::Error {
            message: message.into(),
            recoverable: Some(true),
        }
    }
}

/// Data payload in successful responses
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseData {
    /// List of renderers
    Renderers { renderers: Vec<RendererInfo> },

    /// Playback status
    Playback {
        state: String,
        position_secs: u64,
        duration_secs: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        current_uri: Option<String>,
    },

    /// Volume info
    Volume { volume: u8, muted: bool },

    /// Daemon info
    DaemonInfo {
        version: String,
        uptime_secs: u64,
        active_streams: usize,
    },

    /// Pong response
    Pong,

    /// Push started
    Streaming {
        device: String,
        file: String,
        url: String,
    },

    /// Queue information
    Queue {
        items: Vec<QueueItemInfo>,
        current_index: Option<usize>,
        shuffle: bool,
        repeat: String,
    },

    /// Saved playback position
    Position {
        uri: String,
        position_secs: u64,
        duration_secs: u64,
        device: String,
        saved_at: u64,
    },

    /// List of saved positions
    Positions { positions: Vec<PositionInfo> },

    /// List of media files from server
    MediaFiles { files: Vec<MediaFileInfo> },

    /// Pending UPnP events
    Events { events: Vec<Notification> },

    /// File count after media operation
    FileCount { count: usize },

    /// Auto-refresh (file watching) status
    AutoRefresh { enabled: bool },
}

/// Queue item info for responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItemInfo {
    pub index: usize,
    pub title: String,
    pub source: String,
    /// Whether this item is a URL (vs a file)
    #[serde(default)]
    pub is_url: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<u64>,
}

/// Position info for responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionInfo {
    pub uri: String,
    pub position_secs: u64,
    pub duration_secs: u64,
    pub device: String,
    pub saved_at: u64,
}

/// Device type for renderers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DeviceType {
    #[default]
    Dlna,
    Chromecast,
}

impl std::fmt::Display for DeviceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeviceType::Dlna => write!(f, "dlna"),
            DeviceType::Chromecast => write!(f, "chromecast"),
        }
    }
}

/// Renderer information for list response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RendererInfo {
    pub name: String,
    pub location: String,
    #[serde(default)]
    pub device_type: DeviceType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mac_address: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub icon_urls: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firmware_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<String>,
}

impl From<&dlna_core::Renderer> for RendererInfo {
    fn from(r: &dlna_core::Renderer) -> Self {
        let icon_urls: Vec<String> = r.icons.iter().map(|icon| icon.url.to_string()).collect();

        tracing::info!(
            "DLNA '{}': Converting to RendererInfo with {} icon(s)",
            r.friendly_name,
            icon_urls.len()
        );

        for (idx, url) in icon_urls.iter().enumerate() {
            tracing::info!("DLNA '{}': icon[{}] = {}", r.friendly_name, idx, url);
        }

        Self {
            name: r.friendly_name.clone(),
            location: r.location.to_string(),
            device_type: DeviceType::Dlna,
            manufacturer: r.manufacturer.clone(),
            model: r.model_name.clone(),
            mac_address: r.mac_address.clone(),
            icon_urls,
            firmware_version: r.firmware_version.clone(),
            capabilities: r.capabilities.clone(),
        }
    }
}

/// Media file information from server library
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaFileInfo {
    pub id: String,
    pub filename: String,
    pub path: PathBuf,
    pub mime_type: String,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
}

impl From<&dlna_core::MediaFile> for MediaFileInfo {
    fn from(f: &dlna_core::MediaFile) -> Self {
        Self {
            id: f.id.clone(),
            filename: f.filename.clone(),
            path: f.path.clone(),
            mime_type: f.mime_type.clone(),
            size: f.size,
            duration_secs: f.duration.map(|d| d.as_secs()),
            title: f.title.clone(),
            artist: f.artist.clone(),
        }
    }
}

/// Real-time notifications pushed from daemon to subscribed clients
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Notification {
    /// Volume changed on a device
    VolumeChanged { device: String, volume: u8 },
    /// Mute state changed
    MuteChanged { device: String, muted: bool },
    /// Transport state changed (PLAYING, PAUSED, STOPPED, etc.)
    TransportStateChanged { device: String, state: String },
    /// Currently playing URI changed
    CurrentUriChanged { device: String, uri: String },
    /// Device subscription lost (reconnect may be needed)
    SubscriptionLost { device: String, service: String },
}

/// Subscribe command for receiving notifications
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum SubscribeCommand {
    /// Subscribe to events for a device
    Subscribe { device: String },
    /// Unsubscribe from events for a device
    Unsubscribe { device: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_serialization() {
        let cmd = Command::List { timeout_secs: 5 };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"cmd\":\"list\""));

        let cmd = Command::Push {
            file: PathBuf::from("/tmp/test.mp4"),
            device: "LG".into(),
            port: 9000,
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"cmd\":\"push\""));
    }

    #[test]
    fn test_command_deserialization() {
        let json = r#"{"cmd":"list","timeout_secs":10}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        assert!(matches!(cmd, Command::List { timeout_secs: 10 }));

        let json = r#"{"cmd":"ping"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        assert!(matches!(cmd, Command::Ping));
    }

    #[test]
    fn test_response_serialization() {
        let resp = Response::ok();
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"status\":\"ok\""));

        let resp = Response::error("Device not found");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"status\":\"error\""));
        assert!(json.contains("Device not found"));
    }
}
