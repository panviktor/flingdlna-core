use std::sync::Arc;

use dlna_combo::{DlnaCombo, Renderer};
use tokio::runtime::Runtime;
use tokio::sync::RwLock as TokioRwLock;

// Helper: find renderer by name
pub(crate) fn find_renderer_by_name(
    rt: &Runtime,
    arc: &Arc<TokioRwLock<Option<DlnaCombo>>>,
    device_name: &str,
) -> Option<Renderer> {
    let name = device_name.to_string();
    rt.block_on(async {
        let guard = arc.read().await;
        match guard.as_ref() {
            Some(combo) => combo.find_renderer(&name).await.ok().flatten(),
            None => None,
        }
    })
}
