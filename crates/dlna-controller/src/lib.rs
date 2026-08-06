//! dlna-controller - DLNA Controller (DMC) implementation
//!
//! This crate provides a DLNA/UPnP Controller that can:
//! - Discover Media Renderers on the network
//! - Control playback (play, pause, stop, seek)
//! - Push local files to renderers
//!
//! Optional Chromecast support via the `chromecast` feature.

pub mod cache;
pub mod discover;
pub mod eventing;
pub mod playability;
pub mod session;
pub mod streaming;
pub mod subtitles;
pub mod transport;

pub mod chromecast;
pub mod wol;

// Re-export SOAP configuration for retry/timeout settings
pub use transport::{set_soap_config, set_uri_with_subtitle, SoapConfig};

// Re-export device cache
pub use cache::DeviceCache;

// Re-export subtitle utilities
pub use subtitles::{find_subtitle_for_video, generate_didl_with_subtitle};

// Re-export eventing types
pub use eventing::{EventManager, UpnpEvent};

// Re-export session types
pub use playability::{Playability, PlayabilityReport};
pub use session::{
    ChromecastSession, DlnaSession, PlayRequest, QueueOptions, RendererProtocol, RendererSession,
    SessionEvent, SessionManager,
};

use dlna_core::{
    ControllerConfig, DeviceCapabilities, MediaInfo, PlaybackInfo, Renderer, Result, SsdpService,
    TransportState,
};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, info};

/// DLNA Controller for discovering and controlling renderers
#[derive(Clone)]
pub struct Controller {
    config: ControllerConfig,
    ssdp: Arc<SsdpService>,
    active_servers: Arc<Mutex<HashMap<String, streaming::StreamingServer>>>,
    device_cache: Option<DeviceCache>,
    session_manager: Arc<SessionManager>,
}

impl Controller {
    /// Create a new controller
    pub async fn new(config: ControllerConfig) -> Result<Self> {
        let ssdp = SsdpService::new(config.streaming_port).await?;
        let session_manager = Arc::new(SessionManager::new(None, ssdp.local_ip().to_string()));

        Ok(Self {
            config,
            ssdp: Arc::new(ssdp),
            active_servers: Arc::new(Mutex::new(HashMap::new())),
            device_cache: None, // No cache by default for backward compatibility
            session_manager,
        })
    }

    /// Create a new controller with device caching enabled
    ///
    /// Device caching significantly improves performance by avoiding full SSDP discovery
    /// on every playback attempt. Cached devices are valid for `cache_ttl_seconds`.
    ///
    /// # Arguments
    /// * `config` - Controller configuration
    /// * `cache_ttl_seconds` - How long to cache discovered devices (e.g., 300 for 5 minutes)
    pub async fn new_with_cache(config: ControllerConfig, cache_ttl_seconds: u64) -> Result<Self> {
        let ssdp = SsdpService::new(config.streaming_port).await?;
        let session_manager = Arc::new(SessionManager::new(None, ssdp.local_ip().to_string()));

        Ok(Self {
            config,
            ssdp: Arc::new(ssdp),
            active_servers: Arc::new(Mutex::new(HashMap::new())),
            device_cache: Some(DeviceCache::new(cache_ttl_seconds)),
            session_manager,
        })
    }

    /// Discover renderers on the network
    pub async fn discover(&self) -> Result<Vec<Renderer>> {
        let renderers = self
            .ssdp
            .discover_renderers(self.config.discovery_timeout)
            .await?;

        // Populate cache if enabled
        if let Some(ref cache) = self.device_cache {
            cache.insert_many(renderers.clone()).await;
        }

        Ok(renderers)
    }

    /// Discover renderers with a custom timeout (used for quick/deep scan UI)
    pub async fn discover_with_timeout(&self, timeout: Duration) -> Result<Vec<Renderer>> {
        let renderers = self.ssdp.discover_renderers(timeout).await?;

        // Populate cache if enabled
        if let Some(ref cache) = self.device_cache {
            cache.insert_many(renderers.clone()).await;
        }

        Ok(renderers)
    }

