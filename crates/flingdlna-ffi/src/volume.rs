use std::ffi::{c_char, c_void};
use std::ptr;

use crate::device::find_renderer_by_name;
use crate::device_actions::ffi_device_action_with_value_async;
use crate::helpers::c_str_to_string;
use crate::state::{get_combo_arc, get_runtime, get_runtime_and_combo, set_error};
use crate::types::{
    FFIResult, FFIResultCallback, FFIVolumeCallback, FFIVolumeInfo, FFI_ERROR, FFI_NOT_FOUND,
    FFI_NOT_INITIALIZED, FFI_OK,
};

// =============================================================================
// Volume Control Functions
// =============================================================================

/// Get volume for a device
#[no_mangle]
pub extern "C" fn fling_get_volume(device: *const c_char) -> *mut FFIVolumeInfo {
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

    let result = rt.block_on(async {
        let guard = arc.read().await;
        match guard.as_ref() {
            Some(combo) => {
                let ctrl = combo.controller();
                let volume = ctrl.get_volume(&renderer).await.ok();
                let muted = ctrl.get_mute(&renderer).await.unwrap_or(false);
                volume.map(|v| (v, muted))
            }
            None => None,
        }
    });

    match result {
        Some((volume, muted)) => Box::into_raw(Box::new(FFIVolumeInfo { volume, muted })),
        None => {
            set_error("Failed to get volume");
            ptr::null_mut()
        }
    }
}

/// Free volume info
#[no_mangle]
pub extern "C" fn fling_free_volume(info: *mut FFIVolumeInfo) {
    if !info.is_null() {
        unsafe {
            drop(Box::from_raw(info));
        }
    }
}

/// Set volume for a device
#[no_mangle]
pub extern "C" fn fling_set_volume(device: *const c_char, volume: u8) -> FFIResult {
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
            Some(combo) => Some(combo.controller().set_volume(&renderer, volume).await),
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

/// Set mute state for a device
#[no_mangle]
pub extern "C" fn fling_set_mute(device: *const c_char, mute: bool) -> FFIResult {
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
            Some(combo) => Some(combo.controller().set_mute(&renderer, mute).await),
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

/// Toggle mute state for a device
#[no_mangle]
pub extern "C" fn fling_toggle_mute(device: *const c_char) -> FFIResult {
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

    // Get current mute state and toggle it
    let result = rt.block_on(async {
        let guard = arc.read().await;
        match guard.as_ref() {
            Some(combo) => {
                let ctrl = combo.controller();
                let current_mute = ctrl.get_mute(&renderer).await.unwrap_or(false);
                Some(ctrl.set_mute(&renderer, !current_mute).await)
            }
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

/// Get volume (async, non-blocking)
#[no_mangle]
pub extern "C" fn fling_get_volume_async(
    device: *const c_char,
    callback: FFIVolumeCallback,
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

        let renderer = match combo.find_renderer(&device_name).await {
            Ok(Some(r)) => r,
            _ => {
                set_error("Device not found");
                callback(ptr::null_mut(), ctx_ptr as *mut c_void);
                return;
            }
        };

        let ctrl = combo.controller();
        let volume = ctrl.get_volume(&renderer).await.ok();
        let muted = ctrl.get_mute(&renderer).await.unwrap_or(false);

        match volume {
            Some(v) => {
                let info = Box::into_raw(Box::new(FFIVolumeInfo { volume: v, muted }));
                callback(info, ctx_ptr as *mut c_void);
            }
            None => {
                set_error("Failed to get volume");
                callback(ptr::null_mut(), ctx_ptr as *mut c_void);
            }
        }
    });
}

/// Set volume (async, non-blocking)
#[no_mangle]
pub extern "C" fn fling_set_volume_async(
    device: *const c_char,
    volume: u8,
    callback: FFIResultCallback,
    context: *mut c_void,
) {
    ffi_device_action_with_value_async(device, callback, context, "set_volume", volume as i64);
}

/// Set mute (async, non-blocking)
#[no_mangle]
pub extern "C" fn fling_set_mute_async(
    device: *const c_char,
    mute: bool,
    callback: FFIResultCallback,
    context: *mut c_void,
) {
    ffi_device_action_with_value_async(
        device,
        callback,
        context,
        "set_mute",
        if mute { 1 } else { 0 },
    );
}
