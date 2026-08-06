use std::ffi::c_char;
use std::path::PathBuf;
use std::ptr;

use crate::device::find_renderer_by_name;
use crate::helpers::c_str_to_string;
use crate::state::{get_runtime_and_combo, set_error};
use crate::types::FFIPlayability;

/// Check whether a device is likely to play a URL request (best-effort)
#[no_mangle]
pub extern "C" fn fling_can_play_request(
    device: *const c_char,
    url: *const c_char,
    content_type: *const c_char,
    subtitle_url: *const c_char,
) -> *mut FFIPlayability {
    let device_name = match unsafe { c_str_to_string(device) } {
        Some(d) => d,
        None => {
            set_error("Invalid device name");
            return ptr::null_mut();
        }
    };

    let url_str = match unsafe { c_str_to_string(url) } {
        Some(u) => u,
        None => {
            set_error("Invalid URL");
            return ptr::null_mut();
        }
    };

    let content_type_str = unsafe { c_str_to_string(content_type) };
    let subtitle_url_str = unsafe { c_str_to_string(subtitle_url) };

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
                let request = dlna_combo::PlayRequest::new(&url_str)
                    .with_content_type(content_type_str.as_deref())
                    .with_subtitle_url(subtitle_url_str.as_deref());
                Some(
                    combo
                        .controller()
                        .assess_playability_for_request(&renderer, &request),
                )
            }
            None => None,
        }
    });

    match result {
        Some(report) => Box::into_raw(Box::new(FFIPlayability::from_report(&report))),
        None => {
            set_error("Not initialized");
            ptr::null_mut()
        }
    }
}

/// Check whether a device is likely to play a local file (best-effort)
#[no_mangle]
pub extern "C" fn fling_can_play_file(
    device: *const c_char,
    path: *const c_char,
) -> *mut FFIPlayability {
    let device_name = match unsafe { c_str_to_string(device) } {
        Some(d) => d,
        None => {
            set_error("Invalid device name");
            return ptr::null_mut();
        }
    };

    let path_str = match unsafe { c_str_to_string(path) } {
        Some(p) => p,
        None => {
            set_error("Invalid path");
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

    let path = PathBuf::from(path_str);
    let result = rt.block_on(async {
        let guard = arc.read().await;
        guard.as_ref().map(|combo| {
            combo
                .controller()
                .assess_playability_for_file(&renderer, &path)
        })
    });

    match result {
        Some(report) => Box::into_raw(Box::new(FFIPlayability::from_report(&report))),
        None => {
            set_error("Not initialized");
            ptr::null_mut()
        }
    }
}

/// Free playability info
#[no_mangle]
pub extern "C" fn fling_free_playability(info: *mut FFIPlayability) {
    if !info.is_null() {
        unsafe {
            let mut boxed = Box::from_raw(info);
            boxed.free();
        }
    }
}
