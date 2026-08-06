//! Type definitions for the TUI module

use dlna_combo::{MediaFileInfo, Notification, QueueItemInfo, RendererInfo};
use std::path::PathBuf;

/// Background task result
pub enum BackgroundResult {
    Renderers(Vec<RendererInfo>),
    DiscoveryError(String),
    /// Operation completed successfully (e.g., play, add to queue)
    OperationSuccess(String),
    /// Operation failed
    OperationError(String),
    /// Status update from periodic refresh (playback + volume + queue)
    StatusUpdate(Box<StatusUpdateData>),
    /// Daemon info update
    DaemonInfoUpdate {
        uptime_secs: u64,
        active_streams: usize,
    },
    /// UPnP events received from subscribed device
    EventsReceived(Vec<Notification>),
    /// Subscription result (success or failure)
    SubscriptionResult {
        device: String,
        success: bool,
    },
    /// Queue operation completed with updated queue data (fast path)
    QueueUpdated {
        message: String,
        items: Vec<QueueItemInfo>,
        current_index: Option<usize>,
        shuffle: bool,
        repeat: String,
    },
    /// History list refreshed
    HistoryRefreshed(Vec<HistoryDisplayItem>),
    /// Auto-refresh state changed
    AutoRefreshChanged(bool),
}

/// Status data collected from background refresh
pub struct StatusUpdateData {
    /// Device this status update is for (to prevent applying to wrong device)
    pub device: String,
    pub playback_state: Option<String>,
    pub playback_position: Option<u64>,
    pub playback_duration: Option<u64>,
    pub current_uri: Option<String>,
    pub volume: Option<u8>,
    pub muted: Option<bool>,
    pub queue_items: Option<Vec<QueueItemInfo>>,
    pub queue_current: Option<usize>,
    pub shuffle: Option<bool>,
    pub repeat: Option<String>,
}

/// Queue item for display in the TUI
#[derive(Clone)]
pub struct QueueDisplayItem {
    pub index: usize,
    pub title: String,
    pub source: String,
    pub is_url: bool,
    pub duration: Option<u64>,
}

/// Library entry - either a folder or a file
#[derive(Clone)]
pub enum LibraryEntry {
    /// A folder containing media files
    Folder {
        name: String,
        path: PathBuf,
        file_count: usize,
    },
    /// A media file
    File(MediaFileInfo),
}

/// Focus area in the TUI
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Focus {
    Renderers,
    Library,
    History,
    Queue,
}

/// History item for display in the TUI
#[derive(Clone)]
pub struct HistoryDisplayItem {
    pub uri: String,
    pub title: String,
    pub position_secs: u64,
    pub duration_secs: u64,
    #[allow(dead_code)] // Reserved for future display of device info
    pub device: String,
    pub saved_at: u64,
}

/// User action triggered by keyboard input
#[derive(Debug, Clone)]
pub enum Action {
    SeekBackward(u64),
    SeekForward(u64),
    PlayPause,
    PlayQueueItem,
    Stop,
    NextTrack,
    PrevTrack,
    VolumeUp,
    VolumeDown,
    ToggleMute,
    ToggleShuffle,
    CycleRepeat,
    RefreshRenderers,
    RefreshStatus,
    AddToQueue,
    RemoveFromQueue,
    AddUrlToQueue(String),
    DeviceChanged,
    /// Enter selected folder in library
    EnterFolder,
    /// Go back to parent folder in library
    GoParentFolder,
    /// Resume playback from history item
    ResumeFromHistory(String),
    /// Refresh history list
    RefreshHistory,
    /// Set media directory
    SetMediaDir(String),
    /// Refresh media library
    RefreshMediaLibrary,
    /// Toggle auto-refresh (file watching)
    ToggleAutoRefresh,
}
