//! Cache types for daemon state

use crate::protocol::RendererInfo;
use std::path::PathBuf;
use std::time::Instant;

#[cfg(feature = "chromecast")]
use dlna_controller::chromecast::ChromecastDevice;
#[cfg(feature = "chromecast")]
use std::collections::HashMap;
#[cfg(feature = "chromecast")]
use std::sync::Arc;
#[cfg(feature = "chromecast")]
use tokio::sync::RwLock;

/// Active stream info
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields used for future stream management features
pub struct ActiveStream {
    pub device_name: String,
    pub file_path: PathBuf,
    pub port: u16,
    pub url: String,
    pub started_at: Instant,
}

/// Cached renderer with timestamp
#[derive(Debug, Clone)]
pub struct CachedRenderer {
    pub info: RendererInfo,
    #[allow(dead_code)]
    pub cached_at: Instant,
}

/// Cached Chromecast device with timestamp
#[cfg(feature = "chromecast")]
#[derive(Debug, Clone)]
pub struct CachedChromecast {
    pub device: ChromecastDevice,
    #[allow(dead_code)]
    pub cached_at: Instant,
}

/// Chromecast cache type (enabled)
#[cfg(feature = "chromecast")]
pub type ChromecastCache = Arc<RwLock<HashMap<String, CachedChromecast>>>;

/// Chromecast cache type (disabled - unit type placeholder)
#[cfg(not(feature = "chromecast"))]
pub type ChromecastCache = ();

/// Create a new Chromecast cache (enabled)
#[cfg(feature = "chromecast")]
pub fn new_chromecast_cache() -> ChromecastCache {
    Arc::new(RwLock::new(HashMap::new()))
}

/// Create a new Chromecast cache (disabled - returns unit)
#[cfg(not(feature = "chromecast"))]
pub fn new_chromecast_cache() -> ChromecastCache {
    ()
}
