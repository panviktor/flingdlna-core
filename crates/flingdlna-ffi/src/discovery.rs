use std::ffi::c_void;
use std::ptr;
use std::sync::Arc;
use std::time::Duration;

use dlna_combo::Renderer;

use crate::state::{get_combo_arc, get_runtime, get_runtime_and_combo, set_error};
use crate::types::{FFIRenderer, FFIRendererCallback, FFIRendererList, FFIRendererListCallback};

// =============================================================================
// Discovery Functions
// =============================================================================

/// List available renderers
///
/// # Arguments
/// * `timeout_secs` - Discovery timeout in seconds (ignored, uses default)
///
/// # Returns
/// Pointer to FFIRendererList (must be freed with fling_free_renderers)
#[no_mangle]
pub extern "C" fn fling_list_renderers(timeout_secs: u64) -> *mut FFIRendererList {
    let timeout = Duration::from_secs(timeout_secs.clamp(30, 120));
    tracing::info!("FFI: Starting device discovery (timeout: {:?})", timeout);

    let (rt, arc) = match get_runtime_and_combo() {
        Some(x) => x,
        None => {
            set_error("Library not initialized");
            return ptr::null_mut();
        }
    };

    let renderers: Vec<Renderer> = rt.block_on(async {
        let guard = arc.read().await;
        match guard.as_ref() {
            Some(combo) => match combo.discover_renderers_with_timeout(timeout).await {
                Ok(r) => {
                    tracing::info!("FFI: Discovery returned {} devices", r.len());
                    r
                }
                Err(e) => {
                    let msg = format!("Discovery failed: {e}");
                    tracing::error!("FFI: {}", msg);
                    set_error(msg);
                    vec![]
                }
            },
            None => {
                tracing::error!("FFI: No combo available");
                set_error("Library not initialized");
                vec![]
            }
        }
    });

    let ffi_renderers: Vec<FFIRenderer> =
        renderers.iter().map(FFIRenderer::from_renderer).collect();

    let count = ffi_renderers.len();
    let items = if count > 0 {
        let boxed = ffi_renderers.into_boxed_slice();
        Box::into_raw(boxed) as *mut FFIRenderer
    } else {
        ptr::null_mut()
    };

    Box::into_raw(Box::new(FFIRendererList { items, count }))
}

/// Free single renderer (for progressive discovery)
#[no_mangle]
pub extern "C" fn fling_free_renderer(renderer: *mut FFIRenderer) {
    if renderer.is_null() {
        return;
    }

    unsafe {
        let mut renderer = Box::from_raw(renderer);
        renderer.free();
    }
}

/// Free renderer list
#[no_mangle]
pub extern "C" fn fling_free_renderers(list: *mut FFIRendererList) {
    if list.is_null() {
        return;
    }

    unsafe {
        let list = Box::from_raw(list);
        if !list.items.is_null() && list.count > 0 {
            let slice = std::slice::from_raw_parts_mut(list.items, list.count);
            for item in slice.iter_mut() {
                item.free();
            }
            drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                list.items, list.count,
            )));
        }
    }
}

/// List renderers (async, non-blocking)
#[no_mangle]
pub extern "C" fn fling_list_renderers_async(
    timeout_secs: u64,
    callback: FFIRendererListCallback,
    context: *mut c_void,
) {
    let ctx_ptr = context as usize;
    let timeout = Duration::from_secs(timeout_secs.clamp(30, 120));

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
        let renderers: Vec<Renderer> = {
            let guard = combo_arc.read().await;
            match guard.as_ref() {
                Some(combo) => match combo.discover_renderers_with_timeout(timeout).await {
                    Ok(r) => r,
                    Err(e) => {
                        set_error(format!("Discovery failed: {e}"));
                        vec![]
                    }
                },
                None => {
                    set_error("Not initialized");
                    vec![]
                }
            }
        };

        let ffi_renderers: Vec<FFIRenderer> =
            renderers.iter().map(FFIRenderer::from_renderer).collect();
        let count = ffi_renderers.len();
        let items = if count > 0 {
            let boxed = ffi_renderers.into_boxed_slice();
            Box::into_raw(boxed) as *mut FFIRenderer
        } else {
            ptr::null_mut()
        };

        let list = Box::into_raw(Box::new(FFIRendererList { items, count }));
        callback(list, ctx_ptr as *mut c_void);
    });
}