    /// Discover renderers with progressive updates via channel
    pub async fn discover_progressive(
        &self,
        timeout: Duration,
        tx: tokio::sync::mpsc::Sender<Renderer>,
    ) -> Result<Vec<Renderer>> {
        // Create intermediate channel to intercept devices and populate cache
        let (internal_tx, mut internal_rx) = tokio::sync::mpsc::channel::<Renderer>(100);

        // Spawn task to forward devices and populate cache as they're discovered
        let cache = self.device_cache.clone();
        let user_tx = tx.clone();
        tokio::spawn(async move {
            while let Some(renderer) = internal_rx.recv().await {
                // Add to cache immediately if enabled
                if let Some(ref cache) = cache {
                    cache.insert(renderer.clone()).await;
                }
                // Forward to user
                let _ = user_tx.send(renderer).await;
            }
        });

        // Run SSDP discovery with intermediate channel
        self.ssdp
            .discover_renderers_progressive(timeout, internal_tx)
            .await
    }

    /// Find a specific renderer by name or URL
    ///
    /// Supports:
    /// - Device name (e.g. "Living Room TV") - finds first match
    /// - HTTP URL (e.g. "http://192.168.1.100:1652/") - fetches DLNA device
    /// - Cast URL (e.g. "cast://192.168.1.100:8009") - finds Chromecast by location
    pub async fn find_renderer(&self, query: &str) -> Result<Option<Renderer>> {
        // First check if it's a URL
        if query.starts_with("http://") || query.starts_with("https://") {
            return discover::fetch_renderer_by_url(query).await.map(Some);
        }

        // Check if it's a cast:// URL - find by exact location match
        if query.starts_with("cast://") {
            // Try cache first
            if let Some(ref cache) = self.device_cache {
                if let Some(renderer) = cache.get_by_location(query).await {
                    debug!("Found Chromecast '{}' in cache by location", query);
                    return Ok(Some(renderer));
                }
            }

            // Parse cast:// URL and create minimal Renderer
            // This allows playback even if device wasn't in cache (e.g., TV was in standby during discovery)
            if let Ok(url) = query.parse::<url::Url>() {
                let host = url.host_str().map(|h| h.to_string());
                let port = url.port().unwrap_or(8009);

                if let Some(host) = host {
                    debug!(
                        "Creating Chromecast renderer from URL: {}:{} (not in cache)",
                        host, port
                    );

                    let renderer = dlna_core::Renderer {
                        friendly_name: format!("Chromecast at {host}"),
                        location: url,
                        udn: format!("chromecast:{host}:{port}"),
                        av_transport_url: None,
                        av_transport_service_type: None,
                        rendering_control_url: None,
                        rendering_control_service_type: None,
                        av_transport_event_url: None,
                        rendering_control_event_url: None,
                        manufacturer: Some("Google".to_string()),
                        model_name: None,
                        model_number: None,
                        model_description: None,
                        serial_number: None,
                        presentation_url: None,
                        mac_address: None,
                        icons: vec![],
                        firmware_version: None,
                        capabilities: None,
                    };

                    // Cache it for future use
                    if let Some(ref cache) = self.device_cache {
                        cache.insert(renderer.clone()).await;
                    }

                    return Ok(Some(renderer));
                }
            }

            // If URL parsing failed, fall back to discovery
            let renderers = self.discover().await?;
            return Ok(renderers.into_iter().find(|r| r.location.as_str() == query));
        }

        // Try cache first if enabled
        if let Some(ref cache) = self.device_cache {
            if let Some(renderer) = cache.get_by_name(query).await {
                debug!("Found renderer '{}' in cache", query);
                return Ok(Some(renderer));
            }
            debug!(
                "Renderer '{}' not in cache, falling back to discovery",
                query
            );
        }

        // Fall back to discovery
        let renderers = self.discover().await?;
        Ok(renderers.into_iter().find(|r| r.matches(query)))
    }

    /// Clear the device cache
    ///
    /// Useful after network changes or when devices are known to have changed.
    /// If caching is disabled, this is a no-op.
    pub async fn clear_cache(&self) {
        if let Some(ref cache) = self.device_cache {
            cache.clear().await;
            debug!("Device cache cleared");
        }
    }

    /// Insert a renderer into the cache (useful for caching discovered devices from other sources)
    pub async fn cache_renderer(&self, renderer: Renderer) {
        if let Some(ref cache) = self.device_cache {
            cache.insert(renderer).await;
        }
    }

    /// Access the session manager (protocol-agnostic control/events)
    pub fn session_manager(&self) -> Arc<SessionManager> {
        Arc::clone(&self.session_manager)
    }

