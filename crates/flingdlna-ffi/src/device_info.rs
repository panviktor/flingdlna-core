use std::ffi::{c_char, c_void};

use crate::device_actions::{ffi_get_device_capabilities_async, ffi_get_media_info_async};
use crate::types::{
    FFIDeviceCapabilities, FFIDeviceCapabilitiesCallback, FFIMediaInfo, FFIMediaInfoCallback,
};

// =============================================================================
// Device Information Functions
// =============================================================================

/// Get media information (duration, metadata) from device (async)
///
/// # Arguments
/// * `device` - Device name or location
/// * `callback` - Callback function called when info is retrieved
/// * `context` - User context pointer passed to callback
#[no_mangle]
pub extern "C" fn fling_get_media_info_async(
    device: *const c_char,
    callback: FFIMediaInfoCallback,
    context: *mut c_void,
) {
    ffi_get_media_info_async(device, callback, context);
}

/// Free media info structure
#[no_mangle]
pub extern "C" fn fling_free_media_info(info: *mut FFIMediaInfo) {
    if info.is_null() {
        return;
    }
    unsafe {
        let mut boxed = Box::from_raw(info);
        boxed.free();
    }
}

/// Get device capabilities (supported media formats) from device (async)
///
/// # Arguments
/// * `device` - Device name or location
/// * `callback` - Callback function called when capabilities are retrieved
/// * `context` - User context pointer passed to callback
#[no_mangle]
pub extern "C" fn fling_get_device_capabilities_async(
    device: *const c_char,
    callback: FFIDeviceCapabilitiesCallback,
    context: *mut c_void,
) {
    ffi_get_device_capabilities_async(device, callback, context);
}

/// Free device capabilities structure
#[no_mangle]
pub extern "C" fn fling_free_device_capabilities(caps: *mut FFIDeviceCapabilities) {
    if caps.is_null() {
        return;
    }
    unsafe {
        let mut boxed = Box::from_raw(caps);
        boxed.free();
    }
}
