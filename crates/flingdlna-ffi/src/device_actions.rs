use std::ffi::{c_char, c_void};
use std::time::Duration;

use crate::helpers::c_str_to_string;
use crate::state::{get_combo_arc, get_runtime, set_error};
use crate::types::{
    FFIDeviceCapabilities, FFIDeviceCapabilitiesCallback, FFIMediaInfo, FFIMediaInfoCallback,
    FFIResultCallback, FFI_ERROR, FFI_NOT_FOUND, FFI_NOT_INITIALIZED, FFI_OK,
};

// Helper function for simple device actions (resume, pause, stop)
pub(crate) fn ffi_device_action_async(
    device: *const c_char,
    callback: FFIResultCallback,
    context: *mut c_void,
    action: &'static str,
) {
    let device_name = match unsafe { c_str_to_string(device) } {
        Some(d) => d,
        None => {
            set_error("Invalid device name");
            callback(FFI_ERROR, context);
            return;
        }
    };

    let ctx_ptr = context as usize;

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

    rt.spawn(async move {
        let guard = combo_arc.read().await;
        let combo = match guard.as_ref() {
            Some(c) => c,
            None => {
                set_error("Not initialized");
                callback(FFI_NOT_INITIALIZED, ctx_ptr as *mut c_void);
                return;
            }
        };

        let renderer = match combo.find_renderer(&device_name).await {
            Ok(Some(r)) => r,
            _ => {
                set_error("Device not found");
                callback(FFI_NOT_FOUND, ctx_ptr as *mut c_void);
                return;
            }
        };

        let result = match action {
            "resume" => combo.controller().resume(&renderer).await,
            "pause" => combo.pause(&renderer).await,
            "stop" => combo.stop(&renderer).await,
            "next_track" => combo.controller().next_track(&renderer).await,
            "previous_track" => combo.controller().previous_track(&renderer).await,
            _ => {
                set_error("Unknown action");
                callback(FFI_ERROR, ctx_ptr as *mut c_void);
                return;
            }
        };

        match result {
            Ok(()) => callback(FFI_OK, ctx_ptr as *mut c_void),
            Err(e) => {
                set_error(e.to_string());
                callback(FFI_ERROR, ctx_ptr as *mut c_void);
            }
        }
    });
}

