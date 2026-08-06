//! dlna-core - Core types, SSDP, and configuration for flingdlna
//!
//! This crate provides the foundational types and unified SSDP implementation
//! used by both the DLNA server and controller crates.

pub mod config;
pub mod error;
pub mod net;
pub mod process;
pub mod ssdp;
pub mod types;
pub mod wol;

pub use config::{
    default_config_path, AppConfigFile, ComboConfig, ControllerConfig, ControllerConfigFile,
    ServerConfig, ServerConfigFile, WatcherConfigFile,
};
pub use error::{Error, Result};
pub use net::best_local_ipv4;
pub use process::run_command_capture_stdout;
pub use ssdp::SsdpService;
pub use types::{
    DeviceCapabilities, DeviceIcon, MediaFile, MediaInfo, MediaType, PlaybackInfo, Renderer,
    ServerInfo, TransportState,
};
pub use wol::{get_mac_from_arp, wake};
