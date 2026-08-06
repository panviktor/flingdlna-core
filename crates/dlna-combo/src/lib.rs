//! dlna-combo - Unified DLNA library combining server and controller
//!
//! This crate provides a unified API for running a DLNA Media Server
//! and controlling DLNA Media Renderers.
//!
//! Includes a daemon mode for persistent operation with Unix socket IPC.

#[cfg(feature = "daemon")]
pub mod daemon;
pub mod database;
pub mod protocol;
pub mod queue;

use dlna_controller::Controller;
use dlna_server::MediaServer;
use std::path::{Path, PathBuf};
use std::time::Duration;

// Re-export core types
pub use dlna_core::{
    config,
    // WOL functions
    get_mac_from_arp,
    wake,
    ControllerConfig,
    Error,
    MediaFile,
    MediaType,
    PlaybackInfo,
    Renderer,
    Result,
    ServerConfig,
    TransportState,
};

// Re-export daemon types
#[cfg(feature = "daemon")]
pub use daemon::{default_socket_path, is_daemon_running, Daemon, DaemonClient, DaemonConfig};

// Re-export database
pub use database::{data_dir, db_path, socket_path, CachedDevice, Database, SavedPosition};
pub use protocol::{
    Command, DeviceType, MediaFileInfo, Notification, QueueItemInfo, RendererInfo, Response,
    ResponseData,
};

// Re-export server and controller
pub use dlna_controller::{Controller as DlnaController, PlayRequest, QueueOptions};
pub use dlna_server::{
    count_media_files, scan_directory_with_progress, MediaServer as DlnaServer, ScanProgress,
};

/// Unified DLNA combo providing both server and controller functionality
pub struct DlnaCombo {
    server: Option<MediaServer>,
    controller: Controller,
    #[allow(dead_code)]
    server_config: Option<ServerConfig>,
}

impl DlnaCombo {
    /// Create a new DlnaCombo instance
    pub async fn new(config: config::ComboConfig) -> Result<Self> {
        // Use caching with 5-minute TTL for better performance
        let controller = Controller::new_with_cache(config.controller, 300).await?;

        let (server, server_config) = if let Some(server_config) = config.server {
            let server = MediaServer::new(server_config.clone()).await?;
            (Some(server), Some(server_config))
        } else {
            (None, None)
        };

        Ok(Self {
            server,
            controller,
            server_config,
        })
    }

    /// Create with only controller (no server)
    pub async fn controller_only(config: ControllerConfig) -> Result<Self> {
        // Use caching with 5-minute TTL for better performance
        let controller = Controller::new_with_cache(config, 300).await?;

        Ok(Self {
            server: None,
            controller,
            server_config: None,
        })
    }

    /// Create with both server and controller
    pub async fn with_server(
        server_config: ServerConfig,
        controller_config: ControllerConfig,
    ) -> Result<Self> {
        // Use caching with 5-minute TTL for better performance
        let controller = Controller::new_with_cache(controller_config, 300).await?;
        let server = MediaServer::new(server_config.clone()).await?;

        Ok(Self {
            server: Some(server),
            controller,
            server_config: Some(server_config),
        })
    }

    // === Server Operations ===

    /// Start the media server (if configured)
    pub async fn start_server(&mut self) -> Result<()> {
        if let Some(ref mut server) = self.server {
            server.start().await
        } else {
            Err(Error::Config("Server not configured".into()))
        }
    }

    /// Stop the media server
    pub async fn stop_server(&mut self) {
        if let Some(ref mut server) = self.server {
            server.stop().await;
        }
    }

    /// Update the server's friendly name without restarting
    ///
    /// This sends SSDP byebye with the old name, updates the name, and sends alive with the new name.
    /// Returns an error if no server is running.
    pub async fn set_server_name(&self, new_name: String) -> Result<()> {
        if let Some(ref server) = self.server {
            server.set_server_name(new_name).await
        } else {
            Err(Error::Config("Server not configured".into()))
        }
    }

    /// Add a media directory (scans and indexes files)
    pub fn add_media_directory(&mut self, path: PathBuf) -> Result<usize> {
        if let Some(ref mut server) = self.server {
            server.add_directory(path)
        } else {
            Err(Error::Config("Server not configured".into()))
        }
    }

    /// Add a media directory with progress callback
    pub fn add_media_directory_with_progress<F>(
        &mut self,
        path: PathBuf,
        progress_callback: F,
    ) -> Result<usize>
    where
        F: Fn(&ScanProgress) + Send + Sync,
    {
        if let Some(ref mut server) = self.server {
            server.add_directory_with_progress(path, progress_callback)
        } else {
            Err(Error::Config("Server not configured".into()))
        }
    }

    /// Clear all media files and set a new directory
    pub fn set_media_directory(&mut self, path: PathBuf) -> Result<usize> {
        if let Some(ref mut server) = self.server {
            server.set_directories(vec![path])
        } else {
            Err(Error::Config("Server not configured".into()))
        }
    }

