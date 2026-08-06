//! Session Event callback support for FFI
//!
//! This module provides FFI functions for subscribing to session events from all renderer types
//! (DLNA + Chromecast). Events are delivered via callback to Swift/C code for real-time UI updates.

use std::ffi::c_void;
use std::sync::OnceLock;

use parking_lot::RwLock;

use crate::types::{FFISessionEvent, FFISessionEventCallback};

/// Storage for session event callback and context
struct SessionEventCallbackStorage {
    callback: FFISessionEventCallback,
    context: *mut c_void,
}

// SAFETY: We only access this from the callback thread, and Swift manages the context lifetime
unsafe impl Send for SessionEventCallbackStorage {}
unsafe impl Sync for SessionEventCallbackStorage {}

static SESSION_EVENT_CALLBACK: OnceLock<RwLock<Option<SessionEventCallbackStorage>>> =
    OnceLock::new();

/// Subscribe to session events from all renderer types (DLNA + Chromecast)
///
/// # Arguments
/// * `callback` - Function to call when events are received
/// * `context` - User context pointer passed to callback
///
/// # Example (Swift)
/// ```swift
/// let context = Unmanaged.passUnretained(self).toOpaque()
/// fling_subscribe_session_events(sessionEventCallback, context)
/// ```
#[no_mangle]
pub extern "C" fn fling_subscribe_session_events(
    callback: FFISessionEventCallback,
    context: *mut c_void,
) {
    let storage = SessionEventCallbackStorage { callback, context };

    SESSION_EVENT_CALLBACK
        .get_or_init(|| RwLock::new(None))
        .write()
        .replace(storage);

    tracing::debug!("Session event callback registered");

    // Start session event forwarding task
    start_session_event_forwarding();
}

/// Unsubscribe from session events
#[no_mangle]
pub extern "C" fn fling_unsubscribe_session_events() {
    if let Some(lock) = SESSION_EVENT_CALLBACK.get() {
        lock.write().take();
        tracing::debug!("Session event callback unregistered");
    }
}

/// Free a session event structure
///
/// # Safety
/// The event pointer must be valid and must have been created by this library
#[no_mangle]
pub unsafe extern "C" fn fling_free_session_event(event: *mut FFISessionEvent) {
    if !event.is_null() {
        let mut boxed = Box::from_raw(event);
        boxed.free();
    }
}

/// Internal function to invoke the registered callback with a session event
///
/// This is called from the session manager when a session event is received
pub(crate) fn invoke_session_event_callback(event: &FFISessionEvent) {
    if let Some(lock) = SESSION_EVENT_CALLBACK.get() {
        if let Some(storage) = lock.read().as_ref() {
            // Call the registered callback
            (storage.callback)(event as *const FFISessionEvent, storage.context);
        }
    }
}

/// Start forwarding session events from SessionManager to FFI callbacks
///
/// This should be called after both SessionManager and the FFI callback are registered.
/// It spawns a task that listens to SessionManager events and forwards them to Swift.
pub(crate) fn start_session_event_forwarding() {
    use crate::state::{combo_ref, get_runtime};
    use crate::types::FFISessionEvent;

    let Some(rt) = get_runtime() else {
        tracing::warn!("Cannot start session event forwarding: runtime not initialized");
        return;
    };

    rt.spawn(async move {
        tracing::info!("Session event forwarding task started");

        // Get the session manager and subscribe to all events
        let mut event_rx = {
            let Some(combo_arc) = combo_ref() else {
                tracing::warn!("Combo not initialized, session event forwarding disabled");
                return;
            };
            let guard = combo_arc.read().await;
            let Some(combo) = guard.as_ref() else {
                tracing::warn!("Combo not initialized, session event forwarding disabled");
                return;
            };
            combo.controller().session_manager().subscribe_all_events()
        };

        // Forward events to FFI callback
        loop {
            match event_rx.recv().await {
                Ok((device_id, session_event)) => {
                    tracing::debug!(
                        "Received session event from {}: {:?}",
                        device_id,
                        session_event
                    );

                    // Convert to FFI format
                    let ffi_event = FFISessionEvent::from_session_event(&session_event, &device_id);

                    // Invoke callback
                    invoke_session_event_callback(&ffi_event);

                    // Note: We don't free ffi_event here because we're not passing ownership
                    // The callback receives a pointer but doesn't own it
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!("Session event receiver lagged, {} events skipped", skipped);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    tracing::info!("Session event channel closed, stopping forwarding");
                    break;
                }
            }
        }
    });
}
