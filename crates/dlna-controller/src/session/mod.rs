//! Universal renderer session management for all protocols
//!
//! Provides trait-based architecture for DLNA and Chromecast.

mod chromecast;
mod dlna;

use async_trait::async_trait;
use dlna_core::{PlaybackInfo, Renderer, Result};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};
use tracing::{info, warn};

pub use chromecast::ChromecastSession;
pub use dlna::DlnaSession;

/// Unified playback request for all renderer types.
#[derive(Debug, Clone)]
pub struct PlayRequest {
    pub url: String,
    pub content_type: Option<String>,
    pub subtitle_url: Option<String>,
    pub title: Option<String>,
}

impl PlayRequest {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            content_type: None,
            subtitle_url: None,
            title: None,
        }
    }

    pub fn with_content_type(mut self, content_type: Option<&str>) -> Self {
        self.content_type = content_type.map(str::to_string);
        self
    }

    pub fn with_subtitle_url(mut self, subtitle_url: Option<&str>) -> Self {
        self.subtitle_url = subtitle_url.map(str::to_string);
        self
    }

    pub fn with_title(mut self, title: Option<&str>) -> Self {
        self.title = title.map(str::to_string);
        self
    }
}

/// Optional queue settings for protocols that support device-side queues.
#[derive(Debug, Clone, Default)]
pub struct QueueOptions {
    pub repeat_mode: Option<String>,
}

impl QueueOptions {
    pub fn new(repeat_mode: Option<String>) -> Self {
        Self { repeat_mode }
    }
}

/// Event from a renderer session (protocol-agnostic)
#[derive(Debug, Clone)]
pub enum SessionEvent {
    /// Volume changed (0-100)
    VolumeChanged { volume: u8 },
    /// Mute state changed
    MuteChanged { muted: bool },
    /// Playback state changed (PLAYING, PAUSED, STOPPED, etc.)
    StateChanged { state: String },
    /// Current URI changed
    UriChanged { uri: String },
    /// Playback position changed (in seconds)
    PositionChanged { position_secs: u64 },
    /// Media duration changed (in seconds)
    DurationChanged { duration_secs: u64 },
    /// Play mode changed (NORMAL, SHUFFLE, REPEAT, etc.)
    PlayModeChanged { mode: String },
    /// Connection lost or session ended
    SessionLost { reason: String },
}

/// Protocol type for a renderer session
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendererProtocol {
    Dlna,
    Chromecast,
}

/// Universal trait for renderer sessions across all protocols
#[async_trait]
pub trait RendererSession: Send + Sync {
    /// Get the protocol type
    fn protocol(&self) -> RendererProtocol;

    /// Get the device identifier (UDN for DLNA, ID for Chromecast, etc.)
    fn device_id(&self) -> &str;

    /// Get the device friendly name
    fn device_name(&self) -> &str;

    /// Check if session is still connected
    async fn is_connected(&self) -> bool;

    /// Subscribe to session events (returns None if protocol doesn't support events)
    fn subscribe_events(&self) -> Option<broadcast::Receiver<SessionEvent>>;

    // === Playback Control ===

    /// Load and play a URL
    async fn play_url(&self, url: &str, content_type: Option<&str>) -> Result<()>;

    /// Load and play a URL starting at specific position
    /// Chromecast: uses native currentTime in LOAD command (instant start at position)
    /// DLNA: does play + wait for PLAYING state + seek (sequential)
    async fn play_url_at_position(
        &self,
        url: &str,
        content_type: Option<&str>,
        position_secs: f64,
    ) -> Result<()>;

    /// Load and play a unified request
    async fn play_request(&self, request: &PlayRequest) -> Result<()> {
        self.play_url(&request.url, request.content_type.as_deref())
            .await
    }

    /// Load and play a unified request at a specific position
    async fn play_request_at_position(
        &self,
        request: &PlayRequest,
        position_secs: f64,
    ) -> Result<()> {
        self.play_url_at_position(&request.url, request.content_type.as_deref(), position_secs)
            .await
    }

    /// Resume playback
    async fn play(&self) -> Result<()>;

    /// Pause playback
    async fn pause(&self) -> Result<()>;

    /// Stop playback
    async fn stop(&self) -> Result<()>;

    /// Seek to position
    async fn seek(&self, position: Duration) -> Result<()>;

    // === Status Queries ===

    /// Get current playback status
    async fn get_status(&self) -> Result<PlaybackInfo>;

    /// Get volume (0-100) and mute state
    async fn get_volume(&self) -> Result<(u8, bool)>;

    // === Volume Control ===

    /// Set volume (0-100)
    async fn set_volume(&self, volume: u8) -> Result<()>;

    /// Set mute state
    async fn set_mute(&self, muted: bool) -> Result<()>;

    // === Advanced Playback (optional per protocol) ===

    /// Get current play mode (NORMAL, SHUFFLE, REPEAT_ONE, REPEAT_ALL)
    async fn get_play_mode(&self) -> Result<String> {
        Err(dlna_core::Error::Upnp("Play mode not supported".into()))
    }

    /// Set play mode (shuffle/repeat)
    async fn set_play_mode(&self, _mode: &str) -> Result<()> {
        Err(dlna_core::Error::Upnp("Play mode not supported".into()))
    }

    /// Skip to next track
    async fn next_track(&self) -> Result<()> {
        Err(dlna_core::Error::Upnp("Next track not supported".into()))
    }

    /// Skip to previous track
    async fn previous_track(&self) -> Result<()> {
        Err(dlna_core::Error::Upnp(
            "Previous track not supported".into(),
        ))
    }

    /// Set next item (device-side queue hint)
    async fn set_next_request(&self, _request: &PlayRequest) -> Result<()> {
        Err(dlna_core::Error::Unsupported(
            "Next item not supported".into(),
        ))
    }

