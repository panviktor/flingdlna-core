//! Daemon server - Unix socket IPC for persistent DLNA operations
//!
//! The daemon maintains state between CLI invocations:
//! - Active streaming servers
//! - Device cache
//! - Server instance (if enabled)
//!
//! Usage:
//!   flingdlna daemon              # start daemon
//!   echo '{"cmd":"list"}' | nc -U ~/Library/Application\ Support/FlingDLNA/flingdlna.sock

mod cache;
#[cfg(feature = "chromecast")]
mod chromecast;
mod client;
mod commands;
mod config;
mod peer;

pub use cache::{new_chromecast_cache, ActiveStream, CachedRenderer, ChromecastCache};
pub use client::{is_daemon_running, DaemonClient};
pub use config::{default_socket_path, DaemonConfig};

use crate::database::{self, Database};
use crate::protocol::{Command, Notification, RendererInfo, Response};
use crate::queue::Queue;
use crate::{ControllerConfig, DlnaCombo};
use dlna_controller::{EventManager, UpnpEvent};
use dlna_core::config::ComboConfig;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info};

#[cfg(feature = "chromecast")]
use chromecast::do_chromecast_discovery;

/// The daemon server
pub struct Daemon {
    config: DaemonConfig,
    combo: Arc<RwLock<DlnaCombo>>,
    streams: Arc<RwLock<HashMap<String, ActiveStream>>>,
    /// Playback queues per device
    queues: Arc<RwLock<HashMap<String, Queue>>>,
    /// Cached renderers (name -> renderer)
    renderer_cache: Arc<RwLock<HashMap<String, CachedRenderer>>>,
    next_port: AtomicU16,
    started_at: Instant,
    /// Shutdown signal sender (public for external signal handling)
    pub shutdown_tx: broadcast::Sender<()>,
    running: AtomicBool,
    /// UPnP event manager (optional - only created if eventing is enabled)
    event_manager: Option<Arc<EventManager>>,
    /// Local IP address for callback URLs
    local_ip: Option<String>,
    /// Pending events buffer (max 100 events)
    event_buffer: Arc<RwLock<VecDeque<Notification>>>,
    /// UDN to friendly name mapping (for event translation)
    udn_to_name: Arc<RwLock<HashMap<String, String>>>,
    /// Chromecast device cache
    #[cfg_attr(not(feature = "chromecast"), allow(dead_code))]
    chromecast_cache: ChromecastCache,
    /// SQLite database for positions and history
    db: Arc<Database>,
}

impl Daemon {
    /// Create a new daemon instance
    pub async fn new(config: DaemonConfig) -> dlna_core::Result<Self> {
        let base_port = config.base_streaming_port;

        let combo_config = ComboConfig {
            server: config.server_config.clone(),
            controller: ControllerConfig::new()
                .with_discovery_timeout(config.discovery_timeout)
                .with_streaming_port(base_port),
        };

        let combo = DlnaCombo::new(combo_config).await?;
        let (shutdown_tx, _) = broadcast::channel(1);

        // Get local IP for callbacks
        let local_ip = get_local_ip();
        let bind_ip = local_ip
            .as_deref()
            .and_then(|s| s.parse::<std::net::IpAddr>().ok());

        // Try to create event manager (non-fatal if it fails)
        let event_manager = match bind_ip {
            Some(ip) => match EventManager::new_bound(ip, config.event_callback_port).await {
                Ok(em) => {
                    info!(
                        "UPnP event manager started on {}:{}",
                        ip, config.event_callback_port
                    );
                    Some(Arc::new(em))
                }
                Err(e) => {
                    info!(
                        "Could not start event manager on {}:{}: {} (events disabled)",
                        ip, config.event_callback_port, e
                    );
                    None
                }
            },
            None => match EventManager::new(config.event_callback_port).await {
                Ok(em) => {
                    info!(
                        "UPnP event manager started on port {}",
                        config.event_callback_port
                    );
                    Some(Arc::new(em))
                }
                Err(e) => {
                    info!("Could not start event manager: {} (events disabled)", e);
                    None
                }
            },
        };

        // Event buffer for pending events
        let event_buffer: Arc<RwLock<VecDeque<Notification>>> =
            Arc::new(RwLock::new(VecDeque::new()));
        let udn_to_name: Arc<RwLock<HashMap<String, String>>> =
            Arc::new(RwLock::new(HashMap::new()));

        // Chromecast cache
        let chromecast_cache = new_chromecast_cache();

        // Open SQLite database for positions and history
        let db = Database::open()
            .map_err(|e| dlna_core::Error::Config(format!("Failed to open database: {e}")))?;
        info!("Database opened at {:?}", database::db_path());

        Ok(Self {
            config,
            combo: Arc::new(RwLock::new(combo)),
            streams: Arc::new(RwLock::new(HashMap::new())),
            queues: Arc::new(RwLock::new(HashMap::new())),
            renderer_cache: Arc::new(RwLock::new(HashMap::new())),
            next_port: AtomicU16::new(base_port),
            started_at: Instant::now(),
            shutdown_tx,
            running: AtomicBool::new(false),
            event_manager,
            local_ip,
            event_buffer,
            udn_to_name,
            chromecast_cache,
            db: Arc::new(db),
        })
    }

