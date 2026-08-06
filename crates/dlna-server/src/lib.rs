//! dlna-server - DLNA Media Server (DMS) implementation
//!
//! This crate provides a DLNA/UPnP Media Server that can:
//! - Scan directories for media files
//! - Announce itself on the network via SSDP
//! - Serve media files via HTTP with Range request support
//! - Implement ContentDirectory service for browsing
//! - Watch directories for changes and auto-update

pub mod cache;
pub mod content_directory;
pub mod description;
pub mod didl;
pub mod http;
pub mod metadata;
pub mod scanner;
pub mod watcher;

mod state;

use dlna_core::{Result, ServerConfig, ServerInfo, SsdpService};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

pub use dlna_core::MediaFile;
pub use scanner::{count_media_files, scan_directory_with_progress, ScanProgress};
pub use watcher::{create_watcher, FileWatcher, WatchEvent, WatchEventProcessor};

/// DLNA Media Server
pub struct MediaServer {
    config: ServerConfig,
    state: Arc<state::ServerState>,
    ssdp: Arc<SsdpService>,
    shutdown_tx: Option<broadcast::Sender<()>>,
    /// File watcher (if enabled)
    #[allow(dead_code)]
    watcher: Option<FileWatcher>,
    /// Whether file watching is enabled
    watch_enabled: bool,
    /// Debounce duration for file watcher
    watch_debounce: Duration,
}

impl MediaServer {
    /// Create a new media server with the given configuration
    pub async fn new(config: ServerConfig) -> Result<Self> {
        let server_info = ServerInfo {
            friendly_name: config.friendly_name.clone(),
            udn: format!("uuid:{}", uuid::Uuid::new_v4()),
            manufacturer: config.manufacturer.clone(),
            model_name: config.model_name.clone(),
        };

        let mut ssdp = SsdpService::new(config.http_port).await?;
        ssdp.set_server_info(server_info.clone());

        let state = Arc::new(state::ServerState::new(server_info));

        let watch_enabled = config.watch_enabled;
        let watch_debounce = config.watch_debounce();

        Ok(Self {
            config,
            state,
            ssdp: Arc::new(ssdp),
            shutdown_tx: None,
            watcher: None,
            watch_enabled,
            watch_debounce,
        })
    }

    /// Start the media server
    pub async fn start(&mut self) -> Result<()> {
        info!(
            "Starting media server '{}' on port {}",
            self.config.friendly_name, self.config.http_port
        );

        // Scan configured directories
        for dir in &self.config.directories {
            if let Err(e) = self.scan_directory(dir) {
                warn!("Failed to scan directory {:?}: {}", dir, e);
            }
        }

        let (shutdown_tx, _) = broadcast::channel(1);
        self.shutdown_tx = Some(shutdown_tx.clone());

        // Bind HTTP listener FIRST to get actual port (important when port=0)
        let bind_ip = server_bind_ip(self.ssdp.local_ip());
        let listener =
            tokio::net::TcpListener::bind(SocketAddr::new(bind_ip, self.config.http_port)).await?;
        let actual_port = listener.local_addr()?.port();

        // Update SSDP with actual port if it was auto-selected
        if self.config.http_port == 0 {
            if let Some(ssdp) = Arc::get_mut(&mut self.ssdp) {
                ssdp.set_http_port(actual_port);
                info!("HTTP server bound to auto-selected port {}", actual_port);
            } else {
                warn!("Could not update SSDP port - Arc already shared");
            }
        }

        // Start SSDP announcer (now with correct port)
        self.ssdp.start_announcer()?;

        // Start HTTP server with pre-bound listener
        let state = self.state.clone();
        let ssdp = self.ssdp.clone();
        let shutdown_rx = shutdown_tx.subscribe();

        tokio::spawn(async move {
            if let Err(e) = http::run_server_with_listener(state, ssdp, listener, shutdown_rx).await
            {
                tracing::error!("HTTP server error: {}", e);
            }
        });

        // Start file watcher if enabled
        if self.watch_enabled {
            self.start_watcher()?;
        }

        info!("Media server started successfully");
        Ok(())
    }

    /// Enable or disable file watching dynamically
    pub fn set_watch_enabled(&mut self, enabled: bool) -> Result<()> {
        if enabled == self.watch_enabled {
            return Ok(()); // No change needed
        }

        self.watch_enabled = enabled;

        if enabled {
            // Start watcher if not already running
            if self.watcher.is_none() {
                self.start_watcher()?;
            }
        } else {
            // Stop watcher
            self.stop_watcher();
        }

        Ok(())
    }

    /// Set the debounce duration for file watching
    pub fn set_watch_debounce(&mut self, duration: Duration) {
        self.watch_debounce = duration;
    }