    /// Remove a media directory
    pub fn remove_media_directory(&mut self, path: PathBuf) -> Result<()> {
        if let Some(ref mut server) = self.server {
            server.remove_directory(&path)
        } else {
            Err(Error::Config("Server not configured".into()))
        }
    }

    /// Clear all media files
    pub fn clear_media_files(&self) {
        if let Some(ref server) = self.server {
            server.clear_files();
        }
    }

    /// Clear all media directories and files
    pub fn clear_media_directories(&mut self) -> Result<()> {
        if let Some(ref mut server) = self.server {
            server.set_directories(Vec::new())?;
            Ok(())
        } else {
            Err(Error::Config("Server not configured".into()))
        }
    }

    /// Add a single media file by path (ad-hoc rescan)
    pub fn add_file(&self, path: PathBuf) -> Result<Option<MediaFile>> {
        if let Some(ref server) = self.server {
            server.add_file(path)
        } else {
            Err(Error::Config("Server not configured".into()))
        }
    }

    /// Get all media files from the server
    pub fn media_files(&self) -> Vec<MediaFile> {
        self.server.as_ref().map(|s| s.files()).unwrap_or_default()
    }

    /// Get the server's base URL
    pub fn server_url(&self) -> Option<String> {
        self.server.as_ref().map(|s| s.base_url())
    }

    /// Enable or disable file watching dynamically
    pub fn set_file_watch_enabled(&mut self, enabled: bool) -> Result<()> {
        if let Some(ref mut server) = self.server {
            server.set_watch_enabled(enabled)
        } else {
            Err(Error::Config("Server not configured".into()))
        }
    }

    /// Check if file watching is enabled
    pub fn is_file_watch_enabled(&self) -> bool {
        self.server
            .as_ref()
            .map(|s| s.watch_enabled())
            .unwrap_or(false)
    }

    // === Controller Operations ===

