use std::ffi::{c_char, CStr, CString};
use std::ptr;

pub(crate) fn string_to_c(s: &str) -> *mut c_char {
    CString::new(s)
        .map(|cs| cs.into_raw())
        .unwrap_or(ptr::null_mut())
}

pub(crate) fn option_string_to_c(s: Option<&str>) -> *mut c_char {
    s.map(string_to_c).unwrap_or(ptr::null_mut())
}

pub(crate) unsafe fn free_c_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}

pub(crate) unsafe fn c_str_to_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    CStr::from_ptr(ptr).to_str().ok().map(String::from)
}
