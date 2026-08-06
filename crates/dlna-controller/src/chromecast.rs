//! Chromecast support - discovery and transport control
//!
//! Uses mDNS to discover Chromecast devices and rust_cast for control.

use dlna_core::{Error, Result};
use mdns_sd::{ServiceDaemon, ServiceEvent};
use rust_cast::channels::media::{
    GenericMediaMetadata, LoadOptions, Media, MediaQueue, Metadata, PlayerState, QueueItem,
    QueueType, StreamType,
};
use rust_cast::channels::receiver::CastDeviceApp;
use rust_cast::CastDevice;
use std::collections::HashMap;
use std::net::IpAddr;
use std::time::Duration;
use tracing::{debug, info};

/// mDNS service type for Chromecast
const CHROMECAST_SERVICE: &str = "_googlecast._tcp.local.";

/// Information about a discovered Chromecast device
#[derive(Debug, Clone)]
pub struct ChromecastDevice {
    pub name: String,
    pub host: IpAddr,
    pub port: u16,
    pub model: Option<String>,
    pub id: String,
    // Additional mDNS TXT record fields
    pub icon_path: Option<String>, // ic - Icon path (relative, e.g. "/setup/icon.png")
    pub version: Option<String>,   // ve - Firmware version
    pub capabilities: Option<String>, // ca - Device capabilities
    pub status: Option<String>,    // st - Device status
}

impl ChromecastDevice {
    /// Get address string for connection
    pub fn address(&self) -> String {
        self.host.to_string()
    }
}

/// Discover Chromecast devices on the network using mDNS
pub async fn discover(timeout: Duration) -> Result<Vec<ChromecastDevice>> {
    discover_impl(timeout, None).await
}

/// Discover Chromecast devices with progressive updates via channel
pub async fn discover_progressive(
    timeout: Duration,
    tx: tokio::sync::mpsc::Sender<ChromecastDevice>,
) -> Result<Vec<ChromecastDevice>> {
    discover_impl(timeout, Some(tx)).await
}

