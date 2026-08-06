use crate::protocol::Response;
use crate::DlnaCombo;
use dlna_core::Renderer;

pub(super) async fn find_renderer_or_error(
    combo: &DlnaCombo,
    device: &str,
) -> Result<Renderer, Response> {
    match combo.find_renderer(device).await {
        Ok(Some(r)) => Ok(r),
        Ok(None) => Err(Response::error(format!("Device not found: {device}"))),
        Err(e) => Err(Response::error(e.to_string())),
    }
}