    /// Run the daemon (blocks until shutdown)
    pub async fn run(&self) -> dlna_core::Result<()> {
        // Remove existing socket file if present
        if self.config.socket_path.exists() {
            std::fs::remove_file(&self.config.socket_path)?;
        }

        // Create parent directory if needed
        if let Some(parent) = self.config.socket_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let listener = UnixListener::bind(&self.config.socket_path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(
                &self.config.socket_path,
                std::fs::Permissions::from_mode(0o600),
            );
        }
        info!("Daemon listening on {:?}", self.config.socket_path);

        // Start server if configured
        {
            let mut combo = self.combo.write().await;
            if combo.server().is_some() {
                combo.start_server().await?;
                info!("Media server started");
            }
        }

        self.running.store(true, Ordering::SeqCst);
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        // Start background discovery task
        self.spawn_discovery_task();

        // Start event consumer task (if eventing is enabled)
        self.spawn_event_consumer_task();

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, _addr)) => {
                            self.handle_new_connection(stream).await;
                        }
                        Err(e) => {
                            error!("Accept error: {}", e);
                        }
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!("Shutdown signal received");
                    break;
                }
            }
        }

        self.running.store(false, Ordering::SeqCst);

        // Cleanup
        {
            let mut combo = self.combo.write().await;
            combo.stop_server().await;
        }

        // Remove socket file
        let _ = std::fs::remove_file(&self.config.socket_path);

        info!("Daemon stopped");
        Ok(())
    }

    /// Spawn background discovery task
    fn spawn_discovery_task(&self) {
        let cache = Arc::clone(&self.renderer_cache);
        let combo = Arc::clone(&self.combo);
        let mut shutdown_for_discovery = self.shutdown_tx.subscribe();
        #[cfg(feature = "chromecast")]
        let cc_cache = Arc::clone(&self.chromecast_cache);

        tokio::spawn(async move {
            // Initial discovery on startup
            do_background_discovery(&combo, &cache).await;
            #[cfg(feature = "chromecast")]
            do_chromecast_discovery(&cc_cache).await;

            let mut interval = tokio::time::interval(Duration::from_secs(45));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        do_background_discovery(&combo, &cache).await;
                        #[cfg(feature = "chromecast")]
                        do_chromecast_discovery(&cc_cache).await;
                    }
                    _ = shutdown_for_discovery.recv() => break,
                }
            }
            debug!("Background discovery task stopped");
        });
    }

    /// Spawn event consumer task (if eventing is enabled)
    fn spawn_event_consumer_task(&self) {
        if let Some(ref em) = self.event_manager {
            let event_buffer = Arc::clone(&self.event_buffer);
            let udn_to_name = Arc::clone(&self.udn_to_name);
            let mut event_rx = em.subscribe_events();
            let mut shutdown_for_events = self.shutdown_tx.subscribe();

            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        result = event_rx.recv() => {
                            match result {
                                Ok(event) => {
                                    handle_upnp_event(event, &event_buffer, &udn_to_name).await;
                                }
                                Err(broadcast::error::RecvError::Lagged(n)) => {
                                    debug!("Event consumer lagged by {} events", n);
                                }
                                Err(broadcast::error::RecvError::Closed) => {
                                    debug!("Event channel closed");
                                    break;
                                }
                            }
                        }
                        _ = shutdown_for_events.recv() => break,
                    }
                }
                debug!("Event consumer task stopped");
            });
        }
    }

    /// Handle a new connection
    async fn handle_new_connection(&self, stream: tokio::net::UnixStream) {
        let combo = self.combo.clone();
        let streams = self.streams.clone();
        let shutdown_tx = self.shutdown_tx.clone();
        let started_at = self.started_at;
        let next_port = &self.next_port;
        let base_port = self.config.base_streaming_port;
        let max_streams = self.config.max_streams;

        // Allocate port for potential streaming
        let port = next_port.fetch_add(1, Ordering::SeqCst);
        if port >= base_port + max_streams as u16 {
            next_port.store(base_port, Ordering::SeqCst);
        }

        let queues = Arc::clone(&self.queues);
        let renderer_cache = Arc::clone(&self.renderer_cache);
        let event_manager = self.event_manager.clone();
        let local_ip = self.local_ip.clone();
        let event_buffer = Arc::clone(&self.event_buffer);
        let udn_to_name = Arc::clone(&self.udn_to_name);
        #[cfg(feature = "chromecast")]
        let chromecast_cache = Arc::clone(&self.chromecast_cache);
        #[cfg(not(feature = "chromecast"))]
        let chromecast_cache = ();
        let db = Arc::clone(&self.db);

        tokio::spawn(async move {
            if let Err(e) = handle_connection(
                stream,
                combo,
                streams,
                queues,
                renderer_cache,
                shutdown_tx,
                started_at,
                port,
                event_manager,
                local_ip,
                event_buffer,
                udn_to_name,
                chromecast_cache,
                db,
            )
            .await
            {
                // Broken pipe is normal when client disconnects - don't log as error
                let err_str = e.to_string();
                if err_str.contains("Broken pipe") || err_str.contains("Connection reset") {
                    debug!("Client disconnected: {}", e);
                } else {
                    error!("Connection handler error: {}", e);
                }
            }
        });
    }

    /// Check if daemon is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Request shutdown
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }
}

