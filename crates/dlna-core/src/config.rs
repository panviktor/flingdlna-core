//! Configuration types for DLNA server and controller
//!
//! Supports loading from TOML configuration files.
//!
//! # Configuration File Location
//!
//! The configuration file is searched in the following order:
//! 1. `./flingdlna.toml` (current directory)
//! 2. `~/.config/flingdlna/config.toml` (XDG config on Linux/macOS)
//!
//! # Example Configuration
//!
//! ```toml
//! [server]
//! name = "My Media Server"
//! port = 8080
//! directories = ["/home/user/Movies", "/home/user/Music"]
//!
//! [controller]
//! discovery_timeout_secs = 30
//! streaming_port = 9000
//!
//! [watcher]
//! enabled = true
//! debounce_ms = 250
//! ```

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use tracing::{debug, info};

/// Configuration for the DLNA Media Server (DMS)
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Friendly name shown to DLNA clients
    pub friendly_name: String,
    /// HTTP port for serving media and handling UPnP requests
    pub http_port: u16,
    /// Directories to scan for media files
    pub directories: Vec<PathBuf>,
    /// Manufacturer name in device description
    pub manufacturer: String,
    /// Model name in device description
    pub model_name: String,
    /// Enable file watcher for auto-updating library
    pub watch_enabled: bool,
    /// Debounce duration for file watcher in milliseconds
    pub watch_debounce_ms: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            friendly_name: "flingdlna Media Server".to_string(),
            http_port: 8080,
            directories: Vec::new(),
            manufacturer: "flingdlna".to_string(),
            model_name: "flingdlna DMS 1.0".to_string(),
            watch_enabled: true,
            watch_debounce_ms: 1000, // 1 second debounce
        }
    }
}

impl ServerConfig {
    /// Create a new server config with the given name
    pub fn new(friendly_name: impl Into<String>) -> Self {
        Self {
            friendly_name: friendly_name.into(),
            ..Default::default()
        }
    }

    /// Create a new server config with hostname-based name
    /// Uses format: "FlingDLNA ({hostname})"
    pub fn new_with_hostname() -> Self {
        let hostname = hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_else(|| "Unknown".to_string());

        Self {
            friendly_name: format!("FlingDLNA ({hostname})"),
            ..Default::default()
        }
    }

    /// Set the HTTP port
    pub fn with_port(mut self, port: u16) -> Self {
        self.http_port = port;
        self
    }

    /// Add a directory to scan
    pub fn with_directory(mut self, path: impl Into<PathBuf>) -> Self {
        self.directories.push(path.into());
        self
    }

    /// Add multiple directories to scan
    pub fn with_directories(mut self, paths: impl IntoIterator<Item = impl Into<PathBuf>>) -> Self {
        self.directories.extend(paths.into_iter().map(|p| p.into()));
        self
    }

    /// Enable or disable file watching
    pub fn with_watch(mut self, enabled: bool) -> Self {
        self.watch_enabled = enabled;
        self
    }

    /// Set file watcher debounce duration in milliseconds
    pub fn with_watch_debounce_ms(mut self, ms: u64) -> Self {
        self.watch_debounce_ms = ms;
        self
    }

    /// Get watch debounce as Duration
    pub fn watch_debounce(&self) -> Duration {
        Duration::from_millis(self.watch_debounce_ms)
    }
}

/// Configuration for the DLNA Controller (DMC)
#[derive(Debug, Clone)]
pub struct ControllerConfig {
    /// Timeout for device discovery
    pub discovery_timeout: Duration,
    /// Port for local streaming server (when pushing files)
    pub streaming_port: u16,
}

impl Default for ControllerConfig {
    fn default() -> Self {
        Self {
            discovery_timeout: Duration::from_secs(30),
            streaming_port: 9000,
        }
    }
}

impl ControllerConfig {
    /// Create a new controller config
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the discovery timeout
    pub fn with_discovery_timeout(mut self, timeout: Duration) -> Self {
        self.discovery_timeout = timeout;
        self
    }

    /// Set the streaming port
    pub fn with_streaming_port(mut self, port: u16) -> Self {
        self.streaming_port = port;
        self
    }
}