    /// Update the EventManager used for DLNA session eventing
    pub async fn set_event_manager(&self, event_manager: Option<Arc<EventManager>>) {
        self.session_manager.set_event_manager(event_manager).await;
    }

    /// Play a URL on a renderer
    pub async fn play_url(&self, renderer: &Renderer, url: &str) -> Result<()> {
        let request = PlayRequest::new(url).with_content_type(guess_mime_from_url(url).as_deref());
        self.play_request(renderer, &request).await
    }

    /// Play a URL on a renderer starting at a specific position
    ///
    /// For Chromecast: uses native currentTime in LOAD command (instant start at position)
    /// For DLNA: does SetURI + Play + wait for PLAYING + Seek (sequential)
    pub async fn play_url_at_position(
        &self,
        renderer: &Renderer,
        url: &str,
        position_secs: f64,
    ) -> Result<()> {
        let request = PlayRequest::new(url).with_content_type(guess_mime_from_url(url).as_deref());
        self.play_request_at_position(renderer, &request, position_secs)
            .await
    }

    /// Play a unified request on a renderer
    pub async fn play_request(&self, renderer: &Renderer, request: &PlayRequest) -> Result<()> {
        let session = self.session_manager.get_or_create_session(renderer).await?;
        session.play_request(request).await
    }