    /// Discover renderers on the network
    pub async fn discover_renderers(&self) -> Result<Vec<Renderer>> {
        use dlna_controller::chromecast;
        use std::time::Duration;

        let timeout = Duration::from_secs(30); // Default discovery timeout

        // Run DLNA and Chromecast discovery in parallel
        let (dlna_result, chromecast_result) =
            tokio::join!(self.controller.discover(), chromecast::discover(timeout));

        let mut renderers = dlna_result?;

        match chromecast_result {
            Ok(devices) => {
                tracing::info!("Chromecast discovery found {} device(s)", devices.len());
                for device in devices {
                    // Create a minimal stub Renderer for Chromecast
                    // Use cast:// URL scheme to distinguish from DLNA
                    let cast_url = format!("cast://{}:{}", device.host, device.port);
                    if let Ok(location) = cast_url.parse() {
                        let renderer = Renderer {
                            friendly_name: device.name.clone(),
                            location,
                            udn: format!("chromecast:{}", device.id),
                            av_transport_url: None,
                            av_transport_service_type: None,
                            rendering_control_url: None,
                            rendering_control_service_type: None,
                            av_transport_event_url: None,
                            rendering_control_event_url: None,
                            manufacturer: Some("Google".to_string()),
                            model_name: device.model.clone(),
                            model_number: None, // Chromecast doesn't expose model number
                            model_description: None, // Chromecast doesn't expose model description
                            serial_number: None, // Chromecast doesn't expose serial number
                            presentation_url: None, // Chromecast doesn't have web UI
                            mac_address: None,
                            icons: Vec::new(), // Chromecast devices don't provide icons via UPnP
                            firmware_version: device.version.clone(), // From mDNS 've' field
                            capabilities: device.capabilities.clone(), // From mDNS 'ca' field
                        };
                        renderers.push(renderer);
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Chromecast discovery failed: {}", e);
            }
        }

        Ok(renderers)
    }

    /// Discover renderers with a custom timeout (used for quick/deep scan UI)
    pub async fn discover_renderers_with_timeout(
        &self,
        timeout: Duration,
    ) -> Result<Vec<Renderer>> {
        use dlna_controller::chromecast;

        // Run DLNA and Chromecast discovery in parallel
        let (dlna_result, chromecast_result) = tokio::join!(
            self.controller.discover_with_timeout(timeout),
            chromecast::discover(timeout)
        );

        let mut renderers = dlna_result?;
        tracing::info!(
            "DLNA discovery completed, found {} DLNA devices",
            renderers.len()
        );

        match chromecast_result {
            Ok(devices) => {
                tracing::info!("Chromecast discovery found {} device(s)", devices.len());
                for device in devices {
                    // Create a minimal stub Renderer for Chromecast
                    // Use cast:// URL scheme to distinguish from DLNA
                    let cast_url = format!("cast://{}:{}", device.host, device.port);
                    if let Ok(location) = cast_url.parse() {
                        let renderer = Renderer {
                            friendly_name: device.name.clone(),
                            location,
                            udn: format!("chromecast:{}", device.id),
                            av_transport_url: None,
                            av_transport_service_type: None,
                            rendering_control_url: None,
                            rendering_control_service_type: None,
                            av_transport_event_url: None,
                            rendering_control_event_url: None,
                            manufacturer: Some("Google".to_string()),
                            model_name: device.model.clone(),
                            model_number: None, // Chromecast doesn't expose model number
                            model_description: None, // Chromecast doesn't expose model description
                            serial_number: None, // Chromecast doesn't expose serial number
                            presentation_url: None, // Chromecast doesn't have web UI
                            mac_address: None,
                            icons: Vec::new(), // Chromecast devices don't provide icons via UPnP
                            firmware_version: device.version.clone(), // From mDNS 've' field
                            capabilities: device.capabilities.clone(), // From mDNS 'ca' field
                        };
                        renderers.push(renderer);
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Chromecast discovery failed: {}", e);
            }
        }

        Ok(renderers)
    }

    /// Find a renderer by name or URL
    pub async fn find_renderer(&self, query: &str) -> Result<Option<Renderer>> {
        self.controller.find_renderer(query).await
    }

    /// Push a local file to a renderer
    pub async fn push(&self, file: &Path, renderer: &Renderer) -> Result<()> {
        self.controller.play_file(renderer, file).await
    }

    /// Push a local file to a renderer starting at a specific position
    pub async fn push_at_position(
        &self,
        file: &Path,
        renderer: &Renderer,
        position_secs: f64,
    ) -> Result<()> {
        self.controller
            .play_file_at_position(renderer, file, position_secs)
            .await
    }

    /// Play a URL on a renderer
    pub async fn play_url(&self, url: &str, renderer: &Renderer) -> Result<()> {
        self.controller.play_url(renderer, url).await
    }

    /// Play a URL on a renderer starting at a specific position
    ///
    /// For Chromecast: uses native currentTime in LOAD command (instant start)
    /// For DLNA: does SetURI + Play + wait + Seek (sequential)
    pub async fn play_url_at_position(
        &self,
        url: &str,
        renderer: &Renderer,
        position_secs: f64,
    ) -> Result<()> {
        self.controller
            .play_url_at_position(renderer, url, position_secs)
            .await
    }

    /// Play a unified request on a renderer
    pub async fn play_request(&self, request: &PlayRequest, renderer: &Renderer) -> Result<()> {
        self.controller.play_request(renderer, request).await
    }

    /// Sync a device-side queue if supported
    pub async fn sync_queue(
        &self,
        renderer: &Renderer,
        items: &[PlayRequest],
        start_index: usize,
        options: &QueueOptions,
    ) -> Result<()> {
        self.controller
            .sync_queue(renderer, items, start_index, options)
            .await
    }

    /// Play a unified request on a renderer starting at a specific position
    pub async fn play_request_at_position(
        &self,
        request: &PlayRequest,
        renderer: &Renderer,
        position_secs: f64,
    ) -> Result<()> {
        self.controller
            .play_request_at_position(renderer, request, position_secs)
            .await
    }

    /// Pause playback on a renderer
    pub async fn pause(&self, renderer: &Renderer) -> Result<()> {
        self.controller.pause(renderer).await
    }

    /// Stop playback on a renderer
    pub async fn stop(&self, renderer: &Renderer) -> Result<()> {
        self.controller.stop(renderer).await
    }

    /// Seek to a position
    pub async fn seek(&self, renderer: &Renderer, position: Duration) -> Result<()> {
        self.controller.seek(renderer, position).await
    }

    /// Get playback info
    pub async fn get_playback_info(&self, renderer: &Renderer) -> Result<PlaybackInfo> {
        self.controller.get_position(renderer).await
    }

    // === Direct Access ===

    /// Get direct access to the server (if available)
    pub fn server(&self) -> Option<&MediaServer> {
        self.server.as_ref()
    }

    /// Get mutable access to the server
    pub fn server_mut(&mut self) -> Option<&mut MediaServer> {
        self.server.as_mut()
    }

    /// Get direct access to the controller
    pub fn controller(&self) -> &Controller {
        &self.controller
    }

    // === Folder Visibility ===

    /// Set public folders (visible to all devices browsing the server)
    pub fn set_public_folders(&self, folders: Vec<PathBuf>) -> Result<()> {
        if let Some(ref server) = self.server {
            server.set_public_folders(folders);
            Ok(())
        } else {
            Err(Error::Config("Server not configured".into()))
        }
    }

    /// Set folders for a specific device (by IP address)
    pub fn set_device_folders(&self, device_ip: &str, folders: Vec<PathBuf>) -> Result<()> {
        if let Some(ref server) = self.server {
            server.set_device_folders(device_ip, folders);
            Ok(())
        } else {
            Err(Error::Config("Server not configured".into()))
        }
    }

    /// Clear all device folder assignments
    pub fn clear_device_folders(&self) -> Result<()> {
        if let Some(ref server) = self.server {
            server.clear_device_folders();
            Ok(())
        } else {
            Err(Error::Config("Server not configured".into()))
        }
    }
}