/// Discover renderers with progressive updates (async, non-blocking)
///
/// Calls the callback once for each discovered renderer as they are found,
/// then calls with NULL to signal completion.
///
/// # Arguments
/// * `timeout_secs` - Discovery timeout in seconds (clamped to 30-120)
/// * `callback` - Called for each renderer, then with NULL when done
/// * `context` - User data passed to callback
#[no_mangle]
pub extern "C" fn fling_discover_renderers_progressive(
    timeout_secs: u64,
    callback: FFIRendererCallback,
    context: *mut c_void,
) {
    let ctx_ptr = context as usize;
    let timeout = Duration::from_secs(timeout_secs.clamp(30, 120));

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
        use tokio::sync::mpsc;

        // Channel to receive devices as they're discovered
        let (tx, mut rx) = mpsc::channel::<Renderer>(100);

        // DLNA discovery task - clone controller to avoid holding lock
        let tx_dlna = tx.clone();
        let combo_arc_dlna = Arc::clone(combo_arc);
        let dlna_task = tokio::spawn(async move {
            // Briefly acquire lock to clone controller, then release
            let controller = {
                let guard = combo_arc_dlna.read().await;
                match guard.as_ref() {
                    Some(combo) => combo.controller().clone(),
                    None => return,
                }
                // Lock released here
            };

            // Run discovery WITHOUT holding the lock (allows scanning to proceed)
            match controller.discover_progressive(timeout, tx_dlna).await {
                Ok(renderers) => {
                    tracing::info!(
                        "DLNA discovery completed, found {} devices total",
                        renderers.len()
                    );
                }
                Err(e) => {
                    tracing::warn!("DLNA discovery failed: {}", e);
                    set_error(format!("DLNA discovery failed: {e}"));
                }
            }
        });

        // Chromecast discovery task (runs in parallel) - progressive discovery
        let cast_task = {
            let tx_cast = tx.clone();
            let combo_arc_cast = Arc::clone(combo_arc);
            tokio::spawn(async move {
                use dlna_controller::chromecast;

                // Briefly acquire lock to clone controller for caching, then release
                let controller = {
                    let guard = combo_arc_cast.read().await;
                    guard.as_ref().map(|combo| combo.controller().clone())
                    // Lock released here
                };

                // Create channel to receive Chromecast devices progressively
                let (cast_tx, mut cast_rx) =
                    tokio::sync::mpsc::channel::<chromecast::ChromecastDevice>(100);

                // Spawn task to convert ChromecastDevice -> Renderer and forward
                let tx_forward = tx_cast.clone();
                let controller_for_cache = controller.clone();
                let forward_task = tokio::spawn(async move {
                    let controller = controller_for_cache;
                    while let Some(device) = cast_rx.recv().await {
                        let cast_url = format!("cast://{}:{}", device.host, device.port);
                        if let Ok(location) = cast_url.parse() {
                            // Generate icon from mDNS 'ic' field if available
                            let icons = if let Some(ref icon_path) = device.icon_path {
                                let icon_url_str =
                                    format!("http://{}:8008{}", device.host, icon_path);
                                tracing::info!(
                                    "Chromecast FFI '{}': Generating icon from mDNS ic field: {}",
                                    device.name,
                                    icon_url_str
                                );
                                match icon_url_str.parse() {
                                    Ok(url) => {
                                        vec![dlna_core::types::DeviceIcon {
                                            mime_type: "image/png".to_string(),
                                            width: 0,  // Unknown from mDNS
                                            height: 0, // Unknown from mDNS
                                            depth: 0,  // Unknown from mDNS
                                            url,
                                        }]
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            "Chromecast FFI '{}': Invalid icon URL {}: {}",
                                            device.name,
                                            icon_url_str,
                                            e
                                        );
                                        Vec::new()
                                    }
                                }
                            } else {
                                tracing::info!(
                                    "Chromecast FFI '{}': No icon (ic field not present in mDNS)",
                                    device.name
                                );
                                Vec::new()
                            };

                            let renderer = dlna_core::Renderer {
                                friendly_name: device.name.clone(),
                                location,
                                udn: format!("chromecast:{}", device.id),
                                av_transport_url: None,
                                av_transport_service_type: None,
                                rendering_control_url: None,
                                rendering_control_service_type: None,
                                av_transport_event_url: None,
                                rendering_control_event_url: None,
                                manufacturer: Some("Google".to_string()),
                                model_name: device.model.clone(),
                                model_number: None, // Chromecast doesn't expose model number
                                model_description: None, // Chromecast doesn't expose model description
                                serial_number: None,     // Chromecast doesn't expose serial number
                                presentation_url: None,  // Chromecast doesn't have web UI
                                mac_address: None,
                                icons,
                                firmware_version: device.version.clone(), // From mDNS 've' field
                                capabilities: device.capabilities.clone(), // From mDNS 'ca' field
                            };

                            // Cache Chromecast device using cloned controller (no lock needed)
                            if let Some(ref ctrl) = controller {
                                ctrl.cache_renderer(renderer.clone()).await;
                            }

                            let _ = tx_forward.send(renderer).await;
                        }
                    }
                });

                tracing::info!("Starting Chromecast discovery (timeout: {:?})", timeout);
                match chromecast::discover_progressive(timeout, cast_tx).await {
                    Ok(devices) => {
                        tracing::info!(
                            "Chromecast discovery completed, found {} devices total",
                            devices.len()
                        );
                    }
                    Err(e) => {
                        tracing::warn!("Chromecast discovery failed: {}", e);
                    }
                }

                // Wait for forwarding to complete
                let _ = forward_task.await;
            })
        };

        // Drop the sender so channel closes when both tasks complete
        drop(tx);

        // Send devices to callback as they're discovered
        while let Some(renderer) = rx.recv().await {
            tracing::info!(
                "FFI: Discovered device '{}' with {} icon(s)",
                renderer.friendly_name,
                renderer.icons.len()
            );
            for (i, icon) in renderer.icons.iter().enumerate() {
                tracing::info!(
                    "  Icon {}: {}x{} {}bpp, {}, URL: {}",
                    i + 1,
                    icon.width,
                    icon.height,
                    icon.depth,
                    icon.mime_type,
                    icon.url
                );
            }

            let ffi_renderer = Box::into_raw(Box::new(FFIRenderer::from_renderer(&renderer)));
            callback(ffi_renderer, ctx_ptr as *mut c_void);
        }

        // Wait for both discovery tasks to complete
        let _ = dlna_task.await;
        let _ = cast_task.await;

        // Signal completion with NULL
        callback(ptr::null_mut(), ctx_ptr as *mut c_void);
    });
}