/// Internal implementation of Chromecast discovery
async fn discover_impl(
    timeout: Duration,
    progressive_tx: Option<tokio::sync::mpsc::Sender<ChromecastDevice>>,
) -> Result<Vec<ChromecastDevice>> {
    let mdns = ServiceDaemon::new().map_err(|e| Error::Network(e.to_string()))?;
    let receiver = mdns
        .browse(CHROMECAST_SERVICE)
        .map_err(|e| Error::Network(e.to_string()))?;

    let mut devices: HashMap<String, ChromecastDevice> = HashMap::new();
    let start = std::time::Instant::now();

    info!("Discovering Chromecast devices for {:?}...", timeout);

    // Process events until timeout
    while start.elapsed() < timeout {
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(event) => match event {
                ServiceEvent::ServiceResolved(info) => {
                    // Log raw mDNS service info for debugging
                    info!("Chromecast mDNS ServiceResolved:");
                    info!("  fullname: {}", info.get_fullname());
                    info!("  hostname: {}", info.get_hostname());
                    info!("  port: {}", info.get_port());
                    info!("  addresses: {:?}", info.get_addresses());

                    // Log all TXT record properties
                    info!("  TXT record properties:");
                    let properties = info.get_properties();
                    for prop in properties.iter() {
                        info!("    {} = {:?}", prop.key(), prop.val_str());
                    }

                    let name = info
                        .get_property_val_str("fn")
                        .unwrap_or_else(|| {
                            info.get_fullname().split('.').next().unwrap_or("Unknown")
                        })
                        .to_string();

                    let model = info.get_property_val_str("md").map(|s| s.to_string());
                    let id = info
                        .get_property_val_str("id")
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| info.get_fullname().to_string());

                    // Parse additional mDNS TXT record fields
                    let icon_path = info.get_property_val_str("ic").map(|s| s.to_string());
                    let version = info.get_property_val_str("ve").map(|s| s.to_string());
                    let capabilities = info.get_property_val_str("ca").map(|s| s.to_string());
                    let status = info.get_property_val_str("st").map(|s| s.to_string());

                    // Get the first IP address
                    if let Some(addr) = info.get_addresses().iter().next() {
                        let device = ChromecastDevice {
                            name: name.clone(),
                            host: *addr,
                            port: info.get_port(),
                            model: model.clone(),
                            id: id.clone(),
                            icon_path: icon_path.clone(),
                            version: version.clone(),
                            capabilities: capabilities.clone(),
                            status: status.clone(),
                        };

                        // Log all parsed fields for debugging
                        info!("Found Chromecast: {} at {}:{}", name, addr, info.get_port());
                        if let Some(ref m) = model {
                            info!("  Model: {}", m);
                        }
                        if let Some(ref ver) = version {
                            info!("  Version: {}", ver);
                        }
                        if let Some(ref cap) = capabilities {
                            info!("  Capabilities: {}", cap);
                        }
                        if let Some(ref st) = status {
                            info!("  Status: {}", st);
                        }
                        if let Some(ref ic) = icon_path {
                            info!("  Icon: http://{}:8008{}", addr, ic);
                        } else {
                            info!("  Icon: NONE (ic field not present in mDNS TXT record)");
                        }

                        // Send to progressive channel if provided (only if new device)
                        if !devices.contains_key(&id) {
                            if let Some(ref tx) = progressive_tx {
                                let _ = tx.send(device.clone()).await;
                            }
                        }

                        devices.insert(id, device);
                    }
                }
                ServiceEvent::ServiceRemoved(_, fullname) => {
                    debug!("Chromecast removed: {}", fullname);
                }
                _ => {}
            },
            Err(flume::RecvTimeoutError::Timeout) => continue,
            Err(flume::RecvTimeoutError::Disconnected) => break,
        }
    }

    // Stop browsing
    let _ = mdns.stop_browse(CHROMECAST_SERVICE);

    let result: Vec<ChromecastDevice> = devices.into_values().collect();
    info!("Found {} Chromecast device(s)", result.len());
    Ok(result)
}

/// Playback state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayState {
    Playing,
    Paused,
    Buffering,
    Idle,
    Unknown,
}

impl From<PlayerState> for PlayState {
    fn from(state: PlayerState) -> Self {
        match state {
            PlayerState::Idle => PlayState::Idle,
            PlayerState::Playing => PlayState::Playing,
            PlayerState::Paused => PlayState::Paused,
            PlayerState::Buffering => PlayState::Buffering,
        }
    }
}

/// Chromecast playback status
#[derive(Debug, Clone)]
pub struct ChromecastStatus {
    pub state: PlayState,
    pub position_secs: f32,
    pub duration_secs: Option<f32>,
    pub volume: f32,
    pub muted: bool,
}

