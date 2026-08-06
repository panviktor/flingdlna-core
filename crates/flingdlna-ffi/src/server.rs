use std::ffi::{c_char, c_void};
use std::ptr;

use crate::helpers::{c_str_to_string, string_to_c};
use crate::state::{combo_ref, get_combo_arc, get_runtime, set_error, with_combo_sync};
use crate::types::{FFIResult, FFIResultCallback, FFI_ERROR, FFI_NOT_INITIALIZED, FFI_OK};

// =============================================================================
// Server URL Function
// =============================================================================

/// Get the media server URL
#[no_mangle]
pub extern "C" fn fling_server_url() -> *mut c_char {
    with_combo_sync(|combo| {
        combo
            .server_url()
            .map(|url| string_to_c(&url))
            .unwrap_or(ptr::null_mut())
    })
    .unwrap_or(ptr::null_mut())
}

// =============================================================================
// Server Name Management
// =============================================================================

/// Update the server's friendly name without restarting
///
/// This sends SSDP byebye with the old name, updates the name, and sends alive with the new name.
/// This allows changing the server name without restarting.
///
/// # Arguments
/// * `name` - New server name (must not be NULL)
///
/// # Returns
/// FFI_OK on success, error code otherwise
#[no_mangle]
pub extern "C" fn fling_set_server_name(name: *const c_char) -> FFIResult {
    let new_name = match unsafe { c_str_to_string(name) } {
        Some(n) if !n.is_empty() => n,
        _ => {
            set_error("Invalid server name");
            return FFI_ERROR;
        }
    };

    let runtime = match get_runtime() {
        Some(rt) => rt,
        None => {
            set_error("Runtime not initialized");
            return FFI_NOT_INITIALIZED;
        }
    };

    let arc = match combo_ref() {
        Some(a) => a,
        None => {
            set_error("Not initialized");
            return FFI_NOT_INITIALIZED;
        }
    };

    runtime.block_on(async {
        let guard = arc.read().await;
        match guard.as_ref() {
            Some(combo) => match combo.set_server_name(new_name.clone()).await {
                Ok(()) => {
                    tracing::info!("FFI: Server name updated to '{}'", new_name);
                    FFI_OK
                }
                Err(e) => {
                    set_error(e.to_string());
                    FFI_ERROR
                }
            },
            None => {
                set_error("Not initialized");
                FFI_NOT_INITIALIZED
            }
        }
    })
}

/// Update the server's friendly name without restarting (async, non-blocking)
///
/// This sends SSDP byebye with the old name, updates the name, and sends alive with the new name.
///
/// # Arguments
/// * `name` - New server name (must not be NULL)
/// * `callback` - Function to call when complete
/// * `context` - User data passed to callback
#[no_mangle]
pub extern "C" fn fling_set_server_name_async(
    name: *const c_char,
    callback: FFIResultCallback,
    context: *mut c_void,
) {
    let new_name = match unsafe { c_str_to_string(name) } {
        Some(n) if !n.is_empty() => n,
        _ => {
            set_error("Invalid server name");
            callback(FFI_ERROR, context);
            return;
        }
    };

    let rt = match get_runtime() {
        Some(rt) => rt,
        None => {
            set_error("Not initialized");
            callback(FFI_NOT_INITIALIZED, context);
            return;
        }
    };

    let combo_arc = match get_combo_arc() {
        Some(arc) => arc,
        None => {
            set_error("Not initialized");
            callback(FFI_NOT_INITIALIZED, context);
            return;
        }
    };

    let ctx_ptr = context as usize;

    rt.spawn(async move {
        let guard = combo_arc.read().await;
        let result = match guard.as_ref() {
            Some(combo) => match combo.set_server_name(new_name.clone()).await {
                Ok(()) => {
                    tracing::info!("FFI: Server name updated to '{}'", new_name);
                    FFI_OK
                }
                Err(e) => {
                    set_error(e.to_string());
                    FFI_ERROR
                }
            },
            None => {
                set_error("Not initialized");
                FFI_NOT_INITIALIZED
            }
        };

        callback(result, ctx_ptr as *mut c_void);
    });
}

// =============================================================================
// File Watch Control
// =============================================================================

/// Enable or disable automatic file watching
///
/// When enabled, the library automatically detects file changes in the media
/// directory and updates the library accordingly.
///
/// # Arguments
/// * `enabled` - true to enable, false to disable
///
/// # Returns
/// FFI_OK on success, error code otherwise
#[no_mangle]
pub extern "C" fn fling_set_file_watch_enabled(enabled: bool) -> FFIResult {
    let runtime = match get_runtime() {
        Some(rt) => rt,
        None => {
            set_error("Runtime not initialized");
            return FFI_NOT_INITIALIZED;
        }
    };

    let arc = match combo_ref() {
        Some(a) => a,
        None => {
            set_error("Not initialized");
            return FFI_NOT_INITIALIZED;
        }
    };

    // Must run inside tokio runtime because start_watcher uses tokio::spawn
    runtime.block_on(async {
        let mut guard = arc.write().await;
        let combo = match guard.as_mut() {
            Some(c) => c,
            None => {
                set_error("Not initialized");
                return FFI_NOT_INITIALIZED;
            }
        };

        match combo.set_file_watch_enabled(enabled) {
            Ok(()) => {
                tracing::info!(
                    "FFI: File watch {}",
                    if enabled { "enabled" } else { "disabled" }
                );
                FFI_OK
            }
            Err(e) => {
                set_error(format!("Failed to set file watch: {e}"));
                FFI_ERROR
            }
        }
    })
}

/// Check if file watching is enabled
///
/// # Returns
/// true if file watching is enabled, false otherwise
#[no_mangle]
pub extern "C" fn fling_is_file_watch_enabled() -> bool {
    with_combo_sync(|combo| combo.is_file_watch_enabled()).unwrap_or(false)
}
