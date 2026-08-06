use dlna_core::{Error, Renderer, Result};
use url::Url;

const AVTRANSPORT_SERVICE: &str = "urn:schemas-upnp-org:service:AVTransport:1";
const RENDERING_CONTROL_SERVICE: &str = "urn:schemas-upnp-org:service:RenderingControl:1";

pub(crate) fn av_transport_service_type(renderer: &Renderer) -> &str {
    renderer
        .av_transport_service_type
        .as_deref()
        .unwrap_or(AVTRANSPORT_SERVICE)
}

pub(crate) fn rendering_control_service_type(renderer: &Renderer) -> &str {
    renderer
        .rendering_control_service_type
        .as_deref()
        .unwrap_or(RENDERING_CONTROL_SERVICE)
}

/// Get the AVTransport control URL for a renderer
pub(crate) fn get_control_url(renderer: &Renderer) -> Result<Url> {
    renderer
        .av_transport_url
        .clone()
        .ok_or_else(|| Error::ServiceNotFound("AVTransport control URL not found".into()))
}

/// Get the RenderingControl URL for a renderer
pub(crate) fn get_rendering_control_url(renderer: &Renderer) -> Result<Url> {
    renderer
        .rendering_control_url
        .clone()
        .ok_or_else(|| Error::ServiceNotFound("RenderingControl URL not found".into()))
}
