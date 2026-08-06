//! Chromecast support for daemon (conditionally compiled)

#[cfg(feature = "chromecast")]
use super::cache::{CachedChromecast, ChromecastCache};
#[cfg(feature = "chromecast")]
use crate::protocol::{DeviceType, RendererInfo};
#[cfg(feature = "chromecast")]
use dlna_controller::chromecast::{self, ChromecastDevice};
#[cfg(feature = "chromecast")]
use std::time::{Duration, Instant};
#[cfg(feature = "chromecast")]
use tracing::{debug, info};

/// Convert Chromecast device to RendererInfo
#[cfg(feature = "chromecast")]
pub fn chromecast_to_renderer_info(device: &ChromecastDevice) -> RendererInfo {
    // Generate icon URL from mDNS 'ic' field if available
    let icon_urls = if let Some(ref icon_path) = device.icon_path {
        let icon_url = format!("http://{}:8008{}", device.host, icon_path);
        info!(
            "Chromecast '{}': Generated icon URL: {}",
            device.name, icon_url
        );
        vec![icon_url]
    } else {
        info!(
            "Chromecast '{}': No icon path (ic field) in mDNS TXT record",
            device.name
        );
        Vec::new()
    };

    let renderer_info = RendererInfo {
        name: device.name.clone(),
        location: format!("cast://{}:{}", device.host, device.port),
        device_type: DeviceType::Chromecast,
        manufacturer: Some("Google".to_string()),
        model: device.model.clone(),
        mac_address: None,
        icon_urls: icon_urls.clone(),
        firmware_version: device.version.clone(),
        capabilities: device.capabilities.clone(),
    };

    info!(
        "Chromecast '{}': RendererInfo created with {} icon URL(s)",
        device.name,
        icon_urls.len()
    );

    renderer_info
}

/// Background Chromecast discovery
#[cfg(feature = "chromecast")]
pub async fn do_chromecast_discovery(cache: &ChromecastCache) {
    match chromecast::discover(Duration::from_secs(3)).await {
        Ok(devices) => {
            let mut cache = cache.write().await;
            let now = Instant::now();
            for device in devices {
                cache.insert(
                    device.name.clone(),
                    CachedChromecast {
                        device,
                        cached_at: now,
                    },
                );
            }
            info!("Chromecast cache updated: {} device(s)", cache.len());
        }
        Err(e) => {
            debug!("Chromecast discovery failed: {}", e);
        }
    }
}

/// Get Chromecast devices as RendererInfo list
#[cfg(feature = "chromecast")]
pub async fn get_chromecast_renderers(cache: &ChromecastCache) -> Vec<RendererInfo> {
    let cache_guard = cache.read().await;
    let count = cache_guard.len();
    info!("Reading Chromecast cache: {} device(s)", count);
    cache_guard
        .values()
        .map(|c| chromecast_to_renderer_info(&c.device))
        .collect()
}

/// Find a Chromecast device by name
#[cfg(feature = "chromecast")]
#[allow(dead_code)] // Will be used for Chromecast playback control
pub async fn find_chromecast(cache: &ChromecastCache, name: &str) -> Option<ChromecastDevice> {
    let cache = cache.read().await;
    // Check for exact match first
    if let Some(cached) = cache.get(name) {
        return Some(cached.device.clone());
    }
    // Then check for partial match
    let name_lower = name.to_lowercase();
    cache
        .values()
        .find(|c| c.device.name.to_lowercase().contains(&name_lower))
        .map(|c| c.device.clone())
}
