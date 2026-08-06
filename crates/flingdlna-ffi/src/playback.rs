use std::ffi::{c_char, c_void};
use std::ptr;
use std::time::Duration;

use crate::device::find_renderer_by_name;
use crate::device_actions::{ffi_device_action_async, ffi_device_action_with_value_async};
use crate::helpers::{c_str_to_string, free_c_string, option_string_to_c, string_to_c};
use crate::state::{get_combo_arc, get_runtime, get_runtime_and_combo, set_error};
use crate::types::{
    FFIPlaybackStatus, FFIResult, FFIResultCallback, FFIStatusCallback, FFI_ERROR, FFI_NOT_FOUND,
    FFI_NOT_INITIALIZED, FFI_OK,
};

// =============================================================================
// Playback Control Functions
// =============================================================================

/// Get playback status for a device
#[no_mangle]
pub extern "C" fn fling_get_status(device: *const c_char) -> *mut FFIPlaybackStatus {
    let device_name = match unsafe { c_str_to_string(device) } {
        Some(d) => d,
        None => {
            set_error("Invalid device name");
            return ptr::null_mut();
        }
    };

    let (rt, arc) = match get_runtime_and_combo() {
        Some(x) => x,
        None => {
            set_error("Not initialized");
            return ptr::null_mut();
        }
    };

    let renderer = match find_renderer_by_name(rt, arc, &device_name) {
        Some(r) => r,
        None => {
            set_error("Device not found");
            return ptr::null_mut();
        }
    };

    let info = rt.block_on(async {
        let guard = arc.read().await;
        match guard.as_ref() {
            Some(combo) => combo.get_playback_info(&renderer).await.ok(),
            None => None,
        }
    });

    match info {
        Some(info) => {
            let state_str = format!("{:?}", info.state).to_uppercase();
            Box::into_raw(Box::new(FFIPlaybackStatus {
                state: string_to_c(&state_str),
                position_secs: info.position.as_secs(),
                duration_secs: info.duration.as_secs(),
                current_uri: option_string_to_c(info.current_uri.as_deref()),
            }))
        }
        None => {
            set_error("Failed to get status");
            ptr::null_mut()
        }
    }
}

/// Free playback status
#[no_mangle]
pub extern "C" fn fling_free_status(status: *mut FFIPlaybackStatus) {
    if status.is_null() {
        return;
    }
    unsafe {
        let status = Box::from_raw(status);
        free_c_string(status.state);
        free_c_string(status.current_uri);
    }
}

/// Resume playback
#[no_mangle]
pub extern "C" fn fling_play(device: *const c_char) -> FFIResult {
    let device_name = match unsafe { c_str_to_string(device) } {
        Some(d) => d,
        None => {
            set_error("Invalid device name");
            return FFI_ERROR;
        }
    };

    let (rt, arc) = match get_runtime_and_combo() {
        Some(x) => x,
        None => {
            set_error("Not initialized");
            return FFI_NOT_INITIALIZED;
        }
    };

    let renderer = match find_renderer_by_name(rt, arc, &device_name) {
        Some(r) => r,
        None => {
            set_error("Device not found");
            return FFI_NOT_FOUND;
        }
    };

    let result = rt.block_on(async {
        let guard = arc.read().await;
        match guard.as_ref() {
            Some(combo) => Some(combo.controller().resume(&renderer).await),
            None => None,
        }
    });

    match result {
        Some(Ok(())) => FFI_OK,
        Some(Err(e)) => {
            set_error(e.to_string());
            FFI_ERROR
        }
        None => {
            set_error("Not initialized");
            FFI_NOT_INITIALIZED
        }
    }
}

