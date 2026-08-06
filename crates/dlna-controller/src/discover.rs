//! Device discovery for DLNA renderers

use dlna_core::{Renderer, Result};

/// Fetch renderer information by URL
///
/// This is a convenience wrapper around dlna_core::ssdp::fetch::fetch_renderer_info
/// that allows callers to fetch device information directly from a URL.
pub async fn fetch_renderer_by_url(url: &str) -> Result<Renderer> {
    dlna_core::ssdp::fetch::fetch_renderer_info(url, None).await
}
