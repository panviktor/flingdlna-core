use std::ffi::{c_char, c_void};
use std::path::PathBuf;
use std::sync::Arc;

use dlna_combo::{config::ComboConfig, ControllerConfig, DlnaCombo, ServerConfig};
use dlna_controller::EventManager;

use crate::helpers::c_str_to_string;
use crate::logging::{build_time, init_logging};
use crate::state::{
    combo_ref, get_combo_arc, get_or_create_runtime, get_or_init_combo, get_or_init_event_manager,
    get_runtime, set_error,
};
use crate::types::{FFIResult, FFIResultCallback, FFI_ERROR, FFI_OK};

// =============================================================================
// Lifecycle Functions
// =============================================================================

/// Initialize the flingdlna library
///
/// # Arguments
/// * `serve_dir` - Optional path to media directory (can be NULL)
///
/// # Returns
/// FFI_OK on success, error code otherwise
#[no_mangle]
pub extern "C" fn fling_init(serve_dir: *const c_char) -> FFIResult {
    // Initialize logging (debug builds show debug logs, release builds show info+)
    init_logging();

    tracing::info!("FFI: Initializing flingdlna library");
    tracing::info!("FFI: build time: {}", build_time());

    // Get or create tokio runtime (reused across reinitializations)
    let runtime = get_or_create_runtime();

    // Parse serve_dir
    let serve_dir_path = unsafe { c_str_to_string(serve_dir) }.map(PathBuf::from);

    // Build config - always create server (directories can be added later)
    let directories = serve_dir_path.map(|d| vec![d]).unwrap_or_default();
    let server_config = ServerConfig {
        http_port: 0, // Auto-select
        directories,
        manufacturer: "FlingDLNA".to_string(),
        model_name: "FFI Library".to_string(),
        ..ServerConfig::new_with_hostname()
    };
    let combo_config = ComboConfig {
        server: Some(server_config),
        controller: ControllerConfig::new(),
    };

    // Create DlnaCombo
    let combo = match runtime.block_on(DlnaCombo::new(combo_config)) {
        Ok(c) => c,
        Err(e) => {
            set_error(format!("Failed to create DlnaCombo: {e}"));
            return FFI_ERROR;
        }
    };

    // Start server if configured
    let mut combo = combo;
    if combo.server().is_some() {
        if let Err(e) = runtime.block_on(combo.start_server()) {
            set_error(format!("Failed to start server: {e}"));
            return FFI_ERROR;
        }
    }

    // Store combo (initialize Arc once, then update the Option inside)
    let combo_arc = get_or_init_combo();
    runtime.block_on(async {
        *combo_arc.write().await = Some(combo);
    });

    // Create EventManager for UPnP events
    let event_manager_opt = match runtime.block_on(EventManager::new(7677)) {
        Ok(mgr) => {
            let arc = Arc::new(mgr);
            // Store in global STATE
            let event_manager_arc = get_or_init_event_manager();
            runtime.block_on(async {
                *event_manager_arc.write().await = Some(Arc::clone(&arc));
            });
            tracing::info!("EventManager initialized successfully");
            Some(arc)
        }
        Err(e) => {
            tracing::warn!(
                "Failed to create EventManager: {}. Events will not be available.",
                e
            );
            None
        }
    };

    // Attach EventManager to controller session manager
    if let Some(ref event_manager) = event_manager_opt {
        runtime.block_on(async {
            let guard = combo_arc.read().await;
            if let Some(combo) = guard.as_ref() {
                combo
                    .controller()
                    .set_event_manager(Some(Arc::clone(event_manager)))
                    .await;
                tracing::info!("Controller session manager attached to EventManager");
            }
        });
    }

    FFI_OK
}

/// Shutdown the flingdlna library
#[no_mangle]
pub extern "C" fn fling_shutdown() -> FFIResult {
    if let Some(rt) = get_runtime() {
        rt.block_on(async {
            // Stop combo (and sessions via controller)
            if let Some(combo_arc) = combo_ref() {
                let mut guard = combo_arc.write().await;
                if let Some(mut combo) = guard.take() {
                    if let Err(e) = combo
                        .controller()
                        .session_manager()
                        .close_all_sessions()
                        .await
                    {
                        tracing::warn!("Error closing sessions: {}", e);
                    }
                    combo.stop_server().await;
                }
            }
        });
    }
    FFI_OK
}

/// Check if library is initialized and running
#[no_mangle]
pub extern "C" fn fling_is_running() -> bool {
    if let (Some(arc), Some(rt)) = (combo_ref(), get_runtime()) {
        rt.block_on(async { arc.read().await.is_some() })
    } else {
        false
    }
}

// =============================================================================
// Async (Non-Blocking) API
// =============================================================================