/// Pause playback
#[no_mangle]
pub extern "C" fn fling_pause(device: *const c_char) -> FFIResult {
    let device_name = match unsafe { c_str_to_string(device) } {
        Some(d) => d,
        None => {
            set_error("Invalid device name");
            return FFI_ERROR;
        }
    };

    let (rt, arc) = match get_runtime_and_combo() {
        Some(x) => x,
        None => {
            set_error("Not initialized");
            return FFI_NOT_INITIALIZED;
        }
    };

    let renderer = match find_renderer_by_name(rt, arc, &device_name) {
        Some(r) => r,
        None => {
            set_error("Device not found");
            return FFI_NOT_FOUND;
        }
    };

    let result = rt.block_on(async {
        let guard = arc.read().await;
        match guard.as_ref() {
            Some(combo) => Some(combo.pause(&renderer).await),
            None => None,
        }
    });

    match result {
        Some(Ok(())) => FFI_OK,
        Some(Err(e)) => {
            set_error(e.to_string());
            FFI_ERROR
        }
        None => {
            set_error("Not initialized");
            FFI_NOT_INITIALIZED
        }
    }
}

/// Stop playback
#[no_mangle]
pub extern "C" fn fling_stop(device: *const c_char) -> FFIResult {
    let device_name = match unsafe { c_str_to_string(device) } {
        Some(d) => d,
        None => {
            set_error("Invalid device name");
            return FFI_ERROR;
        }
    };

    let (rt, arc) = match get_runtime_and_combo() {
        Some(x) => x,
        None => {
            set_error("Not initialized");
            return FFI_NOT_INITIALIZED;
        }
    };

    let renderer = match find_renderer_by_name(rt, arc, &device_name) {
        Some(r) => r,
        None => {
            set_error("Device not found");
            return FFI_NOT_FOUND;
        }
    };

    let result = rt.block_on(async {
        let guard = arc.read().await;
        match guard.as_ref() {
            Some(combo) => Some(combo.stop(&renderer).await),
            None => None,
        }
    });

    match result {
        Some(Ok(())) => FFI_OK,
        Some(Err(e)) => {
            set_error(e.to_string());
            FFI_ERROR
        }
        None => {
            set_error("Not initialized");
            FFI_NOT_INITIALIZED
        }
    }
}

/// Seek to position
#[no_mangle]
pub extern "C" fn fling_seek(device: *const c_char, position_secs: u64) -> FFIResult {
    let device_name = match unsafe { c_str_to_string(device) } {
        Some(d) => d,
        None => {
            set_error("Invalid device name");
            return FFI_ERROR;
        }
    };

    let (rt, arc) = match get_runtime_and_combo() {
        Some(x) => x,
        None => {
            set_error("Not initialized");
            return FFI_NOT_INITIALIZED;
        }
    };

    let renderer = match find_renderer_by_name(rt, arc, &device_name) {
        Some(r) => r,
        None => {
            set_error("Device not found");
            return FFI_NOT_FOUND;
        }
    };

    let pos = Duration::from_secs(position_secs);
    let result = rt.block_on(async {
        let guard = arc.read().await;
        match guard.as_ref() {
            Some(combo) => Some(combo.seek(&renderer, pos).await),
            None => None,
        }
    });

    match result {
        Some(Ok(())) => FFI_OK,
        Some(Err(e)) => {
            set_error(e.to_string());
            FFI_ERROR
        }
        None => {
            set_error("Not initialized");
            FFI_NOT_INITIALIZED
        }
    }
}

/// Get playback status (async, non-blocking)
#[no_mangle]
pub extern "C" fn fling_get_status_async(
    device: *const c_char,
    callback: FFIStatusCallback,
    context: *mut c_void,
) {
    let device_name = match unsafe { c_str_to_string(device) } {
        Some(d) => d,
        None => {
            set_error("Invalid device name");
            callback(ptr::null_mut(), context);
            return;
        }
    };

    let ctx_ptr = context as usize;

    let rt = match get_runtime() {
        Some(rt) => rt,
        None => {
            set_error("Not initialized");
            callback(ptr::null_mut(), context);
            return;
        }
    };

    let combo_arc = match get_combo_arc() {
        Some(arc) => arc,
        None => {
            set_error("Not initialized");
            callback(ptr::null_mut(), context);
            return;
        }
    };

    rt.spawn(async move {
        let guard = combo_arc.read().await;
        let combo = match guard.as_ref() {
            Some(c) => c,
            None => {
                set_error("Not initialized");
                callback(ptr::null_mut(), ctx_ptr as *mut c_void);
                return;
            }
        };

        // Find renderer
        let renderer = match combo.find_renderer(&device_name).await {
            Ok(Some(r)) => r,
            _ => {
                set_error("Device not found");
                callback(ptr::null_mut(), ctx_ptr as *mut c_void);
                return;
            }
        };

        match combo.get_playback_info(&renderer).await {
            Ok(info) => {
                let state_str = format!("{:?}", info.state).to_uppercase();
                let status = Box::into_raw(Box::new(FFIPlaybackStatus {
                    state: string_to_c(&state_str),
                    position_secs: info.position.as_secs(),
                    duration_secs: info.duration.as_secs(),
                    current_uri: option_string_to_c(info.current_uri.as_deref()),
                }));
                callback(status, ctx_ptr as *mut c_void);
            }
            Err(e) => {
                set_error(format!("Failed to get status: {e}"));
                callback(ptr::null_mut(), ctx_ptr as *mut c_void);
            }
        }
    });
}