/// Background discovery task - updates renderer cache periodically
async fn do_background_discovery(
    combo: &Arc<RwLock<DlnaCombo>>,
    cache: &Arc<RwLock<HashMap<String, CachedRenderer>>>,
) {
    let combo = combo.read().await;
    match combo.discover_renderers().await {
        Ok(renderers) => {
            let mut cache = cache.write().await;
            let now = Instant::now();
            for r in &renderers {
                let info: RendererInfo = r.into();
                cache.insert(
                    info.name.clone(),
                    CachedRenderer {
                        info,
                        cached_at: now,
                    },
                );
            }
            debug!("Background discovery: {} renderer(s)", renderers.len());
        }
        Err(e) => {
            debug!("Background discovery failed: {}", e);
        }
    }
}

/// Handle a UPnP event and add it to the buffer
async fn handle_upnp_event(
    event: UpnpEvent,
    event_buffer: &Arc<RwLock<VecDeque<Notification>>>,
    udn_to_name: &Arc<RwLock<HashMap<String, String>>>,
) {
    // Convert UpnpEvent to Notification
    let notification = match event {
        UpnpEvent::VolumeChanged { device_udn, volume } => {
            let device = udn_to_name
                .read()
                .await
                .get(&device_udn)
                .cloned()
                .unwrap_or_else(|| device_udn.clone());
            Notification::VolumeChanged { device, volume }
        }
        UpnpEvent::MuteChanged { device_udn, muted } => {
            let device = udn_to_name
                .read()
                .await
                .get(&device_udn)
                .cloned()
                .unwrap_or_else(|| device_udn.clone());
            Notification::MuteChanged { device, muted }
        }
        UpnpEvent::TransportStateChanged { device_udn, state } => {
            let device = udn_to_name
                .read()
                .await
                .get(&device_udn)
                .cloned()
                .unwrap_or_else(|| device_udn.clone());
            Notification::TransportStateChanged { device, state }
        }
        UpnpEvent::CurrentUriChanged { device_udn, uri } => {
            let device = udn_to_name
                .read()
                .await
                .get(&device_udn)
                .cloned()
                .unwrap_or_else(|| device_udn.clone());
            Notification::CurrentUriChanged { device, uri }
        }
        UpnpEvent::SubscriptionLost {
            device_udn,
            service,
        } => {
            let device = udn_to_name
                .read()
                .await
                .get(&device_udn)
                .cloned()
                .unwrap_or_else(|| device_udn.clone());
            Notification::SubscriptionLost { device, service }
        }
        // Position, Duration, and PlayMode events are not exposed via daemon protocol
        // They can be polled via get_status command instead
        UpnpEvent::PositionChanged { .. }
        | UpnpEvent::DurationChanged { .. }
        | UpnpEvent::PlayModeChanged { .. } => {
            // Skip - not converted to daemon notifications
            return;
        }
    };

    // Add to buffer (max 100 events)
    let mut buffer = event_buffer.write().await;
    if buffer.len() >= 100 {
        buffer.pop_front();
    }
    buffer.push_back(notification);
    debug!("Event buffered, total: {}", buffer.len());
}

