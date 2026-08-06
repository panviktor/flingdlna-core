use super::parse::build_location_url;
use super::prelude::{Duration, Error, IpAddr, Result, SocketAddr};
use super::socket::{create_sender_socket, create_ssdp_listener, send_multicast_with_fallback};
use super::{SsdpService, SERVER_DEVICE_TYPES, SSDP_MULTICAST_ADDR, SSDP_PORT};
use crate::types::ServerInfo;
use tokio::net::UdpSocket;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, trace, warn};

impl SsdpService {
    /// Start the SSDP announcer for server mode
    /// Returns a handle that can be used to stop the announcer
    pub fn start_announcer(&self) -> Result<JoinHandle<()>> {
        let server_info = self
            .server_info
            .read()
            .ok()
            .and_then(|guard| guard.clone())
            .ok_or_else(|| Error::Config("Server info not set".into()))?;

        let local_ip = self.local_ip;
        let http_port = self.http_port;
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        let handle = tokio::spawn(async move {
            info!(
                "Starting SSDP announcer for '{}'",
                server_info.friendly_name
            );

            // Create socket for announcements
            let socket = match create_sender_socket(local_ip).await {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to create announcement socket: {}", e);
                    return;
                }
            };

            // Also try to create a listener for M-SEARCH requests
            let listener = create_ssdp_listener(local_ip).await;

            // Send initial announcement
            if let Err(e) = send_notify(&socket, &server_info, local_ip, http_port, true).await {
                error!("Failed to send initial announcement: {}", e);
            }

            // Announcement interval (5 minutes, with cache-control of 30 minutes)
            let mut announce_interval = tokio::time::interval(Duration::from_secs(300));
            announce_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            let mut buf = [0u8; 2048];

            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        info!("Shutting down SSDP announcer");
                        // Send byebye
                        let _ = send_notify(&socket, &server_info, local_ip, http_port, false).await;
                        break;
                    }
                    _ = announce_interval.tick() => {
                        debug!("Sending periodic SSDP announcement");
                        if let Err(e) = send_notify(&socket, &server_info, local_ip, http_port, true).await {
                            warn!("Failed to send announcement: {}", e);
                        }
                    }
                    result = async {
                        if let Some(ref listener) = listener {
                            listener.recv_from(&mut buf).await
                        } else {
                            // No listener, just wait forever
                            futures::future::pending::<std::io::Result<(usize, SocketAddr)>>().await
                        }
                    } => {
                        if let Ok((len, from)) = result {
                            let request = String::from_utf8_lossy(&buf[..len]);
                            if request.contains("M-SEARCH") {
                                trace!("Received M-SEARCH from {}", from);
                                if let Err(e) = handle_msearch(&socket, &request, from, &server_info, local_ip, http_port).await {
                                    debug!("Error handling M-SEARCH: {}", e);
                                }
                            }
                        }
                    }
                }
            }
        });

        Ok(handle)
    }

    /// Stop all SSDP services
    pub fn stop(&self) {
        let _ = self.shutdown_tx.send(());
    }

    /// Send SSDP byebye notifications manually (for graceful shutdown)
    /// This is useful when you want to send byebye without stopping the announcer
    pub async fn send_byebye(&self) -> Result<()> {
        let server_info = self
            .server_info
            .read()
            .ok()
            .and_then(|guard| guard.clone())
            .ok_or_else(|| Error::Config("Server info not set".into()))?;

        let socket = self.create_socket().await?;
        send_notify(&socket, &server_info, self.local_ip, self.http_port, false).await?;
        info!("Sent SSDP byebye notifications");
        Ok(())
    }

    /// Send SSDP alive notifications manually
    /// This is useful when you want to re-announce the server
    pub async fn send_alive(&self) -> Result<()> {
        let server_info = self
            .server_info
            .read()
            .ok()
            .and_then(|guard| guard.clone())
            .ok_or_else(|| Error::Config("Server info not set".into()))?;

        let socket = self.create_socket().await?;
        send_notify(&socket, &server_info, self.local_ip, self.http_port, true).await?;
        info!("Sent SSDP alive notifications");
        Ok(())
    }

    /// Update the server's friendly name and re-announce on the network
    ///
    /// This sends byebye with the old name, updates the name, and sends alive with the new name.
    /// This allows changing the server name without restarting.
    pub async fn update_server_name(&self, new_name: String) -> Result<()> {
        let old_info = self
            .server_info
            .read()
            .ok()
            .and_then(|guard| guard.clone())
            .ok_or_else(|| Error::Config("Server info not set".into()))?;

        // Send byebye with old name
        let socket = self.create_socket().await?;
        send_notify(&socket, &old_info, self.local_ip, self.http_port, false).await?;
        info!(
            "Sent byebye for old server name: '{}'",
            old_info.friendly_name
        );

        // Update the server info with new name
        let mut new_info = old_info.clone();
        new_info.friendly_name = new_name.clone();

        if let Ok(mut guard) = self.server_info.write() {
            *guard = Some(new_info.clone());
        }

        // Small delay to ensure byebye is processed
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Send alive with new name
        send_notify(&socket, &new_info, self.local_ip, self.http_port, true).await?;
        info!("Sent alive for new server name: '{}'", new_name);

        Ok(())
    }
}