/// Play (async, non-blocking)
#[no_mangle]
pub extern "C" fn fling_play_async(
    device: *const c_char,
    callback: FFIResultCallback,
    context: *mut c_void,
) {
    ffi_device_action_async(device, callback, context, "resume");
}

/// Pause (async, non-blocking)
#[no_mangle]
pub extern "C" fn fling_pause_async(
    device: *const c_char,
    callback: FFIResultCallback,
    context: *mut c_void,
) {
    ffi_device_action_async(device, callback, context, "pause");
}

/// Stop (async, non-blocking)
#[no_mangle]
pub extern "C" fn fling_stop_async(
    device: *const c_char,
    callback: FFIResultCallback,
    context: *mut c_void,
) {
    ffi_device_action_async(device, callback, context, "stop");
}

/// Seek (async, non-blocking)
#[no_mangle]
pub extern "C" fn fling_seek_async(
    device: *const c_char,
    position_secs: u64,
    callback: FFIResultCallback,
    context: *mut c_void,
) {
    ffi_device_action_with_value_async(device, callback, context, "seek", position_secs as i64);
}

// =============================================================================
// Phase 3: Advanced Playback Features (Track Navigation & Play Mode)
// =============================================================================

/// Skip to next track (async, non-blocking)
///
/// Note: Most DLNA TVs don't support this action (only for multi-track media like playlists)
/// Use app-level queue management as fallback
#[no_mangle]
pub extern "C" fn fling_next_track_async(
    device: *const c_char,
    callback: FFIResultCallback,
    context: *mut c_void,
) {
    ffi_device_action_async(device, callback, context, "next_track");
}

/// Skip to previous track (async, non-blocking)
///
/// Note: Most DLNA TVs don't support this action (only for multi-track media like playlists)
/// Use app-level queue management as fallback
#[no_mangle]
pub extern "C" fn fling_previous_track_async(
    device: *const c_char,
    callback: FFIResultCallback,
    context: *mut c_void,
) {
    ffi_device_action_async(device, callback, context, "previous_track");
}

/// Get current play mode from device (async, non-blocking)
///
/// Returns mode string: "NORMAL", "SHUFFLE", "REPEAT_ONE", "REPEAT_ALL"
/// Note: Many devices don't support this action or always return "NORMAL"
#[no_mangle]
pub extern "C" fn fling_get_play_mode_async(
    device: *const c_char,
    callback: crate::device_actions::FFIPlayModeCallback,
    context: *mut c_void,
) {
    crate::device_actions::ffi_get_play_mode_async(device, callback, context);
}

/// Set play mode (shuffle/repeat) (async, non-blocking)
///
/// # Arguments
/// * `mode` - Play mode: "NORMAL", "SHUFFLE", "REPEAT_ONE", "REPEAT_ALL"
///
/// Note: Many devices don't support this action. Use graceful degradation:
/// - If this fails with UPnP error 401 (Invalid Action), implement shuffle/repeat in app
#[no_mangle]
pub extern "C" fn fling_set_play_mode_async(
    device: *const c_char,
    mode: *const c_char,
    callback: FFIResultCallback,
    context: *mut c_void,
) {
    crate::device_actions::ffi_set_play_mode_async(device, mode, callback, context);
}