    /// Stop the file watcher
    pub fn stop_watcher(&mut self) {
        if self.watcher.take().is_some() {
            info!("File watcher stopped");
        }
    }

    /// Start the file watcher
    pub fn start_watcher(&mut self) -> Result<()> {
        use watcher::create_watcher_with_debounce;

        let (mut watcher, processor) = create_watcher_with_debounce(self.watch_debounce)?;

        // Watch all configured directories
        for dir in &self.config.directories {
            if let Err(e) = watcher.watch(dir) {
                warn!("Failed to watch directory {:?}: {}", dir, e);
            }
        }

        // Spawn the event processor task
        let state = self.state.clone();
        tokio::spawn(async move {
            processor
                .run(move |event| {
                    handle_watch_event(&state, event);
                })
                .await;
        });

        self.watcher = Some(watcher);
        info!(
            "File watcher started with {:?} debounce",
            self.watch_debounce
        );
        Ok(())
    }

    /// Stop the media server gracefully (sends SSDP byebye)
    pub async fn stop(&mut self) {
        info!("Stopping media server");

        // Send SSDP byebye to notify network of shutdown
        if let Err(e) = self.ssdp.send_byebye().await {
            warn!("Failed to send SSDP byebye: {}", e);
        }

        // Stop SSDP announcer
        self.ssdp.stop();

        // Stop HTTP server
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }

        // Small delay to let connections drain
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        info!("Media server stopped");
    }

    /// Update the server's friendly name without restarting
    ///
    /// This sends SSDP byebye with the old name, updates the name, and sends alive with the new name.
    pub async fn set_server_name(&self, new_name: String) -> Result<()> {
        // Update SSDP (sends byebye/alive)
        self.ssdp.update_server_name(new_name.clone()).await?;
        // Update ServerState (for description.xml, etc.)
        self.state.set_server_name(new_name);
        Ok(())
    }

    /// Scan a directory and add media files
    fn scan_directory(&self, path: &Path) -> Result<usize> {
        let files = scanner::scan_directory(path)?;
        let count = files.len();
        self.state.add_files(files);
        info!("Added {} files from {:?}", count, path);
        Ok(count)
    }

    /// Add a directory to scan
    pub fn add_directory(&mut self, path: PathBuf) -> Result<usize> {
        if self.config.directories.iter().any(|dir| dir == &path) {
            info!("Directory already configured: {:?}", path);
            return Ok(0);
        }

        self.config.directories.push(path.clone());
        if self.watch_enabled {
            if let Some(watcher) = self.watcher.as_mut() {
                if let Err(e) = watcher.watch(&path) {
                    warn!("Failed to watch directory {:?}: {}", path, e);
                }
            } else if let Err(e) = self.start_watcher() {
                warn!("Failed to start watcher after adding {:?}: {}", path, e);
            }
        }

        self.scan_directory(&path)
    }

    /// Add a directory with progress callback
    pub fn add_directory_with_progress<F>(
        &mut self,
        path: PathBuf,
        progress_callback: F,
    ) -> Result<usize>
    where
        F: Fn(&scanner::ScanProgress) + Send + Sync,
    {
        if self.config.directories.iter().any(|dir| dir == &path) {
            info!("Directory already configured: {:?}", path);
            return Ok(0);
        }

        self.config.directories.push(path.clone());
        if self.watch_enabled {
            if let Some(watcher) = self.watcher.as_mut() {
                if let Err(e) = watcher.watch(&path) {
                    warn!("Failed to watch directory {:?}: {}", path, e);
                }
            } else if let Err(e) = self.start_watcher() {
                warn!("Failed to start watcher after adding {:?}: {}", path, e);
            }
        }

        let files = scanner::scan_directory_with_progress(&path, progress_callback)?;
        let count = files.len();
        self.state.add_files(files);
        info!("Added {} files from {:?} with progress", count, path);
        Ok(count)
    }

    /// Replace all media directories (clears existing files and scans new directories)
    pub fn set_directories(&mut self, paths: Vec<PathBuf>) -> Result<usize> {
        if self.watcher.is_some() {
            self.stop_watcher();
        }

        let mut unique_paths: Vec<PathBuf> = Vec::new();
        for path in paths {
            if !unique_paths.iter().any(|dir| dir == &path) {
                unique_paths.push(path);
            }
        }

        self.config.directories = unique_paths;
        self.clear_files();

        let mut total = 0;
        for dir in &self.config.directories {
            total += self.scan_directory(dir)?;
        }

        if self.watch_enabled && !self.config.directories.is_empty() {
            self.start_watcher()?;
        }

        Ok(total)
    }

    /// Remove a directory from the media library
    pub fn remove_directory(&mut self, path: &Path) -> Result<()> {
        self.config.directories.retain(|dir| dir != path);
        self.state.remove_files_from_directory(path);

        if let Some(watcher) = self.watcher.as_mut() {
            if watcher.watched_paths().iter().any(|dir| dir == path) {
                if let Err(e) = watcher.unwatch(path) {
                    warn!("Failed to unwatch directory {:?}: {}", path, e);
                }
            }
        }

        if self.config.directories.is_empty() {
            self.stop_watcher();
        }

        Ok(())
    }

    /// Clear all files
    pub fn clear_files(&self) {
        self.state.clear_files();
    }

    /// Get all media files
    pub fn files(&self) -> Vec<MediaFile> {
        self.state.files()
    }

    /// Add pre-scanned files to the server (for lock-free scanning)
    ///
    /// This method is used when files have been scanned externally (without holding locks)
    /// and need to be added to the server quickly.
    pub fn add_scanned_files(&mut self, dir_path: PathBuf, files: Vec<MediaFile>) {
        // Add directory to config if not already there
        if !self.config.directories.iter().any(|d| d == &dir_path) {
            self.config.directories.push(dir_path.clone());
        }

        // Add files to state
        let count = files.len();
        self.state.add_files(files);
        info!("Added {} pre-scanned files from {:?}", count, dir_path);

        // Set up file watcher for the directory
        if self.watch_enabled {
            if let Some(watcher) = self.watcher.as_mut() {
                if let Err(e) = watcher.watch(&dir_path) {
                    warn!("Failed to watch directory {:?}: {}", dir_path, e);
                }
            } else if let Err(e) = self.start_watcher() {
                warn!("Failed to start watcher after adding {:?}: {}", dir_path, e);
            }
        }
    }

    /// Add a single media file by path (for ad-hoc rescans)
    pub fn add_file(&self, path: PathBuf) -> Result<Option<MediaFile>> {
        if !path.exists() || !path.is_file() {
            return Ok(None);
        }

        if let Some(file) = scanner::scan_file(&path) {
            let clone = file.clone();
            self.state.add_file(file);
            Ok(Some(clone))
        } else {
            Ok(None)
        }
    }

    /// Get a file by ID
    pub fn get_file(&self, id: &str) -> Option<MediaFile> {
        self.state.get_file(id)
    }

    /// Get the server's base URL
    pub fn base_url(&self) -> String {
        self.ssdp.base_url()
    }

    /// Get the server's UDN
    pub fn udn(&self) -> String {
        self.state.server_info().udn.clone()
    }

    /// Check if file watching is enabled
    pub fn watch_enabled(&self) -> bool {
        self.watch_enabled
    }

    // === Folder Visibility ===

    /// Set public folders (visible to all devices)
    pub fn set_public_folders(&self, folders: Vec<PathBuf>) {
        self.state.set_public_folders(folders);
    }

    /// Set folders for a specific device (by IP address)
    pub fn set_device_folders(&self, device_ip: &str, folders: Vec<PathBuf>) {
        self.state.set_device_folders(device_ip, folders);
    }

    /// Clear all device folder assignments
    pub fn clear_device_folders(&self) {
        self.state.clear_device_folders();
    }
}