/// Combined configuration for server and controller
#[derive(Debug, Clone, Default)]
pub struct ComboConfig {
    pub server: Option<ServerConfig>,
    pub controller: ControllerConfig,
}

impl ComboConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_server(mut self, config: ServerConfig) -> Self {
        self.server = Some(config);
        self
    }

    pub fn with_controller(mut self, config: ControllerConfig) -> Self {
        self.controller = config;
        self
    }
}

// ============================================================================
// TOML Configuration File Support
// ============================================================================

/// TOML-serializable server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfigFile {
    /// Friendly name shown to DLNA clients
    #[serde(default = "default_server_name")]
    pub name: String,
    /// HTTP port
    #[serde(default = "default_server_port")]
    pub port: u16,
    /// Directories to scan
    #[serde(default)]
    pub directories: Vec<PathBuf>,
    /// Manufacturer name
    #[serde(default = "default_manufacturer")]
    pub manufacturer: String,
    /// Model name
    #[serde(default = "default_model")]
    pub model: String,
    /// Enable file watching
    #[serde(default = "default_watch_enabled")]
    pub watch: bool,
    /// Watch debounce in milliseconds
    #[serde(default = "default_watch_debounce_ms")]
    pub watch_debounce_ms: u64,
}

impl Default for ServerConfigFile {
    fn default() -> Self {
        Self {
            name: default_server_name(),
            port: default_server_port(),
            directories: Vec::new(),
            manufacturer: default_manufacturer(),
            model: default_model(),
            watch: default_watch_enabled(),
            watch_debounce_ms: default_watch_debounce_ms(),
        }
    }
}

fn default_server_name() -> String {
    "flingdlna Media Server".to_string()
}

fn default_server_port() -> u16 {
    8080
}

fn default_manufacturer() -> String {
    "flingdlna".to_string()
}

fn default_model() -> String {
    "flingdlna DMS 1.0".to_string()
}

fn default_watch_enabled() -> bool {
    true
}

fn default_watch_debounce_ms() -> u64 {
    1000 // 1 second debounce
}

impl From<ServerConfigFile> for ServerConfig {
    fn from(f: ServerConfigFile) -> Self {
        Self {
            friendly_name: f.name,
            http_port: f.port,
            directories: f.directories,
            manufacturer: f.manufacturer,
            model_name: f.model,
            watch_enabled: f.watch,
            watch_debounce_ms: f.watch_debounce_ms,
        }
    }
}

/// TOML-serializable controller configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControllerConfigFile {
    /// Discovery timeout in seconds
    #[serde(default = "default_discovery_timeout")]
    pub discovery_timeout_secs: u64,
    /// Port for local streaming server
    #[serde(default = "default_streaming_port")]
    pub streaming_port: u16,
}

impl Default for ControllerConfigFile {
    fn default() -> Self {
        Self {
            discovery_timeout_secs: default_discovery_timeout(),
            streaming_port: default_streaming_port(),
        }
    }
}

fn default_discovery_timeout() -> u64 {
    30
}

fn default_streaming_port() -> u16 {
    9000
}

impl From<ControllerConfigFile> for ControllerConfig {
    fn from(f: ControllerConfigFile) -> Self {
        Self {
            discovery_timeout: Duration::from_secs(f.discovery_timeout_secs),
            streaming_port: f.streaming_port,
        }
    }
}

/// TOML-serializable watcher configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatcherConfigFile {
    /// Enable file watching
    #[serde(default = "default_watcher_enabled")]
    pub enabled: bool,
    /// Debounce duration in milliseconds
    #[serde(default = "default_debounce_ms")]
    pub debounce_ms: u64,
}

impl Default for WatcherConfigFile {
    fn default() -> Self {
        Self {
            enabled: default_watcher_enabled(),
            debounce_ms: default_debounce_ms(),
        }
    }
}

fn default_watcher_enabled() -> bool {
    true
}

fn default_debounce_ms() -> u64 {
    250
}

/// Complete application configuration file
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfigFile {
    #[serde(default)]
    pub server: Option<ServerConfigFile>,
    #[serde(default)]
    pub controller: ControllerConfigFile,
    #[serde(default)]
    pub watcher: WatcherConfigFile,
}