#[allow(clippy::too_many_arguments)]
async fn handle_connection(
    mut stream: tokio::net::UnixStream,
    combo: Arc<RwLock<DlnaCombo>>,
    streams: Arc<RwLock<HashMap<String, ActiveStream>>>,
    queues: Arc<RwLock<HashMap<String, Queue>>>,
    renderer_cache: Arc<RwLock<HashMap<String, CachedRenderer>>>,
    shutdown_tx: broadcast::Sender<()>,
    started_at: Instant,
    allocated_port: u16,
    event_manager: Option<Arc<EventManager>>,
    local_ip: Option<String>,
    event_buffer: Arc<RwLock<VecDeque<Notification>>>,
    udn_to_name: Arc<RwLock<HashMap<String, String>>>,
    chromecast_cache: ChromecastCache,
    db: Arc<Database>,
) -> dlna_core::Result<()> {
    if !peer::peer_allowed(&stream) {
        let response = Response::error("Unauthorized");
        let response_json = serde_json::to_string(&response).unwrap_or_else(|e| {
            error!("Failed to serialize response: {}", e);
            "{\"status\":\"error\",\"message\":\"Unauthorized\"}".to_string()
        });
        stream.write_all(response_json.as_bytes()).await?;
        stream.write_all(b"\n").await?;
        stream.flush().await?;
        return Ok(());
    }

    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    // Read command (single line JSON)
    let line = read_line_limited(&mut reader, MAX_DAEMON_CMD_BYTES).await?;
    let line = line.trim();

    if line.is_empty() {
        return Ok(());
    }

    debug!("Received command: {}", line);

    let required_token = std::env::var("FLINGDLNA_DAEMON_TOKEN")
        .ok()
        .filter(|s| !s.is_empty());

    let mut value: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            let response = Response::error(format!("Invalid command: {e}"));
            let response_json = serde_json::to_string(&response).unwrap_or_else(|e| {
                error!("Failed to serialize response: {}", e);
                "{\"status\":\"error\",\"message\":\"Serialization error\"}".to_string()
            });
            writer.write_all(response_json.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
            return Ok(());
        }
    };

    if let Some(required) = required_token.as_deref() {
        let provided = value
            .as_object()
            .and_then(|o| o.get("token"))
            .and_then(|t| t.as_str());

        if provided != Some(required) {
            let response = Response::error("Unauthorized");
            let response_json = serde_json::to_string(&response).unwrap_or_else(|e| {
                error!("Failed to serialize response: {}", e);
                "{\"status\":\"error\",\"message\":\"Serialization error\"}".to_string()
            });
            writer.write_all(response_json.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
            return Ok(());
        }
    }

    // Remove token field before parsing Command
    if let Some(obj) = value.as_object_mut() {
        obj.remove("token");
    }

    // Parse command
    let response = match serde_json::from_value::<Command>(value) {
        Ok(cmd) => {
            commands::execute_command(
                cmd,
                &combo,
                &streams,
                &queues,
                &renderer_cache,
                &shutdown_tx,
                started_at,
                allocated_port,
                &event_manager,
                &local_ip,
                &event_buffer,
                &udn_to_name,
                &chromecast_cache,
                &db,
            )
            .await
        }
        Err(e) => Response::error(format!("Invalid command: {e}")),
    };

    // Send response
    let response_json = serde_json::to_string(&response).unwrap_or_else(|e| {
        error!("Failed to serialize response: {}", e);
        "{\"status\":\"error\",\"message\":\"Serialization error\"}".to_string()
    });

    writer.write_all(response_json.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;

    Ok(())
}

const MAX_DAEMON_CMD_BYTES: usize = 64 * 1024;

async fn read_line_limited<R>(reader: &mut R, max_len: usize) -> std::io::Result<String>
where
    R: tokio::io::AsyncBufRead + Unpin,
{
    let mut buf: Vec<u8> = Vec::new();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            break;
        }

        if let Some(pos) = available.iter().position(|&b| b == b'\n') {
            let take = pos + 1;
            if buf.len().saturating_add(take) > max_len {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Command too large",
                ));
            }
            buf.extend_from_slice(&available[..take]);
            reader.consume(take);
            break;
        }

        if buf.len().saturating_add(available.len()) > max_len {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Command too large",
            ));
        }
        let take = available.len();
        buf.extend_from_slice(available);
        reader.consume(take);
    }

    String::from_utf8(buf).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Command is not valid UTF-8",
        )
    })
}

/// Get local IP address for UPnP callbacks
fn get_local_ip() -> Option<String> {
    dlna_core::best_local_ipv4().ok().map(|ip| ip.to_string())
}