/// Connect to Chromecast and perform operation
/// On connection failure, tries Wake-on-LAN and retries
fn connect(device: &ChromecastDevice) -> Result<CastDevice<'static>> {
    // First attempt
    match CastDevice::connect_without_host_verification(device.address(), device.port) {
        Ok(conn) => {
            // Some devices require an explicit CONNECT to receiver-0 before other messages.
            if let Err(e) = conn.connection.connect("receiver-0") {
                return Err(Error::Network(format!(
                    "Failed to connect to Chromecast receiver: {e}"
                )));
            }
            Ok(conn)
        }
        Err(e) => {
            let err_str = e.to_string();
            if err_str.contains("timed out") || err_str.contains("Connection refused") {
                info!(
                    "Connection to {} failed, trying Wake-on-LAN...",
                    device.name
                );

                // Try to wake the device
                if crate::wol::wake_by_ip(device.host) {
                    info!(
                        "WoL sent to {}, waiting 5 seconds for device to wake...",
                        device.name
                    );
                    std::thread::sleep(std::time::Duration::from_secs(5));

                    // Retry connection
                    info!("Retrying connection to {}...", device.name);
                    match CastDevice::connect_without_host_verification(
                        device.address(),
                        device.port,
                    ) {
                        Ok(conn) => {
                            if let Err(e2) = conn.connection.connect("receiver-0") {
                                return Err(Error::Network(format!(
                                    "Failed to connect to Chromecast receiver after WoL: {e2}"
                                )));
                            }
                            info!("Successfully connected to {} after WoL", device.name);
                            Ok(conn)
                        }
                        Err(e2) => {
                            Err(Error::Network(format!(
                                "Cannot connect to {} even after Wake-on-LAN. Device may need to be turned on manually. Error: {}",
                                device.name, e2
                            )))
                        }
                    }
                } else {
                    Err(Error::Network(format!(
                        "Cannot connect to {} - device may be in standby. Could not send Wake-on-LAN (MAC not found in ARP table)",
                        device.name
                    )))
                }
            } else {
                Err(Error::Network(format!(
                    "Failed to connect to {}: {}",
                    device.name, e
                )))
            }
        }
    }
}

/// Load and play a URL on a Chromecast
pub fn load_url(
    device: &ChromecastDevice,
    url: &str,
    content_type: Option<&str>,
    title: Option<&str>,
) -> Result<()> {
    load_url_at_position(device, url, content_type, None, title)
}

/// Load and play a URL on a Chromecast, optionally starting at a specific position
pub fn load_url_at_position(
    device: &ChromecastDevice,
    url: &str,
    content_type: Option<&str>,
    start_position_secs: Option<f64>,
    title: Option<&str>,
) -> Result<()> {
    if let Some(pos) = start_position_secs {
        info!(
            "Loading URL on Chromecast {} at {}s: {}",
            device.name, pos, url
        );
    } else {
        info!("Loading URL on Chromecast {}: {}", device.name, url);
    }

    debug!(
        "Chromecast: connecting to {}:{}...",
        device.host, device.port
    );
    let conn = connect(device)?;
    debug!("Chromecast: connected successfully");

    // Launch the Default Media Receiver
    debug!("Chromecast: launching Default Media Receiver...");
    let app = conn
        .receiver
        .launch_app(&CastDeviceApp::DefaultMediaReceiver)
        .map_err(|e| Error::Upnp(format!("Failed to launch receiver: {e}")))?;

    info!(
        "Chromecast: launched app, transport={}, session={}",
        app.transport_id, app.session_id
    );

    // Open connection to the launched app transport before media commands.
    conn.connection
        .connect(&app.transport_id)
        .map_err(|e| Error::Upnp(format!("Failed to connect to app transport: {e}")))?;

    let metadata = title.map(|title| {
        Metadata::Generic(GenericMediaMetadata {
            title: Some(title.to_string()),
            ..Default::default()
        })
    });

    let media = Media {
        content_id: url.to_string(),
        content_type: content_type.unwrap_or("video/mp4").to_string(),
        stream_type: StreamType::Buffered,
        duration: None,
        metadata,
    };

    let options = LoadOptions {
        current_time: start_position_secs.unwrap_or(0.0),
        autoplay: true,
    };

    debug!("Chromecast: loading media...");
    conn.media
        .load_with_opts(&app.transport_id, &app.session_id, &media, options)
        .map_err(|e| Error::Upnp(format!("Failed to load media: {e}")))?;

    if let Ok(status) = conn.media.get_status(&app.transport_id, None::<i32>) {
        if let Some(entry) = status.entries.first() {
            info!(
                "Chromecast: status after load: state={:?} idle_reason={:?} pos={:.1} dur={:?}",
                entry.player_state,
                entry.idle_reason,
                entry.current_time.unwrap_or(0.0),
                entry.media.as_ref().and_then(|m| m.duration)
            );
        } else {
            info!("Chromecast: status after load: no entries");
        }
    } else {
        info!("Chromecast: status after load: unavailable");
    }

    info!("Chromecast: media loaded successfully");
    Ok(())
}

