//! Background task handlers and spawners for App

use dlna_combo::{Command as DaemonCommand, Notification, Response, ResponseData};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

use super::App;
use crate::tui::types::{BackgroundResult, HistoryDisplayItem, QueueDisplayItem, StatusUpdateData};

impl App {
    // === Background Task Handlers ===

    /// Handle background task result
    pub fn handle_background_result(&mut self, result: BackgroundResult) {
        match result {
            BackgroundResult::Renderers(renderers) => {
                self.discovering = false;
                self.renderers = renderers;
                if self.renderers.is_empty() {
                    self.status_message = "No renderers found".to_string();
                } else {
                    self.status_message = format!("{} renderer(s)", self.renderers.len());
                }
            }
            BackgroundResult::DiscoveryError(msg) => {
                self.discovering = false;
                self.status_message = format!("Discovery error: {msg}");
            }
            BackgroundResult::OperationSuccess(msg) => {
                self.status_message = msg;
                self.pending_operation = None;
            }
            BackgroundResult::OperationError(msg) => {
                self.status_message = format!("Error: {msg}");
                self.pending_operation = None;
            }
            BackgroundResult::StatusUpdate(data) => {
                self.apply_status_update(*data);
            }
            BackgroundResult::DaemonInfoUpdate {
                uptime_secs,
                active_streams,
            } => {
                self.daemon_uptime = uptime_secs;
                self.daemon_streams = active_streams;
            }
            BackgroundResult::EventsReceived(events) => {
                self.apply_events(events);
            }
            BackgroundResult::SubscriptionResult { device, success } => {
                if success {
                    self.subscribed_device = Some(device.clone());
                } else {
                    self.status_message = format!("Failed to subscribe to {device}");
                }
            }
            BackgroundResult::QueueUpdated {
                message,
                items,
                current_index,
                shuffle,
                repeat,
            } => {
                if !message.is_empty() {
                    self.status_message = message;
                }
                self.pending_operation = None;
                self.queue_items = items
                    .into_iter()
                    .map(|i| QueueDisplayItem {
                        index: i.index,
                        title: i.title,
                        source: i.source,
                        is_url: i.is_url,
                        duration: i.duration_secs,
                    })
                    .collect();
                self.update_search_filter();
                self.queue_current = current_index;
                self.shuffle = shuffle;
                self.repeat = repeat;
                self.clamp_queue_selection();
            }
            BackgroundResult::HistoryRefreshed(items) => {
                self.history_items = items;
                // Clamp selection
                if let Some(selected) = self.selected_history.selected() {
                    if selected >= self.history_items.len() {
                        self.selected_history
                            .select(if self.history_items.is_empty() {
                                None
                            } else {
                                Some(0)
                            });
                    }
                } else if !self.history_items.is_empty() {
                    self.selected_history.select(Some(0));
                }
            }
            BackgroundResult::AutoRefreshChanged(enabled) => {
                self.auto_refresh = enabled;
                self.status_message =
                    format!("Auto-refresh: {}", if enabled { "ON" } else { "OFF" });
            }
        }
    }

    fn apply_status_update(&mut self, data: StatusUpdateData) {
        // Only apply update if it's for the currently selected device
        // This prevents volume/state from one device being applied to another
        if self.selected_device() != Some(&data.device) {
            return;
        }
        if let Some(state) = data.playback_state {
            if state == "Stopped" || state == "NoMediaPresent" {
                self.playback_position = 0;
                self.playback_duration = 0;
                self.position_last_update = None;
            }
            self.playback_state = state;
        }
        if self.playback_state != "Stopped" && self.playback_state != "NoMediaPresent" {
            if let Some(pos) = data.playback_position {
                self.playback_position = pos;
                self.position_last_update = Some(std::time::Instant::now());
            }
            if let Some(dur) = data.playback_duration {
                self.playback_duration = dur;
            }
        }
        if data.current_uri.is_some() {
            self.current_uri = data.current_uri;
        }
        if let Some(vol) = data.volume {
            self.volume = vol;
        }
        if let Some(muted) = data.muted {
            self.muted = muted;
        }
        if let Some(items) = data.queue_items {
            self.queue_items = items
                .into_iter()
                .map(|i| QueueDisplayItem {
                    index: i.index,
                    title: i.title,
                    source: i.source,
                    is_url: i.is_url,
                    duration: i.duration_secs,
                })
                .collect();
            self.update_search_filter();
        }
        if data.queue_current.is_some() {
            self.queue_current = data.queue_current;
        }
        if let Some(shuffle) = data.shuffle {
            self.shuffle = shuffle;
        }
        if let Some(repeat) = data.repeat {
            self.repeat = repeat;
        }
    }