    /// Sync a device-side queue if supported (Chromecast, partial DLNA)
    pub async fn sync_queue(
        &self,
        renderer: &Renderer,
        items: &[PlayRequest],
        start_index: usize,
        options: &QueueOptions,
    ) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }

        let session = self.session_manager.get_or_create_session(renderer).await?;

        match session.load_queue(items, start_index, options).await {
            Ok(()) => Ok(()),
            Err(dlna_core::Error::Unsupported(_)) => {
                let next_index = start_index.saturating_add(1);
                if let Some(next_item) = items.get(next_index) {
                    match session.set_next_request(next_item).await {
                        Ok(()) => Ok(()),
                        Err(dlna_core::Error::Unsupported(_)) => Ok(()),
                        Err(e) => Err(e),
                    }
                } else {
                    Ok(())
                }
            }
            Err(e) => Err(e),
        }
    }

    /// Check whether a renderer is likely to play a request (best-effort).
    pub fn assess_playability_for_request(
        &self,
        renderer: &Renderer,
        request: &PlayRequest,
    ) -> PlayabilityReport {
        playability::assess_request(renderer, request)
    }

    /// Play a unified request at position on a renderer
    pub async fn play_request_at_position(
        &self,
        renderer: &Renderer,
        request: &PlayRequest,
        position_secs: f64,
    ) -> Result<()> {
        let session = self.session_manager.get_or_create_session(renderer).await?;
        session
            .play_request_at_position(request, position_secs)
            .await
    }

    /// Check whether a renderer is likely to play a local file (best-effort).
    pub fn assess_playability_for_file(
        &self,
        renderer: &Renderer,
        path: &Path,
    ) -> PlayabilityReport {
        playability::assess_file(renderer, path)
    }

    /// Play a local file on a renderer (with automatic subtitle detection)
    pub async fn play_file(&self, renderer: &Renderer, path: &Path) -> Result<()> {
        let preferred_port = self.select_streaming_port(renderer).await;
        self.play_file_with_port(renderer, path, preferred_port)
            .await
            .map(|_| ())
    }

    /// Play a local file on a renderer at a specific position
    pub async fn play_file_at_position(
        &self,
        renderer: &Renderer,
        path: &Path,
        position_secs: f64,
    ) -> Result<()> {
        let preferred_port = self.select_streaming_port(renderer).await;
        self.remove_streaming_server(renderer).await;
        let (server, video_url, subtitle_url, mime) =
            self.start_streaming_server(path, preferred_port).await?;
        let title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_string);

        if let Err(e) = self
            .play_stream_url_at_position(
                renderer,
                &video_url,
                mime.as_deref(),
                subtitle_url.as_deref(),
                title.as_deref(),
                position_secs,
            )
            .await
        {
            server.stop();
            return Err(e);
        }

        self.store_streaming_server(renderer, server).await;
        Ok(())
    }

    /// Play a local file on a renderer with an explicit streaming port
    ///
    /// Returns `(video_url, optional_subtitle_url)` for display/diagnostics.
    pub async fn play_file_with_port(
        &self,
        renderer: &Renderer,
        path: &Path,
        streaming_port: u16,
    ) -> Result<(String, Option<String>)> {
        info!("Playing file {:?} on {}", path, renderer.friendly_name);

        self.remove_streaming_server(renderer).await;
        let (server, video_url, subtitle_url, mime) =
            self.start_streaming_server(path, streaming_port).await?;

        debug!("Streaming file at: {}", video_url);
        if let Some(ref sub_url) = subtitle_url {
            debug!("Subtitle available at: {}", sub_url);
        }

        if let Err(e) = self
            .play_stream_url(
                renderer,
                &video_url,
                mime.as_deref(),
                subtitle_url.as_deref(),
                path.file_stem().and_then(|s| s.to_str()),
            )
            .await
        {
            server.stop();
            return Err(e);
        }

        self.store_streaming_server(renderer, server).await;

        Ok((video_url, subtitle_url))
    }

    /// Pause playback on a renderer
    pub async fn pause(&self, renderer: &Renderer) -> Result<()> {
        let session = self.session_manager.get_or_create_session(renderer).await?;
        session.pause().await
    }

    /// Resume playback on a renderer (same as Play after Pause)
    pub async fn resume(&self, renderer: &Renderer) -> Result<()> {
        let session = self.session_manager.get_or_create_session(renderer).await?;
        session.play().await
    }

    /// Stop playback on a renderer
    pub async fn stop(&self, renderer: &Renderer) -> Result<()> {
        self.remove_streaming_server(renderer).await;
        let session = self.session_manager.get_or_create_session(renderer).await?;
        session.stop().await
    }

    /// Seek to a position
    pub async fn seek(&self, renderer: &Renderer, position: Duration) -> Result<()> {
        let session = self.session_manager.get_or_create_session(renderer).await?;
        session.seek(position).await
    }

    /// Get current playback position
    pub async fn get_position(&self, renderer: &Renderer) -> Result<PlaybackInfo> {
        let session = self.session_manager.get_or_create_session(renderer).await?;
        session.get_status().await
    }

    /// Get transport state
    pub async fn get_transport_state(&self, renderer: &Renderer) -> Result<TransportState> {
        let session = self.session_manager.get_or_create_session(renderer).await?;
        let info = session.get_status().await?;
        Ok(info.state)
    }

    /// Get media information (duration, metadata, etc.)
    ///
    /// This retrieves information about the currently loaded media from the device.
    /// Use this as a fallback when local metadata parsing (ffprobe) is unavailable.
    pub async fn get_media_info(&self, renderer: &Renderer) -> Result<MediaInfo> {
        transport::get_media_info(renderer).await
    }

    /// Get device capabilities (supported media formats, record capabilities, etc.)
    ///
    /// This retrieves the device's supported formats and capabilities.
    /// Useful for format negotiation and feature detection.
    pub async fn get_device_capabilities(&self, renderer: &Renderer) -> Result<DeviceCapabilities> {
        transport::get_device_capabilities(renderer).await
    }

    /// Get the streaming port
    pub fn streaming_port(&self) -> u16 {
        self.config.streaming_port
    }

    /// Get the local IP address
    pub fn local_ip(&self) -> std::net::IpAddr {
        self.ssdp.local_ip()
    }

    async fn select_streaming_port(&self, renderer: &Renderer) -> u16 {
        let key = Self::renderer_key(renderer);
        let active = self.active_servers.lock().await;
        if active.is_empty() || (active.len() == 1 && active.contains_key(&key)) {
            self.config.streaming_port
        } else {
            0
        }
    }

    async fn start_streaming_server(
        &self,
        path: &Path,
        streaming_port: u16,
    ) -> Result<(
        streaming::StreamingServer,
        String,
        Option<String>,
        Option<String>,
    )> {
        let (server, video_url, subtitle_url) =
            streaming::StreamingServer::start(path.to_path_buf(), streaming_port).await?;
        let mime = mime_guess::from_path(path).first().map(|m| m.to_string());
        Ok((server, video_url, subtitle_url, mime))
    }

    async fn play_stream_url(
        &self,
        renderer: &Renderer,
        url: &str,
        mime: Option<&str>,
        subtitle_url: Option<&str>,
        title: Option<&str>,
    ) -> Result<()> {
        let session = self.session_manager.get_or_create_session(renderer).await?;
        let request = PlayRequest::new(url)
            .with_content_type(mime)
            .with_subtitle_url(subtitle_url)
            .with_title(title);
        session.play_request(&request).await
    }

    async fn play_stream_url_at_position(
        &self,
        renderer: &Renderer,
        url: &str,
        mime: Option<&str>,
        subtitle_url: Option<&str>,
        title: Option<&str>,
        position_secs: f64,
    ) -> Result<()> {
        let session = self.session_manager.get_or_create_session(renderer).await?;
        let request = PlayRequest::new(url)
            .with_content_type(mime)
            .with_subtitle_url(subtitle_url)
            .with_title(title);
        session
            .play_request_at_position(&request, position_secs)
            .await
    }

    async fn store_streaming_server(
        &self,
        renderer: &Renderer,
        server: streaming::StreamingServer,
    ) {
        let key = Self::renderer_key(renderer);
        let mut active = self.active_servers.lock().await;
        if let Some(old_server) = active.remove(&key) {
            old_server.stop();
        }
        active.insert(key, server);
    }

    async fn remove_streaming_server(&self, renderer: &Renderer) {
        let key = Self::renderer_key(renderer);
        if let Some(old_server) = self.active_servers.lock().await.remove(&key) {
            old_server.stop();
        }
    }

    fn renderer_key(renderer: &Renderer) -> String {
        renderer.udn.clone()
    }

    // === Volume Control ===

    /// Get current volume level (0-100)
    pub async fn get_volume(&self, renderer: &Renderer) -> Result<u8> {
        let session = self.session_manager.get_or_create_session(renderer).await?;
        let (volume, _) = session.get_volume().await?;
        Ok(volume)
    }

    /// Set volume level (0-100)
    pub async fn set_volume(&self, renderer: &Renderer, volume: u8) -> Result<()> {
        let session = self.session_manager.get_or_create_session(renderer).await?;
        session.set_volume(volume).await
    }

    /// Get mute state
    pub async fn get_mute(&self, renderer: &Renderer) -> Result<bool> {
        let session = self.session_manager.get_or_create_session(renderer).await?;
        let (_, muted) = session.get_volume().await?;
        Ok(muted)
    }

    /// Set mute state
    pub async fn set_mute(&self, renderer: &Renderer, mute: bool) -> Result<()> {
        let session = self.session_manager.get_or_create_session(renderer).await?;
        session.set_mute(mute).await
    }

    // === Playback Mode Control ===

    /// Get current play mode (NORMAL, SHUFFLE, REPEAT_ONE, REPEAT_ALL)
    ///
    /// Note: Many devices don't support this action or always return "NORMAL"
    pub async fn get_play_mode(&self, renderer: &Renderer) -> Result<String> {
        let session = self.session_manager.get_or_create_session(renderer).await?;
        session.get_play_mode().await
    }

    /// Set play mode (shuffle/repeat)
    ///
    /// # Arguments
    /// * `mode` - Play mode: "NORMAL", "SHUFFLE", "REPEAT_ONE", "REPEAT_ALL"
    ///
    /// Note: Many devices don't support this action. Use graceful degradation:
    /// - If this fails with UPnP error 401 (Invalid Action), implement shuffle/repeat in app
    pub async fn set_play_mode(&self, renderer: &Renderer, mode: &str) -> Result<()> {
        let session = self.session_manager.get_or_create_session(renderer).await?;
        session.set_play_mode(mode).await
    }

    // === Track Navigation ===

    /// Skip to next track
    ///
    /// Note: Most DLNA TVs don't support this action (only for multi-track media like playlists)
    /// Use app-level queue management as fallback
    pub async fn next_track(&self, renderer: &Renderer) -> Result<()> {
        let session = self.session_manager.get_or_create_session(renderer).await?;
        session.next_track().await
    }

    /// Skip to previous track
    ///
    /// Note: Most DLNA TVs don't support this action (only for multi-track media like playlists)
    /// Use app-level queue management as fallback
    pub async fn previous_track(&self, renderer: &Renderer) -> Result<()> {
        let session = self.session_manager.get_or_create_session(renderer).await?;
        session.previous_track().await
    }
}

fn guess_mime_from_url(url: &str) -> Option<String> {
    let path = url::Url::parse(url)
        .ok()
        .map(|parsed| parsed.path().to_string())
        .filter(|parsed_path| !parsed_path.is_empty())
        .unwrap_or_else(|| url.to_string());

    mime_guess::from_path(path).first().map(|m| m.to_string())
}