fn guess_mime_from_url(url: &str) -> Option<String> {
    let path = url::Url::parse(url)
        .ok()
        .map(|parsed| parsed.path().to_string())
        .filter(|parsed_path| !parsed_path.is_empty())
        .unwrap_or_else(|| url.to_string());

    mime_guess::from_path(path).first().map(|m| m.to_string())
}

fn title_from_request(request: &crate::session::PlayRequest) -> String {
    if let Some(title) = request.title.as_deref().filter(|t| !t.is_empty()) {
        return title.to_string();
    }
    request
        .url
        .rsplit('/')
        .next()
        .unwrap_or("Media")
        .split('?')
        .next()
        .unwrap_or("Media")
        .to_string()
}

fn content_type_from_request(request: &crate::session::PlayRequest) -> String {
    request
        .content_type
        .clone()
        .or_else(|| guess_mime_from_url(&request.url))
        .unwrap_or_else(|| "video/mp4".to_string())
}

/// Load and play a media queue on a Chromecast
pub fn load_queue(
    device: &ChromecastDevice,
    items: &[crate::session::PlayRequest],
    start_index: usize,
    repeat_mode: Option<&str>,
) -> Result<()> {
    if items.is_empty() {
        return Err(Error::Upnp("Queue is empty".to_string()));
    }

    let clamped_start = start_index.min(items.len().saturating_sub(1)) as u16;
    info!(
        "Loading queue on Chromecast {} ({} items, start={})",
        device.name,
        items.len(),
        clamped_start
    );

    let conn = connect(device)?;

    let app = conn
        .receiver
        .launch_app(&CastDeviceApp::DefaultMediaReceiver)
        .map_err(|e| Error::Upnp(format!("Failed to launch receiver: {e}")))?;

    conn.connection
        .connect(&app.transport_id)
        .map_err(|e| Error::Upnp(format!("Failed to connect to app transport: {e}")))?;

    let queue_items: Vec<QueueItem> = items
        .iter()
        .map(|request| {
            let metadata = Metadata::Generic(GenericMediaMetadata {
                title: Some(title_from_request(request)),
                ..Default::default()
            });

            let media = Media {
                content_id: request.url.clone(),
                content_type: content_type_from_request(request),
                stream_type: StreamType::Buffered,
                duration: None,
                metadata: Some(metadata),
            };

            QueueItem { media }
        })
        .collect();

    let queue = MediaQueue {
        items: queue_items,
        start_index: clamped_start,
        queue_type: QueueType::VideoPlaylist,
        repeat_mode: repeat_mode.map(str::to_string),
    };

    conn.media
        .load_queue(&app.transport_id, &app.session_id, &queue)
        .map_err(|e| Error::Upnp(format!("Failed to load queue: {e}")))?;

    info!("Chromecast: queue loaded successfully");
    Ok(())
}

/// Play (resume) playback on a Chromecast
pub fn play(device: &ChromecastDevice) -> Result<()> {
    let conn = connect(device)?;
    let (transport_id, media_session_id) = get_active_session(&conn)?;

    conn.media
        .play(&transport_id, media_session_id)
        .map_err(|e| Error::Upnp(format!("Failed to play: {e}")))?;
    Ok(())
}

/// Pause playback on a Chromecast
pub fn pause(device: &ChromecastDevice) -> Result<()> {
    let conn = connect(device)?;
    let (transport_id, media_session_id) = get_active_session(&conn)?;

    conn.media
        .pause(&transport_id, media_session_id)
        .map_err(|e| Error::Upnp(format!("Failed to pause: {e}")))?;
    Ok(())
}

/// Stop playback on a Chromecast
pub fn stop(device: &ChromecastDevice) -> Result<()> {
    let conn = connect(device)?;
    let (transport_id, media_session_id) = get_active_session(&conn)?;

    conn.media
        .stop(&transport_id, media_session_id)
        .map_err(|e| Error::Upnp(format!("Failed to stop: {e}")))?;
    Ok(())
}