    fn apply_events(&mut self, events: Vec<Notification>) {
        let selected = self.selected_device().map(|s| s.to_string());
        for event in events {
            match event {
                Notification::VolumeChanged { device, volume } => {
                    if selected.as_ref().map(|s| s == &device).unwrap_or(false) {
                        self.volume = volume;
                    }
                }
                Notification::MuteChanged { device, muted } => {
                    if selected.as_ref().map(|s| s == &device).unwrap_or(false) {
                        self.muted = muted;
                    }
                }
                Notification::TransportStateChanged { device, state } => {
                    if selected.as_ref().map(|s| s == &device).unwrap_or(false) {
                        self.playback_state = state.clone();
                        self.pending_operation = None;
                        if state == "Stopped" || state == "NoMediaPresent" {
                            self.playback_position = 0;
                            self.playback_duration = 0;
                            self.position_last_update = None;
                        } else if state != "Playing" {
                            self.position_last_update = None;
                        }
                    }
                }
                Notification::CurrentUriChanged { device, uri } => {
                    if selected.as_ref().map(|s| s == &device).unwrap_or(false) {
                        self.current_uri = Some(uri);
                    }
                }
                Notification::SubscriptionLost { device, service } => {
                    if self
                        .subscribed_device
                        .as_ref()
                        .map(|d| d == &device)
                        .unwrap_or(false)
                    {
                        self.subscribed_device = None;
                        self.status_message = format!("Subscription lost ({service}) - will retry");
                    }
                }
            }
        }
    }

    // === Background Task Spawners ===

    /// Start discovery in background
    pub fn start_background_discovery(
        &mut self,
        tx: mpsc::Sender<BackgroundResult>,
        timeout_secs: u64,
    ) {
        if self.discovering {
            return;
        }
        self.discovering = true;
        self.status_message = "Refreshing...".to_string();

        let client = Arc::clone(&self.client);
        tokio::spawn(async move {
            let result = client.send(&DaemonCommand::Refresh { timeout_secs }).await;
            let bg_result = match result {
                Ok(Response::Ok {
                    data: Some(ResponseData::Renderers { renderers }),
                }) => BackgroundResult::Renderers(renderers),
                Ok(Response::Error { message, .. }) => BackgroundResult::DiscoveryError(message),
                Err(e) => BackgroundResult::DiscoveryError(e.to_string()),
                _ => BackgroundResult::DiscoveryError("Unknown error".to_string()),
            };
            let _ = tx.send(bg_result).await;
        });
    }

    /// Start background status refresh (non-blocking)
    pub fn start_background_status_refresh(
        &self,
        tx: mpsc::Sender<BackgroundResult>,
        device: String,
    ) {
        let client = Arc::clone(&self.client);
        let timeout = Duration::from_secs(8);
        tokio::spawn(async move {
            let mut data = StatusUpdateData {
                device: device.clone(),
                playback_state: None,
                playback_position: None,
                playback_duration: None,
                current_uri: None,
                volume: None,
                muted: None,
                queue_items: None,
                queue_current: None,
                shuffle: None,
                repeat: None,
            };

            let status_cmd = DaemonCommand::Status {
                device: device.clone(),
            };
            let volume_cmd = DaemonCommand::GetVolume { device };

            let (status_result, volume_result) = tokio::join!(
                tokio::time::timeout(timeout, client.send(&status_cmd)),
                tokio::time::timeout(timeout, client.send(&volume_cmd))
            );

            if let Ok(Ok(Response::Ok {
                data:
                    Some(ResponseData::Playback {
                        state,
                        position_secs,
                        duration_secs,
                        current_uri,
                    }),
            })) = status_result
            {
                data.playback_state = Some(state);
                data.playback_position = Some(position_secs);
                data.playback_duration = Some(duration_secs);
                data.current_uri = current_uri;
            }

            if let Ok(Ok(Response::Ok {
                data: Some(ResponseData::Volume { volume, muted }),
            })) = volume_result
            {
                data.volume = Some(volume);
                data.muted = Some(muted);
            }

            let _ = tx
                .send(BackgroundResult::StatusUpdate(Box::new(data)))
                .await;
        });
    }

    /// Start background queue refresh
    pub fn start_background_queue_refresh(
        &self,
        tx: mpsc::Sender<BackgroundResult>,
        device: String,
    ) {
        let client = Arc::clone(&self.client);
        tokio::spawn(async move {
            if let Ok(Response::Ok {
                data:
                    Some(ResponseData::Queue {
                        items,
                        current_index,
                        shuffle,
                        repeat,
                    }),
            }) = client.send(&DaemonCommand::QueueList { device }).await
            {
                let _ = tx
                    .send(BackgroundResult::QueueUpdated {
                        message: String::new(),
                        items,
                        current_index,
                        shuffle,
                        repeat,
                    })
                    .await;
            }
        });
    }

    /// Start background daemon info refresh
    pub fn start_background_daemon_info(&self, tx: mpsc::Sender<BackgroundResult>) {
        let client = Arc::clone(&self.client);
        tokio::spawn(async move {
            if let Ok(Response::Ok {
                data:
                    Some(ResponseData::DaemonInfo {
                        uptime_secs,
                        active_streams,
                        ..
                    }),
            }) = client.send(&DaemonCommand::Info).await
            {
                let _ = tx
                    .send(BackgroundResult::DaemonInfoUpdate {
                        uptime_secs,
                        active_streams,
                    })
                    .await;
            }
        });
    }

