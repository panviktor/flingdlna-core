//! Unified SSDP implementation for both discovery and announcement
//!
//! This module provides a single UDP socket that can:
//! - Send M-SEARCH requests and collect responses (for discovering renderers)
//! - Respond to M-SEARCH requests (for advertising this server)
//! - Send NOTIFY announcements (for advertising this server)

mod announce;
mod discovery;
pub mod fetch;
mod parse;
mod prelude;
mod socket;
mod xml;

use crate::error::Result;
use crate::net::best_local_ipv4;
use crate::types::ServerInfo;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, RwLock};
use tokio::net::UdpSocket;
use tokio::sync::broadcast;

/// SSDP multicast address
const SSDP_MULTICAST_ADDR: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const SSDP_PORT: u16 = 1900;

/// Device types we advertise as a server
const SERVER_DEVICE_TYPES: &[&str] = &[
    "upnp:rootdevice",
    "urn:schemas-upnp-org:device:MediaServer:1",
    "urn:schemas-upnp-org:service:ContentDirectory:1",
    "urn:schemas-upnp-org:service:ConnectionManager:1",
];

/// Device type we search for (Media Renderer)
const RENDERER_SEARCH_TARGET: &str = "urn:schemas-upnp-org:device:MediaRenderer:1";

/// AVTransport service type
const AVTRANSPORT_SERVICE: &str = "urn:schemas-upnp-org:service:AVTransport:1";

const MSEARCH_TARGETS: [&str; 3] = [RENDERER_SEARCH_TARGET, AVTRANSPORT_SERVICE, "ssdp:all"];

/// Unified SSDP service for discovery and announcement
pub struct SsdpService {
    local_ip: IpAddr,
    http_port: u16,
    server_info: Arc<RwLock<Option<ServerInfo>>>,
    shutdown_tx: broadcast::Sender<()>,
}

impl SsdpService {
    /// Create a new SSDP service
    pub async fn new(http_port: u16) -> Result<Self> {
        let local_ip = IpAddr::V4(best_local_ipv4()?);

        let (shutdown_tx, _) = broadcast::channel(1);

        Ok(Self {
            local_ip,
            http_port,
            server_info: Arc::new(RwLock::new(None)),
            shutdown_tx,
        })
    }

    /// Create with a specific local IP
    pub async fn with_ip(local_ip: IpAddr, http_port: u16) -> Result<Self> {
        let (shutdown_tx, _) = broadcast::channel(1);

        Ok(Self {
            local_ip,
            http_port,
            server_info: Arc::new(RwLock::new(None)),
            shutdown_tx,
        })
    }

    /// Set server info for announcements
    pub fn set_server_info(&mut self, info: ServerInfo) {
        if let Ok(mut guard) = self.server_info.write() {
            *guard = Some(info);
        }
    }

    /// Get the local IP address
    pub fn local_ip(&self) -> IpAddr {
        self.local_ip
    }

    /// Get the HTTP port
    pub fn http_port(&self) -> u16 {
        self.http_port
    }

    /// Set the HTTP port (used when port 0 auto-selects)
    pub fn set_http_port(&mut self, port: u16) {
        self.http_port = port;
    }

    /// Get the base URL for this server
    pub fn base_url(&self) -> String {
        format!(
            "http://{}:{}",
            parse::format_host_for_url(self.local_ip),
            self.http_port
        )
    }

    /// Create a UDP socket for SSDP operations
    async fn create_socket(&self) -> Result<UdpSocket> {
        socket::create_sender_socket(self.local_ip).await
    }

    /// Get server info if set (clones the ServerInfo)
    pub fn server_info(&self) -> Option<ServerInfo> {
        self.server_info.read().ok().and_then(|guard| guard.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::parse::parse_location;
    use crate::types::ServerInfo;

    #[test]
    fn test_parse_location() {
        let response = "HTTP/1.1 200 OK\r\n\
                        LOCATION: http://192.168.1.100:8080/description.xml\r\n\
                        ST: upnp:rootdevice\r\n\
                        \r\n";
        assert_eq!(
            parse_location(response),
            Some("http://192.168.1.100:8080/description.xml".to_string())
        );
    }

    #[test]
    fn test_server_info_new() {
        let info = ServerInfo::new("Test Server");
        assert_eq!(info.friendly_name, "Test Server");
        assert!(info.udn.starts_with("uuid:"));
    }
}