    /// Load a device-side queue (if supported by protocol)
    async fn load_queue(
        &self,
        _items: &[PlayRequest],
        _start_index: usize,
        _options: &QueueOptions,
    ) -> Result<()> {
        Err(dlna_core::Error::Unsupported("Queue not supported".into()))
    }

    // === Lifecycle ===

    /// Close the session (cleanup resources)
    async fn close(&self) -> Result<()>;
}

/// Manager for renderer sessions
pub struct SessionManager {
    /// Active sessions by device ID
    sessions: Arc<RwLock<HashMap<String, Arc<dyn RendererSession>>>>,
    /// EventManager for DLNA devices (optional)
    event_manager: Arc<RwLock<Option<Arc<crate::eventing::EventManager>>>>,
    /// Local IP for event subscriptions
    local_ip: String,
    /// Global event broadcaster (aggregates events from all sessions)
    global_event_tx: broadcast::Sender<(String, SessionEvent)>, // (device_id, event)
}

impl SessionManager {
    /// Create a new session manager
    pub fn new(
        event_manager: Option<Arc<crate::eventing::EventManager>>,
        local_ip: String,
    ) -> Self {
        let (global_event_tx, _) = broadcast::channel(1000);
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            event_manager: Arc::new(RwLock::new(event_manager)),
            local_ip,
            global_event_tx,
        }
    }

    /// Update the EventManager used for new DLNA sessions
    pub async fn set_event_manager(
        &self,
        event_manager: Option<Arc<crate::eventing::EventManager>>,
    ) {
        let mut guard = self.event_manager.write().await;
        *guard = event_manager;
    }

    /// Subscribe to events from all sessions (DLNA + Chromecast)
    ///
    /// Returns a receiver that will receive (device_id, SessionEvent) tuples
    /// from all active and future sessions.
    pub fn subscribe_all_events(&self) -> broadcast::Receiver<(String, SessionEvent)> {
        self.global_event_tx.subscribe()
    }

    /// Get or create a session for a renderer
    pub async fn get_or_create_session(
        &self,
        renderer: &Renderer,
    ) -> Result<Arc<dyn RendererSession>> {
        let device_id = renderer.udn.clone();

        // Check if session already exists
        {
            let sessions = self.sessions.read().await;
            if let Some(session) = sessions.get(&device_id) {
                // Verify session is still connected
                if session.is_connected().await {
                    info!("Reusing existing session for {}", renderer.friendly_name);
                    return Ok(Arc::clone(session));
                } else {
                    warn!(
                        "Existing session for {} is disconnected, creating new one",
                        renderer.friendly_name
                    );
                }
            }
        }

        // Create new session
        let session = self.create_session(renderer).await?;
        let session_arc: Arc<dyn RendererSession> = Arc::from(session);

        // Forward session events to global broadcaster
        if let Some(mut event_rx) = session_arc.subscribe_events() {
            let global_tx = self.global_event_tx.clone();
            let device_id_clone = device_id.clone();
            tokio::spawn(async move {
                while let Ok(event) = event_rx.recv().await {
                    let _ = global_tx.send((device_id_clone.clone(), event));
                }
                info!("Event forwarding stopped for device {}", device_id_clone);
            });
        }

        // Store in map
        {
            let mut sessions = self.sessions.write().await;
            sessions.insert(device_id, Arc::clone(&session_arc));
        }

        Ok(session_arc)
    }

    /// Create a new session for a renderer
    async fn create_session(&self, renderer: &Renderer) -> Result<Box<dyn RendererSession>> {
        // Determine protocol from renderer location
        let protocol = if renderer.location.scheme() == "cast" {
            RendererProtocol::Chromecast
        } else {
            RendererProtocol::Dlna
        };

        match protocol {
            RendererProtocol::Dlna => {
                let event_manager = { self.event_manager.read().await.clone() };
                let session =
                    DlnaSession::new(renderer.clone(), event_manager, &self.local_ip).await?;
                Ok(Box::new(session))
            }
            RendererProtocol::Chromecast => {
                // Parse host and port from cast:// URL
                let host = renderer.location.host_str().ok_or_else(|| {
                    dlna_core::Error::Config("Invalid Chromecast URL: missing host".into())
                })?;
                let port = renderer.location.port().unwrap_or(8009);

                // Parse host as IpAddr
                let host_ip: std::net::IpAddr = host.parse().map_err(|e| {
                    dlna_core::Error::Config(format!("Invalid Chromecast host IP: {e}"))
                })?;

                // Create ChromecastDevice from renderer
                let device = crate::chromecast::ChromecastDevice {
                    name: renderer.friendly_name.clone(),
                    host: host_ip,
                    port,
                    model: renderer.model_name.clone(),
                    id: renderer.udn.clone(),
                    icon_path: None,
                    version: renderer.firmware_version.clone(),
                    capabilities: renderer.capabilities.clone(),
                    status: None,
                };

                let session = ChromecastSession::new(device).await?;
                Ok(Box::new(session))
            }
        }
    }

    /// Close a session for a device
    pub async fn close_session(&self, device_id: &str) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.remove(device_id) {
            session.close().await?;
        }
        Ok(())
    }

    /// Close all sessions
    pub async fn close_all_sessions(&self) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        for (_, session) in sessions.drain() {
            if let Err(e) = session.close().await {
                warn!("Error closing session: {}", e);
            }
        }
        Ok(())
    }

    /// Get current session count
    pub async fn session_count(&self) -> usize {
        self.sessions.read().await.len()
    }
}

impl Drop for SessionManager {
    fn drop(&mut self) {
        // Sessions will be closed when their Arcs are dropped
        info!("SessionManager dropped");
    }
}