/// Seek to position (in seconds) on a Chromecast
pub fn seek(device: &ChromecastDevice, position_secs: f32) -> Result<()> {
    let conn = connect(device)?;
    let (transport_id, media_session_id) = get_active_session(&conn)?;

    conn.media
        .seek(&transport_id, media_session_id, Some(position_secs), None)
        .map_err(|e| Error::Upnp(format!("Failed to seek: {e}")))?;
    Ok(())
}

/// Get playback status from a Chromecast
pub fn get_status(device: &ChromecastDevice) -> Result<ChromecastStatus> {
    let conn = connect(device)?;

    // Get volume from receiver
    let receiver_status = conn
        .receiver
        .get_status()
        .map_err(|e| Error::Upnp(format!("Failed to get receiver status: {e}")))?;

    let volume = receiver_status.volume.level.unwrap_or(1.0);
    let muted = receiver_status.volume.muted.unwrap_or(false);

    // Try to get media status if there's an active app
    if let Some(app) = receiver_status.applications.first() {
        // Connect to the transport
        conn.connection
            .connect(&app.transport_id)
            .map_err(|e| Error::Upnp(format!("Failed to connect to transport: {e}")))?;

        if let Ok(status) = conn.media.get_status(&app.transport_id, None::<i32>) {
            if let Some(entry) = status.entries.first() {
                return Ok(ChromecastStatus {
                    state: entry.player_state.into(),
                    position_secs: entry.current_time.unwrap_or(0.0),
                    duration_secs: entry.media.as_ref().and_then(|m| m.duration),
                    volume,
                    muted,
                });
            }
        }
    }

    // No active media
    Ok(ChromecastStatus {
        state: PlayState::Idle,
        position_secs: 0.0,
        duration_secs: None,
        volume,
        muted,
    })
}

/// Set volume (0.0 - 1.0) on a Chromecast
pub fn set_volume(device: &ChromecastDevice, level: f32) -> Result<()> {
    let conn = connect(device)?;
    conn.receiver
        .set_volume(level.clamp(0.0, 1.0))
        .map_err(|e| Error::Upnp(format!("Failed to set volume: {e}")))?;
    Ok(())
}

/// Get volume (0.0 - 1.0) and mute state from a Chromecast
pub fn get_volume(device: &ChromecastDevice) -> Result<(f32, bool)> {
    let conn = connect(device)?;
    let status = conn
        .receiver
        .get_status()
        .map_err(|e| Error::Upnp(format!("Failed to get status: {e}")))?;

    let volume = status.volume.level.unwrap_or(1.0);
    let muted = status.volume.muted.unwrap_or(false);

    Ok((volume, muted))
}

// Helper to get active transport ID and media session ID
fn get_active_session(conn: &CastDevice) -> Result<(String, i32)> {
    let receiver_status = conn
        .receiver
        .get_status()
        .map_err(|e| Error::Upnp(format!("Failed to get receiver status: {e}")))?;

    if let Some(app) = receiver_status.applications.first() {
        let transport_id = app.transport_id.clone();

        // Connect to transport
        conn.connection
            .connect(&transport_id)
            .map_err(|e| Error::Upnp(format!("Failed to connect to transport: {e}")))?;

        let status = conn
            .media
            .get_status(&transport_id, None::<i32>)
            .map_err(|e| Error::Upnp(format!("Failed to get media status: {e}")))?;

        if let Some(entry) = status.entries.first() {
            return Ok((transport_id, entry.media_session_id));
        }
    }

    Err(Error::Upnp("No active media session".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires real Chromecast on network
    async fn test_discover() {
        let devices = discover(Duration::from_secs(5)).await.unwrap();
        for device in &devices {
            println!("Found: {} at {}:{}", device.name, device.host, device.port);
        }
    }
}
