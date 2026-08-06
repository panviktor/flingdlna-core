//! Application state and logic

mod background;
mod library;
mod navigation;
mod sync;

use dlna_combo::{DaemonClient, MediaFileInfo, RendererInfo};
use ratatui::widgets::ListState;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::tui::types::{Focus, HistoryDisplayItem, LibraryEntry, QueueDisplayItem};

/// TUI Application state
pub struct App {
    pub(crate) client: Arc<DaemonClient>,
    /// Available renderers
    pub(crate) renderers: Vec<RendererInfo>,
    /// Whether discovery is in progress
    pub(crate) discovering: bool,
    /// Selected renderer index
    pub(crate) selected_renderer: ListState,
    /// Current playback state
    pub(crate) playback_state: String,
    pub(crate) playback_position: u64,
    pub(crate) playback_duration: u64,
    pub(crate) current_uri: Option<String>,
    /// Time when playback position was last updated (for interpolation)
    pub(crate) position_last_update: Option<std::time::Instant>,
    /// Volume state
    pub(crate) volume: u8,
    pub(crate) muted: bool,
    /// Queue items
    pub(crate) queue_items: Vec<QueueDisplayItem>,
    pub(crate) queue_current: Option<usize>,
    pub(crate) shuffle: bool,
    pub(crate) repeat: String,
    /// Selected queue item
    pub(crate) selected_queue: ListState,
    /// Media library files (all files from daemon)
    pub(crate) media_files: Vec<MediaFileInfo>,
    /// Current folder in library navigation (None = root showing all serve-dir roots)
    pub(crate) current_folder: Option<PathBuf>,
    /// Computed library entries for current folder (folders + files)
    pub(crate) library_entries: Vec<LibraryEntry>,
    /// Selected library item
    pub(crate) selected_library: ListState,
    /// Watch history items
    pub(crate) history_items: Vec<HistoryDisplayItem>,
    /// Selected history item
    pub(crate) selected_history: ListState,
    /// Status message
    pub(crate) status_message: String,
    /// Current focus area
    pub(crate) focus: Focus,
    /// Search mode
    pub(crate) search_mode: bool,
    pub(crate) search_query: String,
    pub(crate) filtered_queue_indices: Vec<usize>,
    /// URL input mode
    pub(crate) url_input_mode: bool,
    pub(crate) url_input: String,
    /// Directory input mode
    pub(crate) dir_input_mode: bool,
    pub(crate) dir_input: String,
    /// Should quit
    pub(crate) should_quit: bool,
    /// Last action feedback (shown briefly)
    pub(crate) last_action: Option<String>,
    pub(crate) last_action_time: Option<std::time::Instant>,
    /// Daemon info
    pub(crate) daemon_uptime: u64,
    pub(crate) daemon_streams: usize,
    /// Currently subscribed device (for UPnP events)
    pub(crate) subscribed_device: Option<String>,
    /// Pending operation indicator (e.g., "Pausing...", "Seeking...")
    pub(crate) pending_operation: Option<String>,
    /// Last time position was auto-saved (for periodic auto-save during playback)
    pub(crate) last_position_save: Option<std::time::Instant>,
    /// Auto-refresh (file watching) enabled
    pub(crate) auto_refresh: bool,
}

impl App {
    pub fn new(client: DaemonClient) -> Self {
        let mut selected_renderer = ListState::default();
        selected_renderer.select(Some(0));
        let mut selected_queue = ListState::default();
        selected_queue.select(Some(0));
        let mut selected_library = ListState::default();
        selected_library.select(Some(0));

        Self {
            client: Arc::new(client),
            renderers: Vec::new(),
            discovering: false,
            selected_renderer,
            playback_state: "Unknown".to_string(),
            playback_position: 0,
            playback_duration: 0,
            current_uri: None,
            position_last_update: None,
            volume: 0,
            muted: false,
            queue_items: Vec::new(),
            queue_current: None,
            shuffle: false,
            repeat: "none".to_string(),
            selected_queue,
            media_files: Vec::new(),
            current_folder: None,
            library_entries: Vec::new(),
            selected_library,
            history_items: Vec::new(),
            selected_history: {
                let mut state = ListState::default();
                state.select(Some(0));
                state
            },
            status_message: "Press '?' for help".to_string(),
            focus: Focus::Renderers,
            search_mode: false,
            search_query: String::new(),
            filtered_queue_indices: Vec::new(),
            url_input_mode: false,
            url_input: String::new(),
            dir_input_mode: false,
            dir_input: String::new(),
            should_quit: false,
            last_action: None,
            last_action_time: None,
            daemon_uptime: 0,
            daemon_streams: 0,
            subscribed_device: None,
            pending_operation: None,
            last_position_save: None,
            auto_refresh: true, // Default enabled
        }
    }

    // === UI Helpers ===

    /// Set action feedback (shown for ~1 second)
    pub fn set_action(&mut self, action: impl Into<String>) {
        self.last_action = Some(action.into());
        self.last_action_time = Some(std::time::Instant::now());
    }

    /// Clear action if expired (after 1 second)
    pub fn clear_expired_action(&mut self) {
        if let Some(time) = self.last_action_time {
            if time.elapsed() > Duration::from_secs(1) {
                self.last_action = None;
                self.last_action_time = None;
            }
        }
    }

    /// Get interpolated playback position (smooth updates between daemon polls)
    pub fn interpolated_position(&self) -> u64 {
        if self.playback_state == "Playing" {
            if let Some(last_update) = self.position_last_update {
                let elapsed = last_update.elapsed().as_secs();
                let interpolated = self.playback_position.saturating_add(elapsed);
                return interpolated.min(self.playback_duration);
            }
        }
        self.playback_position
    }

    // === Search/Filter ===

    pub fn update_search_filter(&mut self) {
        if self.search_query.is_empty() {
            self.filtered_queue_indices = (0..self.queue_items.len()).collect();
        } else {
            let query = self.search_query.to_lowercase();
            self.filtered_queue_indices = self
                .queue_items
                .iter()
                .enumerate()
                .filter(|(_, item)| item.title.to_lowercase().contains(&query))
                .map(|(i, _)| i)
                .collect();
        }
    }

    /// Clamp selection to valid range after queue update
    pub fn clamp_queue_selection(&mut self) {
        if let Some(selected) = self.selected_queue.selected() {
            if self.filtered_queue_indices.is_empty() {
                self.selected_queue.select(None);
            } else if selected >= self.filtered_queue_indices.len() {
                self.selected_queue
                    .select(Some(self.filtered_queue_indices.len() - 1));
            }
        }
    }
}