fn server_bind_ip(default_ip: IpAddr) -> IpAddr {
    match std::env::var("FLINGDLNA_SERVER_BIND").ok().as_deref() {
        Some(v) if !v.trim().is_empty() => v.trim().parse().unwrap_or(default_ip),
        _ => default_ip,
    }
}

/// Handle a file system watch event
fn handle_watch_event(state: &Arc<state::ServerState>, event: WatchEvent) {
    match event {
        WatchEvent::Created(path) => {
            debug!("File created: {:?}", path);
            if let Some(file) = scanner::scan_file(&path) {
                info!("Added new file: {}", file.filename);
                state.add_file(file);
            }
        }
        WatchEvent::Modified(path) => {
            debug!("File modified: {:?}", path);
            // Remove old entry and add updated one
            state.remove_file_by_path(&path);
            if let Some(file) = scanner::scan_file(&path) {
                info!("Updated file: {}", file.filename);
                state.add_file(file);
            }
        }
        WatchEvent::Deleted(path) => {
            debug!("File deleted: {:?}", path);
            if let Some(removed) = state.remove_file_by_path(&path) {
                info!("Removed file: {}", removed.filename);
            }
        }
        WatchEvent::Renamed(from, to) => {
            debug!("File renamed: {:?} -> {:?}", from, to);
            // Remove old entry
            state.remove_file_by_path(&from);
            // Add new entry if it's still a media file
            if let Some(file) = scanner::scan_file(&to) {
                info!("Renamed file: {} -> {}", from.display(), file.filename);
                state.add_file(file);
            }
        }
    }
}
