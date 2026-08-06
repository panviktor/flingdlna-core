use std::ffi::c_char;

use crate::helpers::free_c_string;
use crate::state::last_error_ptr;

// =============================================================================
// Error Handling Functions
// =============================================================================

/// Get the last error message
#[no_mangle]
pub extern "C" fn fling_last_error() -> *const c_char {
    last_error_ptr()
}

/// Free a string allocated by the library
#[no_mangle]
pub extern "C" fn fling_free_string(s: *mut c_char) {
    unsafe {
        free_c_string(s);
    }
}
