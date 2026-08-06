use std::ffi::{c_char, CString};
use std::ptr;
use std::sync::{Arc, OnceLock};

use dlna_combo::DlnaCombo;
use dlna_controller::EventManager;
use parking_lot::RwLock as ParkingLotRwLock;
use tokio::runtime::Runtime;
use tokio::sync::RwLock as TokioRwLock;

static RUNTIME: OnceLock<Runtime> = OnceLock::new();
// Use tokio RwLock - Send-safe across await for async API
static COMBO: OnceLock<Arc<TokioRwLock<Option<DlnaCombo>>>> = OnceLock::new();
// Event manager for UPnP events (wrapped in Arc for sharing with SessionManager)
static EVENT_MANAGER: OnceLock<Arc<TokioRwLock<Option<Arc<EventManager>>>>> = OnceLock::new();
// Error storage still uses parking_lot (no async needed)
static LAST_ERROR: ParkingLotRwLock<Option<CString>> = ParkingLotRwLock::new(None);

pub(crate) fn set_error(msg: impl Into<String>) {
    let cstring =
        CString::new(msg.into()).unwrap_or_else(|_| CString::new("Unknown error").unwrap());
    *LAST_ERROR.write() = Some(cstring);
}

pub(crate) fn last_error_ptr() -> *const c_char {
    LAST_ERROR
        .read()
        .as_ref()
        .map(|s| s.as_ptr())
        .unwrap_or(ptr::null())
}

pub(crate) fn get_or_create_runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| Runtime::new().expect("Failed to create tokio runtime"))
}

pub(crate) fn get_runtime() -> Option<&'static Runtime> {
    RUNTIME.get()
}

pub(crate) fn get_or_init_combo() -> &'static Arc<TokioRwLock<Option<DlnaCombo>>> {
    COMBO.get_or_init(|| Arc::new(TokioRwLock::new(None)))
}

pub(crate) fn combo_ref() -> Option<&'static Arc<TokioRwLock<Option<DlnaCombo>>>> {
    COMBO.get()
}

pub(crate) fn get_combo_arc() -> Option<&'static Arc<TokioRwLock<Option<DlnaCombo>>>> {
    COMBO.get()
}

// Sync helper - uses block_on with async lock (for sync closures only)
pub(crate) fn with_combo_sync<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&DlnaCombo) -> R,
{
    let rt = get_runtime()?;
    let arc = COMBO.get()?;
    rt.block_on(async {
        let guard = arc.read().await;
        guard.as_ref().map(f)
    })
}

/// Run an async operation with the combo - returns (runtime, combo_arc) for use in block_on
/// Usage: let (rt, arc) = get_runtime_and_combo()?; rt.block_on(async { ... })
pub(crate) fn get_runtime_and_combo() -> Option<(
    &'static Runtime,
    &'static Arc<TokioRwLock<Option<DlnaCombo>>>,
)> {
    let rt = get_runtime()?;
    let arc = COMBO.get()?;
    Some((rt, arc))
}

// Sync helper for mutable access (unused but kept for completeness)
#[allow(dead_code)]
pub(crate) fn with_combo_sync_mut<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut DlnaCombo) -> R,
{
    let rt = get_runtime()?;
    let arc = COMBO.get()?;
    rt.block_on(async {
        let mut guard = arc.write().await;
        guard.as_mut().map(f)
    })
}

// === EventManager access ===

pub(crate) fn get_or_init_event_manager() -> &'static Arc<TokioRwLock<Option<Arc<EventManager>>>> {
    EVENT_MANAGER.get_or_init(|| Arc::new(TokioRwLock::new(None)))
}