// Helper function for device actions with a value (set_volume, set_mute, seek)
pub(crate) fn ffi_device_action_with_value_async(
    device: *const c_char,
    callback: FFIResultCallback,
    context: *mut c_void,
    action: &'static str,
    value: i64,
) {
    let device_name = match unsafe { c_str_to_string(device) } {
        Some(d) => d,
        None => {
            set_error("Invalid device name");
            callback(FFI_ERROR, context);
            return;
        }
    };

    let ctx_ptr = context as usize;

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

    rt.spawn(async move {
        let guard = combo_arc.read().await;
        let combo = match guard.as_ref() {
            Some(c) => c,
            None => {
                set_error("Not initialized");
                callback(FFI_NOT_INITIALIZED, ctx_ptr as *mut c_void);
                return;
            }
        };

        let renderer = match combo.find_renderer(&device_name).await {
            Ok(Some(r)) => r,
            _ => {
                set_error("Device not found");
                callback(FFI_NOT_FOUND, ctx_ptr as *mut c_void);
                return;
            }
        };

        let result = match action {
            "set_volume" => combo.controller().set_volume(&renderer, value as u8).await,
            "set_mute" => combo.controller().set_mute(&renderer, value != 0).await,
            "seek" => {
                combo
                    .seek(&renderer, Duration::from_secs(value as u64))
                    .await
            }
            _ => {
                set_error("Unknown action");
                callback(FFI_ERROR, ctx_ptr as *mut c_void);
                return;
            }
        };

        match result {
            Ok(()) => callback(FFI_OK, ctx_ptr as *mut c_void),
            Err(e) => {
                set_error(e.to_string());
                callback(FFI_ERROR, ctx_ptr as *mut c_void);
            }
        }
    });
}
/// Get media information (duration, metadata) from device
pub(crate) fn ffi_get_media_info_async(
    device: *const c_char,
    callback: FFIMediaInfoCallback,
    context: *mut c_void,
) {
    let device_name = match unsafe { c_str_to_string(device) } {
        Some(d) => d,
        None => {
            set_error("Invalid device name");
            callback(std::ptr::null_mut(), context);
            return;
        }
    };

    let ctx_ptr = context as usize;

    let rt = match get_runtime() {
        Some(rt) => rt,
        None => {
            set_error("Not initialized");
            callback(std::ptr::null_mut(), context);
            return;
        }
    };

    let combo_arc = match get_combo_arc() {
        Some(arc) => arc,
        None => {
            set_error("Not initialized");
            callback(std::ptr::null_mut(), context);
            return;
        }
    };

    rt.spawn(async move {
        let guard = combo_arc.read().await;
        let combo = match guard.as_ref() {
            Some(c) => c,
            None => {
                set_error("Not initialized");
                callback(std::ptr::null_mut(), ctx_ptr as *mut c_void);
                return;
            }
        };

        let renderer = match combo.find_renderer(&device_name).await {
            Ok(Some(r)) => r,
            _ => {
                set_error("Device not found");
                callback(std::ptr::null_mut(), ctx_ptr as *mut c_void);
                return;
            }
        };

        match combo.controller().get_media_info(&renderer).await {
            Ok(info) => {
                let ffi_info = Box::new(FFIMediaInfo::from_media_info(&info));
                callback(Box::into_raw(ffi_info), ctx_ptr as *mut c_void);
            }
            Err(e) => {
                set_error(e.to_string());
                callback(std::ptr::null_mut(), ctx_ptr as *mut c_void);
            }
        }
    });
}

/// Get device capabilities (supported media formats)
pub(crate) fn ffi_get_device_capabilities_async(
    device: *const c_char,
    callback: FFIDeviceCapabilitiesCallback,
    context: *mut c_void,
) {
    let device_name = match unsafe { c_str_to_string(device) } {
        Some(d) => d,
        None => {
            set_error("Invalid device name");
            callback(std::ptr::null_mut(), context);
            return;
        }
    };

    let ctx_ptr = context as usize;

    let rt = match get_runtime() {
        Some(rt) => rt,
        None => {
            set_error("Not initialized");
            callback(std::ptr::null_mut(), context);
            return;
        }
    };

    let combo_arc = match get_combo_arc() {
        Some(arc) => arc,
        None => {
            set_error("Not initialized");
            callback(std::ptr::null_mut(), context);
            return;
        }
    };

    rt.spawn(async move {
        let guard = combo_arc.read().await;
        let combo = match guard.as_ref() {
            Some(c) => c,
            None => {
                set_error("Not initialized");
                callback(std::ptr::null_mut(), ctx_ptr as *mut c_void);
                return;
            }
        };

        let renderer = match combo.find_renderer(&device_name).await {
            Ok(Some(r)) => r,
            _ => {
                set_error("Device not found");
                callback(std::ptr::null_mut(), ctx_ptr as *mut c_void);
                return;
            }
        };

        match combo.controller().get_device_capabilities(&renderer).await {
            Ok(caps) => {
                let ffi_caps = Box::new(FFIDeviceCapabilities::from_device_capabilities(&caps));
                callback(Box::into_raw(ffi_caps), ctx_ptr as *mut c_void);
            }
            Err(e) => {
                set_error(e.to_string());
                callback(std::ptr::null_mut(), ctx_ptr as *mut c_void);
            }
        }
    });
}

// =============================================================================
// Phase 3: Advanced Playback Features
// =============================================================================

