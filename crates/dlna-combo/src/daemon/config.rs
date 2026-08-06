//! Daemon configuration

use crate::database;
use crate::ServerConfig;
use std::path::PathBuf;
use std::time::Duration;

/// Get the default socket path (App Store compatible)
pub fn default_socket_path() -> PathBuf {
    database::socket_path()
}

/// Configuration for the daemon
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    /// Unix socket path
    pub socket_path: PathBuf,
    /// Base port for streaming servers (will allocate dynamically)
    pub base_streaming_port: u16,
    /// Maximum concurrent streams
    pub max_streams: usize,
    /// Discovery timeout
    pub discovery_timeout: Duration,
    /// Optional server config (if daemon should also run media server)
    pub server_config: Option<ServerConfig>,
    /// Port for UPnP event callbacks
    pub event_callback_port: u16,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            socket_path: default_socket_path(),
            base_streaming_port: 9000,
            max_streams: 10,
            discovery_timeout: Duration::from_secs(5),
            server_config: None,
            event_callback_port: 9050,
        }
    }
}

impl DaemonConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_socket_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.socket_path = path.into();
        self
    }

    pub fn with_server(mut self, config: ServerConfig) -> Self {
        self.server_config = Some(config);
        self
    }
}