/// Send NOTIFY announcement (alive or byebye)
async fn send_notify(
    socket: &UdpSocket,
    server_info: &ServerInfo,
    local_ip: IpAddr,
    http_port: u16,
    alive: bool,
) -> Result<()> {
    let multicast_addr = SocketAddr::new(IpAddr::V4(SSDP_MULTICAST_ADDR), SSDP_PORT);
    let nts = if alive { "ssdp:alive" } else { "ssdp:byebye" };
    let location = build_location_url(local_ip, http_port);

    for device_type in SERVER_DEVICE_TYPES {
        let usn = if *device_type == "upnp:rootdevice" {
            format!("{}::upnp:rootdevice", server_info.udn)
        } else {
            format!("{}::{}", server_info.udn, device_type)
        };

        let notify = if alive {
            format!(
                "NOTIFY * HTTP/1.1\r\n\
                 HOST: {SSDP_MULTICAST_ADDR}:{SSDP_PORT}\r\n\
                 CACHE-CONTROL: max-age=1800\r\n\
                 LOCATION: {location}\r\n\
                 NT: {device_type}\r\n\
                 NTS: {nts}\r\n\
                 SERVER: flingdlna/1.0 UPnP/1.0\r\n\
                 USN: {usn}\r\n\
                 \r\n"
            )
        } else {
            format!(
                "NOTIFY * HTTP/1.1\r\n\
                 HOST: {SSDP_MULTICAST_ADDR}:{SSDP_PORT}\r\n\
                 NT: {device_type}\r\n\
                 NTS: {nts}\r\n\
                 USN: {usn}\r\n\
                 \r\n"
            )
        };

        send_multicast_with_fallback(
            socket,
            notify.as_bytes(),
            multicast_addr,
            local_ip,
            "NOTIFY",
        )
        .await?;
        // Small delay between announcements
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Also announce the UDN itself
    let notify = if alive {
        format!(
            "NOTIFY * HTTP/1.1\r\n\
             HOST: {}:{}\r\n\
             CACHE-CONTROL: max-age=1800\r\n\
             LOCATION: {}\r\n\
             NT: {}\r\n\
             NTS: {}\r\n\
             SERVER: flingdlna/1.0 UPnP/1.0\r\n\
             USN: {}\r\n\
             \r\n",
            SSDP_MULTICAST_ADDR, SSDP_PORT, location, server_info.udn, nts, server_info.udn
        )
    } else {
        format!(
            "NOTIFY * HTTP/1.1\r\n\
             HOST: {}:{}\r\n\
             NT: {}\r\n\
             NTS: {}\r\n\
             USN: {}\r\n\
             \r\n",
            SSDP_MULTICAST_ADDR, SSDP_PORT, server_info.udn, nts, server_info.udn
        )
    };
    send_multicast_with_fallback(
        socket,
        notify.as_bytes(),
        multicast_addr,
        local_ip,
        "NOTIFY",
    )
    .await?;

    Ok(())
}

/// Handle M-SEARCH request
async fn handle_msearch(
    socket: &UdpSocket,
    request: &str,
    from: SocketAddr,
    server_info: &ServerInfo,
    local_ip: IpAddr,
    http_port: u16,
) -> Result<()> {
    // Parse the ST (search target) from the request
    let st = request
        .lines()
        .find(|line| line.to_uppercase().starts_with("ST:"))
        .map(|line| line[3..].trim())
        .unwrap_or("ssdp:all");

    let location = build_location_url(local_ip, http_port);

    // Check if we should respond to this search target
    let should_respond = st == "ssdp:all"
        || st == "upnp:rootdevice"
        || SERVER_DEVICE_TYPES.iter().any(|dt| st.contains(dt))
        || st.contains(&server_info.udn);

    if !should_respond {
        return Ok(());
    }

    trace!("Responding to M-SEARCH for '{}' from {}", st, from);

    // Respond with matching device types
    for device_type in SERVER_DEVICE_TYPES {
        if st == "ssdp:all" || st == *device_type || st == "upnp:rootdevice" {
            let usn = if *device_type == "upnp:rootdevice" {
                format!("{}::upnp:rootdevice", server_info.udn)
            } else {
                format!("{}::{}", server_info.udn, device_type)
            };

            let response = format!(
                "HTTP/1.1 200 OK\r\n\
                 CACHE-CONTROL: max-age=1800\r\n\
                 DATE: {}\r\n\
                 EXT:\r\n\
                 LOCATION: {}\r\n\
                 SERVER: flingdlna/1.0 UPnP/1.0\r\n\
                 ST: {}\r\n\
                 USN: {}\r\n\
                 \r\n",
                chrono::Utc::now().format("%a, %d %b %Y %H:%M:%S GMT"),
                location,
                device_type,
                usn
            );

            // Add small random delay (0-100ms) to avoid network congestion
            let delay = Duration::from_millis(fastrand::u64(0..100));
            tokio::time::sleep(delay).await;

            socket.send_to(response.as_bytes(), from).await?;
        }
    }

    Ok(())
}

/// Fast random number generator
mod fastrand {
    use std::cell::Cell;

    thread_local! {
        static RNG: Cell<u64> = Cell::new(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64
        );
    }

    pub fn u64(range: std::ops::Range<u64>) -> u64 {
        RNG.with(|rng| {
            let mut state = rng.get();
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            rng.set(state);
            let value = state.wrapping_mul(0x2545F4914F6CDD1D);
            range.start + (value % (range.end - range.start))
        })
    }
}