impl AppConfigFile {
    /// Load configuration from the default config file locations
    ///
    /// Searches in order:
    /// 1. `./flingdlna.toml`
    /// 2. `~/.config/flingdlna/config.toml`
    ///
    /// Returns default config if no file is found.
    pub fn load() -> Result<Self> {
        // Try current directory first
        let local_path = PathBuf::from("flingdlna.toml");
        if local_path.exists() {
            info!("Loading config from ./flingdlna.toml");
            return Self::load_from_path(&local_path);
        }

        // Try XDG config directory
        if let Some(config_dir) = dirs::config_dir() {
            let config_path = config_dir.join("flingdlna").join("config.toml");
            if config_path.exists() {
                info!("Loading config from {:?}", config_path);
                return Self::load_from_path(&config_path);
            }
        }

        // No config file found, use defaults
        debug!("No config file found, using defaults");
        Ok(Self::default())
    }

    /// Load configuration from a specific path
    pub fn load_from_path(path: &std::path::Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| Error::Config(format!("Failed to read config file {path:?}: {e}")))?;

        toml::from_str(&content)
            .map_err(|e| Error::Config(format!("Failed to parse config file {path:?}: {e}")))
    }

    /// Save configuration to the default XDG config location
    pub fn save(&self) -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| Error::Config("Could not determine config directory".into()))?
            .join("flingdlna");

        std::fs::create_dir_all(&config_dir)
            .map_err(|e| Error::Config(format!("Failed to create config directory: {e}")))?;

        let config_path = config_dir.join("config.toml");
        self.save_to_path(&config_path)?;

        Ok(config_path)
    }

    /// Save configuration to a specific path
    pub fn save_to_path(&self, path: &std::path::Path) -> Result<()> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| Error::Config(format!("Failed to serialize config: {e}")))?;

        std::fs::write(path, content)
            .map_err(|e| Error::Config(format!("Failed to write config file {path:?}: {e}")))?;

        info!("Saved config to {:?}", path);
        Ok(())
    }

    /// Convert to runtime configuration
    pub fn into_combo_config(self) -> ComboConfig {
        ComboConfig {
            server: self.server.map(|s| s.into()),
            controller: self.controller.into(),
        }
    }

    /// Get watcher debounce duration
    pub fn watcher_debounce(&self) -> Duration {
        Duration::from_millis(self.watcher.debounce_ms)
    }

    /// Check if watcher is enabled
    pub fn watcher_enabled(&self) -> bool {
        self.watcher.enabled
    }
}

/// Get the default config file path
pub fn default_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("flingdlna").join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_config() {
        let toml = r#"
[server]
name = "Test Server"
port = 9090
directories = ["/home/user/Videos"]

[controller]
discovery_timeout_secs = 10
streaming_port = 8000

[watcher]
enabled = false
debounce_ms = 500
"#;

        let config: AppConfigFile = toml::from_str(toml).unwrap();

        assert_eq!(config.server.as_ref().unwrap().name, "Test Server");
        assert_eq!(config.server.as_ref().unwrap().port, 9090);
        assert_eq!(config.controller.discovery_timeout_secs, 10);
        assert_eq!(config.controller.streaming_port, 8000);
        assert!(!config.watcher.enabled);
        assert_eq!(config.watcher.debounce_ms, 500);
    }

    #[test]
    fn test_default_config() {
        let config = AppConfigFile::default();

        assert!(config.server.is_none());
        assert_eq!(config.controller.discovery_timeout_secs, 30);
        assert_eq!(config.controller.streaming_port, 9000);
        assert!(config.watcher.enabled);
        assert_eq!(config.watcher.debounce_ms, 250);
    }

    #[test]
    fn test_serialize_config() {
        let config = AppConfigFile {
            server: Some(ServerConfigFile {
                name: "My Server".to_string(),
                port: 8080,
                directories: vec![PathBuf::from("/media")],
                manufacturer: "test".to_string(),
                model: "test 1.0".to_string(),
                watch: true,
                watch_debounce_ms: 1000,
            }),
            controller: ControllerConfigFile::default(),
            watcher: WatcherConfigFile::default(),
        };

        let toml = toml::to_string_pretty(&config).unwrap();
        assert!(toml.contains("My Server"));
        assert!(toml.contains("8080"));
    }
}