    /// Poll for UPnP events
    pub fn start_background_event_poll(
        &self,
        tx: mpsc::Sender<BackgroundResult>,
        device: Option<String>,
    ) {
        let client = Arc::clone(&self.client);
        tokio::spawn(async move {
            if let Ok(Response::Ok {
                data: Some(ResponseData::Events { events }),
            }) = client.send(&DaemonCommand::PollEvents { device }).await
            {
                if !events.is_empty() {
                    let _ = tx.send(BackgroundResult::EventsReceived(events)).await;
                }
            }
        });
    }

    /// Subscribe to UPnP events for a device
    pub fn start_background_subscribe(&self, tx: mpsc::Sender<BackgroundResult>, device: String) {
        let client = Arc::clone(&self.client);
        let device_clone = device.clone();
        tokio::spawn(async move {
            let success = matches!(
                client
                    .send(&DaemonCommand::Subscribe {
                        device: device_clone.clone()
                    })
                    .await,
                Ok(Response::Ok { .. })
            );
            let _ = tx
                .send(BackgroundResult::SubscriptionResult {
                    device: device_clone,
                    success,
                })
                .await;
        });
    }

    /// Save current playback position to history (background, fire-and-forget)
    /// Call this periodically during playback and before stop/pause
    pub fn start_background_save_position(&mut self, device: String) {
        // Only save if we have a valid URI and some progress
        if self.current_uri.is_none() {
            return;
        }
        if self.playback_position < 5 {
            // Don't save if less than 5 seconds played
            return;
        }

        let client = Arc::clone(&self.client);
        tokio::spawn(async move {
            // SavePosition command tells daemon to read current position from device and save it
            let _ = client.send(&DaemonCommand::SavePosition { device }).await;
        });

        // Update last save time
        self.last_position_save = Some(std::time::Instant::now());
    }

    /// Check if we should auto-save position (30 seconds since last save, while playing)
    pub fn should_auto_save_position(&self) -> bool {
        if self.playback_state != "Playing" {
            return false;
        }
        if self.current_uri.is_none() {
            return false;
        }
        if self.playback_position < 5 {
            return false;
        }

        match self.last_position_save {
            None => true, // Never saved - should save
            Some(last) => last.elapsed() >= Duration::from_secs(30),
        }
    }

    /// Start background history refresh
    #[allow(dead_code)]
    pub fn start_background_history_refresh(&self, tx: mpsc::Sender<BackgroundResult>) {
        let client = Arc::clone(&self.client);
        tokio::spawn(async move {
            if let Ok(Response::Ok {
                data: Some(ResponseData::Positions { positions }),
            }) = client.send(&DaemonCommand::ListPositions).await
            {
                let items: Vec<HistoryDisplayItem> = positions
                    .into_iter()
                    .map(|p| {
                        let title = p.uri.rsplit('/').next().unwrap_or(&p.uri).to_string();
                        HistoryDisplayItem {
                            uri: p.uri,
                            title,
                            position_secs: p.position_secs,
                            duration_secs: p.duration_secs,
                            device: p.device,
                            saved_at: p.saved_at,
                        }
                    })
                    .collect();
                let _ = tx.send(BackgroundResult::HistoryRefreshed(items)).await;
            }
        });
    }

    /// Start background media library refresh
    pub fn start_background_media_refresh(&self, tx: mpsc::Sender<BackgroundResult>) {
        let client = Arc::clone(&self.client);
        tokio::spawn(async move {
            if let Ok(Response::Ok {
                data: Some(ResponseData::MediaFiles { files }),
            }) = client.send(&DaemonCommand::ListMedia).await
            {
                let _ = tx
                    .send(BackgroundResult::OperationSuccess(format!(
                        "Media library: {} files",
                        files.len()
                    )))
                    .await;
            }
        });
    }

    /// Toggle auto-refresh (file watching)
    pub fn start_background_toggle_auto_refresh(&self, tx: mpsc::Sender<BackgroundResult>) {
        let client = Arc::clone(&self.client);
        let current = self.auto_refresh;
        let new_state = !current;
        tokio::spawn(async move {
            match client
                .send(&DaemonCommand::SetAutoRefresh { enabled: new_state })
                .await
            {
                Ok(Response::Ok {
                    data: Some(ResponseData::AutoRefresh { enabled }),
                }) => {
                    let _ = tx.send(BackgroundResult::AutoRefreshChanged(enabled)).await;
                }
                Ok(Response::Ok { .. }) => {
                    let _ = tx
                        .send(BackgroundResult::AutoRefreshChanged(new_state))
                        .await;
                }
                Ok(Response::Error { message, .. }) => {
                    let _ = tx.send(BackgroundResult::OperationError(message)).await;
                }
                Err(e) => {
                    let _ = tx
                        .send(BackgroundResult::OperationError(e.to_string()))
                        .await;
                }
            }
        });
    }
}