/// Initialize the library (async, non-blocking)
///
/// # Arguments
/// * `serve_dir` - Media directory path (NULL for none)
/// * `server_name` - Server name (NULL for hostname-based default)
/// * `callback` - Callback function called when initialization completes
/// * `context` - User context pointer passed to callback
#[no_mangle]
pub extern "C" fn fling_init_async(
    serve_dir: *const c_char,
    server_name: *const c_char,
    callback: FFIResultCallback,
    context: *mut c_void,
) {
    let serve_dir_path = unsafe { c_str_to_string(serve_dir) }.map(PathBuf::from);
    let server_name_str = unsafe { c_str_to_string(server_name) };
    let ctx_ptr = context as usize;

    // Initialize logging (debug builds show debug logs, release builds show info+)
    init_logging();

    tracing::info!("FFI: Initializing flingdlna library (async)");
    tracing::info!("FFI: build time: {}", build_time());

    let runtime = get_or_create_runtime();

    runtime.spawn(async move {
        // Build config
        let directories = serve_dir_path.map(|d| vec![d]).unwrap_or_default();
        let base_config = match server_name_str {
            Some(name) => ServerConfig::new(name),
            None => ServerConfig::new_with_hostname(),
        };
        let server_config = ServerConfig {
            http_port: 0,
            directories,
            manufacturer: "FlingDLNA".to_string(),
            model_name: "FFI Library".to_string(),
            ..base_config
        };
        let combo_config = ComboConfig {
            server: Some(server_config),
            controller: ControllerConfig::new(),
        };

        // Create DlnaCombo
        let result = match DlnaCombo::new(combo_config).await {
            Ok(mut combo) => {
                // Start server if configured
                if combo.server().is_some() {
                    if let Err(e) = combo.start_server().await {
                        set_error(format!("Failed to start server: {e}"));
                        FFI_ERROR
                    } else {
                        // Store combo
                        let combo_arc = get_or_init_combo();
                        *combo_arc.write().await = Some(combo);

                        // Create EventManager for UPnP events
                        let event_mgr_opt = if let Ok(event_manager) = EventManager::new(7677).await
                        {
                            let arc = Arc::new(event_manager);
                            let event_manager_arc = get_or_init_event_manager();
                            *event_manager_arc.write().await = Some(Arc::clone(&arc));
                            tracing::info!("EventManager initialized successfully");
                            Some(arc)
                        } else {
                            tracing::warn!(
                                "Failed to create EventManager. Events will not be available."
                            );
                            None
                        };

                        if let Some(ref event_manager) = event_mgr_opt {
                            let guard = combo_arc.read().await;
                            if let Some(combo) = guard.as_ref() {
                                combo
                                    .controller()
                                    .set_event_manager(Some(Arc::clone(event_manager)))
                                    .await;
                                tracing::info!(
                                    "Controller session manager attached to EventManager"
                                );
                            }
                        }

                        FFI_OK
                    }
                } else {
                    let combo_arc = get_or_init_combo();
                    *combo_arc.write().await = Some(combo);

                    // Create EventManager for UPnP events
                    let event_mgr_opt = if let Ok(event_manager) = EventManager::new(7677).await {
                        let arc = Arc::new(event_manager);
                        let event_manager_arc = get_or_init_event_manager();
                        *event_manager_arc.write().await = Some(Arc::clone(&arc));
                        tracing::info!("EventManager initialized successfully");
                        Some(arc)
                    } else {
                        tracing::warn!(
                            "Failed to create EventManager. Events will not be available."
                        );
                        None
                    };

                    if let Some(ref event_manager) = event_mgr_opt {
                        let guard = combo_arc.read().await;
                        if let Some(combo) = guard.as_ref() {
                            combo
                                .controller()
                                .set_event_manager(Some(Arc::clone(event_manager)))
                                .await;
                            tracing::info!("Controller session manager attached to EventManager");
                        }
                    }

                    FFI_OK
                }
            }
            Err(e) => {
                set_error(format!("Failed to create DlnaCombo: {e}"));
                FFI_ERROR
            }
        };

        callback(result, ctx_ptr as *mut c_void);
    });
}

/// Shutdown the library (async, non-blocking)
#[no_mangle]
pub extern "C" fn fling_shutdown_async(callback: FFIResultCallback, context: *mut c_void) {
    let ctx_ptr = context as usize;

    if let Some(rt) = get_runtime() {
        rt.spawn(async move {
            // Take combo out of the Arc
            if let Some(arc) = get_combo_arc() {
                let combo_opt = arc.write().await.take();
                if let Some(mut combo) = combo_opt {
                    if let Err(e) = combo
                        .controller()
                        .session_manager()
                        .close_all_sessions()
                        .await
                    {
                        tracing::warn!("Error closing sessions: {}", e);
                    }
                    combo.stop_server().await;
                }
            }
            callback(FFI_OK, ctx_ptr as *mut c_void);
        });
    } else {
        callback(FFI_OK, context);
    }
}