/// Callback type for get_play_mode (returns mode string: NORMAL, SHUFFLE, REPEAT_ONE, REPEAT_ALL)
pub type FFIPlayModeCallback = extern "C" fn(mode: *mut c_char, context: *mut c_void);

/// Get current play mode from device (NORMAL, SHUFFLE, REPEAT_ONE, REPEAT_ALL)
///
/// Note: Many devices don't support this action or always return "NORMAL"
pub(crate) fn ffi_get_play_mode_async(
    device: *const c_char,
    callback: FFIPlayModeCallback,
    context: *mut c_void,
) {
    let device_name = match unsafe { c_str_to_string(device) } {
        Some(d) => d,
        None => {
            set_error("Invalid device name");
            callback(std::ptr::null_mut(), context);
            return;
        }
    };

    let ctx_ptr = context as usize;

    let rt = match get_runtime() {
        Some(rt) => rt,
        None => {
            set_error("Not initialized");
            callback(std::ptr::null_mut(), context);
            return;
        }
    };

    let combo_arc = match get_combo_arc() {
        Some(arc) => arc,
        None => {
            set_error("Not initialized");
            callback(std::ptr::null_mut(), context);
            return;
        }
    };

    rt.spawn(async move {
        let guard = combo_arc.read().await;
        let combo = match guard.as_ref() {
            Some(c) => c,
            None => {
                set_error("Not initialized");
                callback(std::ptr::null_mut(), ctx_ptr as *mut c_void);
                return;
            }
        };

        let renderer = match combo.find_renderer(&device_name).await {
            Ok(Some(r)) => r,
            _ => {
                set_error("Device not found");
                callback(std::ptr::null_mut(), ctx_ptr as *mut c_void);
                return;
            }
        };

        match combo.controller().get_play_mode(&renderer).await {
            Ok(mode) => {
                let mode_c = crate::helpers::string_to_c(&mode);
                callback(mode_c, ctx_ptr as *mut c_void);
            }
            Err(e) => {
                set_error(e.to_string());
                callback(std::ptr::null_mut(), ctx_ptr as *mut c_void);
            }
        }
    });
}

/// Set play mode (shuffle/repeat)
///
/// # Arguments
/// * `mode` - Play mode: "NORMAL", "SHUFFLE", "REPEAT_ONE", "REPEAT_ALL"
///
/// Note: Many devices don't support this action. Use graceful degradation:
/// - If this fails with UPnP error 401 (Invalid Action), implement shuffle/repeat in app
pub(crate) fn ffi_set_play_mode_async(
    device: *const c_char,
    mode: *const c_char,
    callback: FFIResultCallback,
    context: *mut c_void,
) {
    let device_name = match unsafe { c_str_to_string(device) } {
        Some(d) => d,
        None => {
            set_error("Invalid device name");
            callback(FFI_ERROR, context);
            return;
        }
    };

    let mode_str = match unsafe { c_str_to_string(mode) } {
        Some(m) => m,
        None => {
            set_error("Invalid mode");
            callback(FFI_ERROR, context);
            return;
        }
    };

    let ctx_ptr = context as usize;

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

    rt.spawn(async move {
        let guard = combo_arc.read().await;
        let combo = match guard.as_ref() {
            Some(c) => c,
            None => {
                set_error("Not initialized");
                callback(FFI_NOT_INITIALIZED, ctx_ptr as *mut c_void);
                return;
            }
        };

        let renderer = match combo.find_renderer(&device_name).await {
            Ok(Some(r)) => r,
            _ => {
                set_error("Device not found");
                callback(FFI_NOT_FOUND, ctx_ptr as *mut c_void);
                return;
            }
        };

        match combo.controller().set_play_mode(&renderer, &mode_str).await {
            Ok(()) => callback(FFI_OK, ctx_ptr as *mut c_void),
            Err(e) => {
                set_error(e.to_string());
                callback(FFI_ERROR, ctx_ptr as *mut c_void);
            }
        }
    });
}
