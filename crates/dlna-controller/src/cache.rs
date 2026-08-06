//! Device cache for storing discovered renderers
//!
//! This module provides a cache for discovered DLNA renderers
//! with TTL-based expiration to avoid re-scanning the network.

use dlna_core::Renderer;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// A cached renderer entry with timestamp
struct CachedRenderer {
    renderer: Renderer,
    cached_at: Instant,
}

/// Device cache for storing discovered renderers with TTL
#[derive(Clone)]
pub struct DeviceCache {
    cache: Arc<RwLock<HashMap<String, CachedRenderer>>>,
    ttl: Duration,
}

impl DeviceCache {
    /// Create a new device cache with the given TTL in seconds
    pub fn new(ttl_seconds: u64) -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            ttl: Duration::from_secs(ttl_seconds),
        }
    }

    /// Get a renderer by UDN if it's still valid
    pub async fn get(&self, udn: &str) -> Option<Renderer> {
        let cache = self.cache.read().await;
        cache.get(udn).and_then(|cached| {
            if cached.cached_at.elapsed() < self.ttl {
                Some(cached.renderer.clone())
            } else {
                None
            }
        })
    }

    /// Get a renderer by friendly name (partial match)
    pub async fn get_by_name(&self, name: &str) -> Option<Renderer> {
        let cache = self.cache.read().await;
        let name_lower = name.to_lowercase();

        cache
            .values()
            .find(|cached| {
                cached.cached_at.elapsed() < self.ttl
                    && cached
                        .renderer
                        .friendly_name
                        .to_lowercase()
                        .contains(&name_lower)
            })
            .map(|cached| cached.renderer.clone())
    }

    /// Get a renderer by location URL (exact match)
    pub async fn get_by_location(&self, location: &str) -> Option<Renderer> {
        let cache = self.cache.read().await;
        cache
            .values()
            .find(|cached| {
                cached.cached_at.elapsed() < self.ttl
                    && cached.renderer.location.as_str() == location
            })
            .map(|cached| cached.renderer.clone())
    }

    /// Get all valid cached renderers
    pub async fn get_all(&self) -> Vec<Renderer> {
        let cache = self.cache.read().await;
        cache
            .values()
            .filter(|cached| cached.cached_at.elapsed() < self.ttl)
            .map(|cached| cached.renderer.clone())
            .collect()
    }

    /// Insert or update a renderer in the cache
    pub async fn insert(&self, renderer: Renderer) {
        let mut cache = self.cache.write().await;
        cache.insert(
            renderer.udn.clone(),
            CachedRenderer {
                renderer,
                cached_at: Instant::now(),
            },
        );
    }

    /// Insert multiple renderers
    pub async fn insert_many(&self, renderers: Vec<Renderer>) {
        let mut cache = self.cache.write().await;
        let now = Instant::now();

        for renderer in renderers {
            cache.insert(
                renderer.udn.clone(),
                CachedRenderer {
                    renderer,
                    cached_at: now,
                },
            );
        }
    }

    /// Invalidate a specific renderer
    pub async fn invalidate(&self, udn: &str) {
        let mut cache = self.cache.write().await;
        cache.remove(udn);
    }

    /// Clear all expired entries from the cache
    pub async fn cleanup_expired(&self) {
        let mut cache = self.cache.write().await;
        cache.retain(|_, cached| cached.cached_at.elapsed() < self.ttl);
    }

    /// Clear the entire cache
    pub async fn clear(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
    }

    /// Get the number of valid entries in the cache
    pub async fn len(&self) -> usize {
        let cache = self.cache.read().await;
        cache
            .values()
            .filter(|cached| cached.cached_at.elapsed() < self.ttl)
            .count()
    }

    /// Check if the cache is empty (no valid entries)
    pub async fn is_empty(&self) -> bool {
        self.len().await == 0
    }
}

impl Default for DeviceCache {
    fn default() -> Self {
        // Default TTL of 5 minutes
        Self::new(300)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    fn test_renderer(name: &str, udn: &str) -> Renderer {
        Renderer {
            friendly_name: name.to_string(),
            location: Url::parse("http://192.168.1.1:8080/description.xml").unwrap(),
            udn: udn.to_string(),
            av_transport_url: None,
            av_transport_service_type: None,
            rendering_control_url: None,
            rendering_control_service_type: None,
            av_transport_event_url: None,
            rendering_control_event_url: None,
            manufacturer: None,
            model_name: None,
            model_number: None,
            model_description: None,
            serial_number: None,
            presentation_url: None,
            mac_address: None,
            icons: vec![],
            firmware_version: None,
            capabilities: None,
        }
    }

    #[tokio::test]
    async fn test_insert_and_get() {
        let cache = DeviceCache::new(60);
        let renderer = test_renderer("Test TV", "uuid:test-1");

        cache.insert(renderer.clone()).await;

        let cached = cache.get("uuid:test-1").await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().friendly_name, "Test TV");
    }

    #[tokio::test]
    async fn test_get_by_name() {
        let cache = DeviceCache::new(60);
        cache.insert(test_renderer("LG OLED TV", "uuid:lg-1")).await;
        cache
            .insert(test_renderer("Samsung TV", "uuid:samsung-1"))
            .await;

        let lg = cache.get_by_name("lg").await;
        assert!(lg.is_some());
        assert_eq!(lg.unwrap().friendly_name, "LG OLED TV");

        let samsung = cache.get_by_name("SAMSUNG").await;
        assert!(samsung.is_some());
    }

    #[tokio::test]
    async fn test_expiration() {
        // Very short TTL for testing
        let cache = DeviceCache::new(0);
        cache.insert(test_renderer("Test TV", "uuid:test-1")).await;

        // Should be expired immediately
        tokio::time::sleep(Duration::from_millis(10)).await;
        let cached = cache.get("uuid:test-1").await;
        assert!(cached.is_none());
    }
}
