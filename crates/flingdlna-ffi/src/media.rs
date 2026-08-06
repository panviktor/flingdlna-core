use std::ffi::{c_char, c_void};
use std::path::PathBuf;
use std::ptr;

use dlna_combo::MediaFile;
use serde::Deserialize;

use crate::device::find_renderer_by_name;
use crate::helpers::c_str_to_string;
use crate::state::{
    combo_ref, get_combo_arc, get_runtime, get_runtime_and_combo, set_error, with_combo_sync,
};
use crate::types::{
    FFIIntCallback, FFIMediaFile, FFIMediaList, FFIResult, FFIResultCallback, FFIScanProgress,
    FFIScanProgressCallback, FFI_ERROR, FFI_NOT_FOUND, FFI_NOT_INITIALIZED, FFI_OK,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueueSyncItem {
    url: Option<String>,
    media_id: Option<String>,
    content_type: Option<String>,
    subtitle_url: Option<String>,
    title: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueueSyncPayload {
    items: Vec<QueueSyncItem>,
    repeat_mode: Option<String>,
    shuffle: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum QueueSyncInput {
    Items(Vec<QueueSyncItem>),
    Payload(QueueSyncPayload),
}

fn parse_queue_payload(input: &str) -> Result<QueueSyncPayload, String> {
    let payload: QueueSyncInput =
        serde_json::from_str(input).map_err(|e| format!("Failed to parse queue JSON: {e}"))?;

    Ok(match payload {
        QueueSyncInput::Items(items) => QueueSyncPayload {
            items,
            repeat_mode: None,
            shuffle: None,
        },
        QueueSyncInput::Payload(payload) => payload,
    })
}

fn build_queue_requests(
    combo: &dlna_combo::DlnaCombo,
    items: &[QueueSyncItem],
) -> Result<Vec<dlna_combo::PlayRequest>, String> {
    let mut requests = Vec::with_capacity(items.len());

    for item in items {
        let url = if let Some(url) = item.url.as_deref() {
            url.to_string()
        } else if let Some(media_id) = item.media_id.as_deref() {
            if let Some(url) = media_id.strip_prefix("url:") {
                url.to_string()
            } else {
                let server = combo
                    .server()
                    .ok_or_else(|| "Server not configured".to_string())?;
                format!("{}/media/{}", server.base_url(), media_id)
            }
        } else {
            return Err("Queue item missing url or mediaId".to_string());
        };

        let mut content_type = item.content_type.clone();
        let mut title = item.title.clone();

        if let Some(media_id) = item.media_id.as_deref() {
            if !media_id.starts_with("url:") {
                if let Some(server) = combo.server() {
                    if let Some(file) = server.get_file(media_id) {
                        if content_type.is_none() {
                            content_type = Some(file.mime_type.clone());
                        }
                        if title.is_none() {
                            title = file.title.or(Some(file.filename));
                        }
                    } else {
                        return Err(format!("Media ID not found: {media_id}"));
                    }
                }
            }
        }

        let request = dlna_combo::PlayRequest::new(&url)
            .with_content_type(content_type.as_deref())
            .with_subtitle_url(item.subtitle_url.as_deref())
            .with_title(title.as_deref());

        requests.push(request);
    }

    Ok(requests)
}

fn map_repeat_mode(mode: Option<&str>, shuffle: bool) -> String {
    match mode.unwrap_or("none") {
        "one" => "REPEAT_SINGLE".to_string(),
        "all" => {
            if shuffle {
                "REPEAT_ALL_AND_SHUFFLE".to_string()
            } else {
                "REPEAT_ALL".to_string()
            }
        }
        _ => "REPEAT_OFF".to_string(),
    }
}

// =============================================================================
// Media Library Functions
// =============================================================================

/// List media files from server
#[no_mangle]
pub extern "C" fn fling_list_media() -> *mut FFIMediaList {
    let files: Vec<MediaFile> = with_combo_sync(|combo| combo.media_files()).unwrap_or_default();

    let ffi_files: Vec<FFIMediaFile> = files.iter().map(FFIMediaFile::from_media_file).collect();

    let count = ffi_files.len();
    let items = if count > 0 {
        let boxed = ffi_files.into_boxed_slice();
        Box::into_raw(boxed) as *mut FFIMediaFile
    } else {
        ptr::null_mut()
    };

    Box::into_raw(Box::new(FFIMediaList { items, count }))
}

/// Free media list
#[no_mangle]
pub extern "C" fn fling_free_media(list: *mut FFIMediaList) {
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

// =============================================================================
// Media Playback Functions
// =============================================================================

/// Play a media file by ID on a device
#[no_mangle]
pub extern "C" fn fling_play_media(device: *const c_char, media_id: *const c_char) -> FFIResult {
    let device_name = match unsafe { c_str_to_string(device) } {
        Some(d) => d,
        None => {
            set_error("Invalid device name");
            return FFI_ERROR;
        }
    };

    let media_id = match unsafe { c_str_to_string(media_id) } {
        Some(id) => id,
        None => {
            set_error("Invalid media ID");
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

    // Find media file
    let files = with_combo_sync(|combo| combo.media_files()).unwrap_or_default();
    let media = files.iter().find(|f| f.id == media_id);

    let path = match media {
        Some(f) => f.path.clone(),
        None => {
            set_error("Media file not found");
            return FFI_NOT_FOUND;
        }
    };

    let result = rt.block_on(async {
        let guard = arc.read().await;
        match guard.as_ref() {
            Some(combo) => Some(combo.push(&path, &renderer).await),
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

/// Play a URL on a device
#[no_mangle]
pub extern "C" fn fling_play_url(device: *const c_char, url: *const c_char) -> FFIResult {
    let device_name = match unsafe { c_str_to_string(device) } {
        Some(d) => d,
        None => {
            set_error("Invalid device name");
            return FFI_ERROR;
        }
    };

    let url = match unsafe { c_str_to_string(url) } {
        Some(u) => u,
        None => {
            set_error("Invalid URL");
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
            Some(combo) => Some(combo.play_url(&url, &renderer).await),
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

/// Play a URL with optional content type and subtitle URL (synchronous)
#[no_mangle]
pub extern "C" fn fling_play_request(
    device: *const c_char,
    url: *const c_char,
    content_type: *const c_char,
    subtitle_url: *const c_char,
) -> FFIResult {
    let device_name = match unsafe { c_str_to_string(device) } {
        Some(d) => d,
        None => {
            set_error("Invalid device name");
            return FFI_ERROR;
        }
    };

    let url = match unsafe { c_str_to_string(url) } {
        Some(u) => u,
        None => {
            set_error("Invalid URL");
            return FFI_ERROR;
        }
    };

    let content_type = unsafe { c_str_to_string(content_type) };
    let subtitle_url = unsafe { c_str_to_string(subtitle_url) };

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
            Some(combo) => {
                let request = dlna_combo::PlayRequest::new(&url)
                    .with_content_type(content_type.as_deref())
                    .with_subtitle_url(subtitle_url.as_deref());
                Some(combo.play_request(&request, &renderer).await)
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

/// Play media by ID (async, non-blocking)
#[no_mangle]
pub extern "C" fn fling_play_media_async(
    device: *const c_char,
    media_id: *const c_char,
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

    let media_id_str = match unsafe { c_str_to_string(media_id) } {
        Some(id) => id,
        None => {
            set_error("Invalid media ID");
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

        // Find media file
        let files = combo.media_files();
        let media = files.iter().find(|f| f.id == media_id_str);

        let path = match media {
            Some(f) => f.path.clone(),
            None => {
                set_error("Media file not found");
                callback(FFI_NOT_FOUND, ctx_ptr as *mut c_void);
                return;
            }
        };

        match combo.push(&path, &renderer).await {
            Ok(()) => callback(FFI_OK, ctx_ptr as *mut c_void),
            Err(e) => {
                set_error(e.to_string());
                callback(FFI_ERROR, ctx_ptr as *mut c_void);
            }
        }
    });
}

/// Play a media file by ID on a device at a specific position
///
/// For Chromecast: uses native currentTime (instant start at position)
/// For DLNA: does play + wait + seek (handles 701 errors internally)
#[no_mangle]
pub extern "C" fn fling_play_media_at_position(
    device: *const c_char,
    media_id: *const c_char,
    position_secs: f64,
) -> FFIResult {
    let device_name = match unsafe { c_str_to_string(device) } {
        Some(d) => d,
        None => {
            set_error("Invalid device name");
            return FFI_ERROR;
        }
    };

    let media_id = match unsafe { c_str_to_string(media_id) } {
        Some(id) => id,
        None => {
            set_error("Invalid media ID");
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

    // Find media file
    let files = with_combo_sync(|combo| combo.media_files()).unwrap_or_default();
    let media = files.iter().find(|f| f.id == media_id);

    let path = match media {
        Some(f) => f.path.clone(),
        None => {
            set_error("Media file not found");
            return FFI_NOT_FOUND;
        }
    };

    let result = rt.block_on(async {
        let guard = arc.read().await;
        match guard.as_ref() {
            Some(combo) => Some(
                combo
                    .push_at_position(&path, &renderer, position_secs)
                    .await,
            ),
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

/// Play a URL on a device at a specific position
///
/// For Chromecast: uses native currentTime (instant start at position)
/// For DLNA: does play + wait + seek (handles 701 errors internally)
#[no_mangle]
pub extern "C" fn fling_play_url_at_position(
    device: *const c_char,
    url: *const c_char,
    position_secs: f64,
) -> FFIResult {
    let device_name = match unsafe { c_str_to_string(device) } {
        Some(d) => d,
        None => {
            set_error("Invalid device name");
            return FFI_ERROR;
        }
    };

    let url = match unsafe { c_str_to_string(url) } {
        Some(u) => u,
        None => {
            set_error("Invalid URL");
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
            Some(combo) => Some(
                combo
                    .play_url_at_position(&url, &renderer, position_secs)
                    .await,
            ),
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

/// Play URL (async, non-blocking)
#[no_mangle]
pub extern "C" fn fling_play_url_async(
    device: *const c_char,
    url: *const c_char,
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

    let url_str = match unsafe { c_str_to_string(url) } {
        Some(u) => u,
        None => {
            set_error("Invalid URL");
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

        match combo.play_url(&url_str, &renderer).await {
            Ok(()) => callback(FFI_OK, ctx_ptr as *mut c_void),
            Err(e) => {
                set_error(e.to_string());
                callback(FFI_ERROR, ctx_ptr as *mut c_void);
            }
        }
    });
}

/// Play a URL with optional content type and subtitle URL (async, non-blocking)
#[no_mangle]
pub extern "C" fn fling_play_request_async(
    device: *const c_char,
    url: *const c_char,
    content_type: *const c_char,
    subtitle_url: *const c_char,
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

    let url_str = match unsafe { c_str_to_string(url) } {
        Some(u) => u,
        None => {
            set_error("Invalid URL");
            callback(FFI_ERROR, context);
            return;
        }
    };

    let content_type_str = unsafe { c_str_to_string(content_type) };
    let subtitle_url_str = unsafe { c_str_to_string(subtitle_url) };

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

        let request = dlna_combo::PlayRequest::new(&url_str)
            .with_content_type(content_type_str.as_deref())
            .with_subtitle_url(subtitle_url_str.as_deref());

        match combo.play_request(&request, &renderer).await {
            Ok(()) => callback(FFI_OK, ctx_ptr as *mut c_void),
            Err(e) => {
                set_error(e.to_string());
                callback(FFI_ERROR, ctx_ptr as *mut c_void);
            }
        }
    });
}

/// Sync a device-side queue using JSON payload (synchronous)
#[no_mangle]
pub extern "C" fn fling_sync_queue(
    device: *const c_char,
    queue_json: *const c_char,
    start_index: u32,
) -> FFIResult {
    let device_name = match unsafe { c_str_to_string(device) } {
        Some(d) => d,
        None => {
            set_error("Invalid device name");
            return FFI_ERROR;
        }
    };

    let queue_str = match unsafe { c_str_to_string(queue_json) } {
        Some(q) => q,
        None => {
            set_error("Invalid queue JSON");
            return FFI_ERROR;
        }
    };

    let payload = match parse_queue_payload(&queue_str) {
        Ok(payload) => payload,
        Err(e) => {
            set_error(e);
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
            Some(combo) => {
                let requests =
                    build_queue_requests(combo, &payload.items).map_err(|e| e.to_string())?;
                let repeat_mode = map_repeat_mode(
                    payload.repeat_mode.as_deref(),
                    payload.shuffle.unwrap_or(false),
                );
                let options = dlna_combo::QueueOptions::new(Some(repeat_mode));
                combo
                    .sync_queue(&renderer, &requests, start_index as usize, &options)
                    .await
                    .map_err(|e| e.to_string())
            }
            None => Err("Not initialized".to_string()),
        }
    });

    match result {
        Ok(()) => FFI_OK,
        Err(e) => {
            set_error(e);
            FFI_ERROR
        }
    }
}

/// Sync a device-side queue using JSON payload (async, non-blocking)
#[no_mangle]
pub extern "C" fn fling_sync_queue_async(
    device: *const c_char,
    queue_json: *const c_char,
    start_index: u32,
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

    let queue_str = match unsafe { c_str_to_string(queue_json) } {
        Some(q) => q,
        None => {
            set_error("Invalid queue JSON");
            callback(FFI_ERROR, context);
            return;
        }
    };

    let payload = match parse_queue_payload(&queue_str) {
        Ok(payload) => payload,
        Err(e) => {
            set_error(e);
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

        let requests = match build_queue_requests(combo, &payload.items) {
            Ok(r) => r,
            Err(e) => {
                set_error(e);
                callback(FFI_ERROR, ctx_ptr as *mut c_void);
                return;
            }
        };

        let repeat_mode = map_repeat_mode(
            payload.repeat_mode.as_deref(),
            payload.shuffle.unwrap_or(false),
        );
        let options = dlna_combo::QueueOptions::new(Some(repeat_mode));

        match combo
            .sync_queue(&renderer, &requests, start_index as usize, &options)
            .await
        {
            Ok(()) => callback(FFI_OK, ctx_ptr as *mut c_void),
            Err(e) => {
                set_error(e.to_string());
                callback(FFI_ERROR, ctx_ptr as *mut c_void);
            }
        }
    });
}

// =============================================================================
// Media Directory Management
// =============================================================================

/// Add a single media file by path (ad-hoc rescan)
#[no_mangle]
pub extern "C" fn fling_add_media_file(path: *const c_char) -> *mut FFIMediaFile {
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

    let path_buf = PathBuf::from(&path_str);

    let result = rt.block_on(async {
        let guard = arc.read().await;
        match guard.as_ref() {
            Some(combo) => combo.add_file(path_buf.clone()).map_err(|e| e.to_string()),
            None => Err("Not initialized".to_string()),
        }
    });

    match result {
        Ok(Some(file)) => Box::into_raw(Box::new(FFIMediaFile::from_media_file(&file))),
        Ok(None) => {
            set_error("Unsupported media file");
            ptr::null_mut()
        }
        Err(e) => {
            set_error(e);
            ptr::null_mut()
        }
    }
}

/// Free a single FFIMediaFile allocated by fling_add_media_file
#[no_mangle]
pub extern "C" fn fling_free_media_file(file: *mut FFIMediaFile) {
    if file.is_null() {
        return;
    }
    unsafe {
        let mut file = Box::from_raw(file);
        file.free();
    }
}

/// Set or change the media directory without restarting
///
/// # Arguments
/// * `path` - Path to new media directory (can be NULL to clear files)
///
/// # Returns
/// Number of files found (or FFI_ERROR on failure)
#[no_mangle]
pub extern "C" fn fling_set_media_dir(path: *const c_char) -> i32 {
    let path_str = unsafe { c_str_to_string(path) };

    tracing::info!("FFI: Setting media directory to: {:?}", path_str);

    let rt = match get_runtime() {
        Some(rt) => rt,
        None => {
            set_error("Runtime not initialized");
            return FFI_NOT_INITIALIZED;
        }
    };

    let arc = match combo_ref() {
        Some(a) => a,
        None => {
            set_error("Not initialized");
            return FFI_NOT_INITIALIZED;
        }
    };

    rt.block_on(async {
        let mut guard = arc.write().await;
        let combo = match guard.as_mut() {
            Some(c) => c,
            None => {
                set_error("Not initialized");
                return FFI_NOT_INITIALIZED;
            }
        };

        match path_str {
            Some(p) => {
                let path = PathBuf::from(p);
                match combo.set_media_directory(path) {
                    Ok(count) => {
                        tracing::info!("FFI: Scanned {} files", count);
                        count as i32
                    }
                    Err(e) => {
                        set_error(format!("Failed to set media directory: {e}"));
                        FFI_ERROR
                    }
                }
            }
            None => {
                // Clear media files if path is NULL
                combo.clear_media_files();
                tracing::info!("FFI: Cleared media files");
                0
            }
        }
    })
}

/// Add a media directory without clearing existing files
///
/// # Arguments
/// * `path` - Path to media directory (required)
///
/// # Returns
/// Number of files found (or FFI_ERROR on failure)
#[no_mangle]
pub extern "C" fn fling_add_media_dir(path: *const c_char) -> i32 {
    let path_str = unsafe { c_str_to_string(path) };

    let rt = match get_runtime() {
        Some(rt) => rt,
        None => {
            set_error("Runtime not initialized");
            return FFI_NOT_INITIALIZED;
        }
    };

    let arc = match combo_ref() {
        Some(a) => a,
        None => {
            set_error("Not initialized");
            return FFI_NOT_INITIALIZED;
        }
    };

    rt.block_on(async {
        let mut guard = arc.write().await;
        let combo = match guard.as_mut() {
            Some(c) => c,
            None => {
                set_error("Not initialized");
                return FFI_NOT_INITIALIZED;
            }
        };

        let path = match path_str {
            Some(p) => PathBuf::from(p),
            None => {
                set_error("Path is required");
                return FFI_ERROR;
            }
        };

        match combo.add_media_directory(path) {
            Ok(count) => count as i32,
            Err(e) => {
                set_error(format!("Failed to add media directory: {e}"));
                FFI_ERROR
            }
        }
    })
}

/// Remove a media directory
///
/// # Arguments
/// * `path` - Path to media directory (required)
#[no_mangle]
pub extern "C" fn fling_remove_media_dir(path: *const c_char) -> FFIResult {
    let path_str = unsafe { c_str_to_string(path) };

    let rt = match get_runtime() {
        Some(rt) => rt,
        None => {
            set_error("Runtime not initialized");
            return FFI_NOT_INITIALIZED;
        }
    };

    let arc = match combo_ref() {
        Some(a) => a,
        None => {
            set_error("Not initialized");
            return FFI_NOT_INITIALIZED;
        }
    };

    rt.block_on(async {
        let mut guard = arc.write().await;
        let combo = match guard.as_mut() {
            Some(c) => c,
            None => {
                set_error("Not initialized");
                return FFI_NOT_INITIALIZED;
            }
        };

        let path = match path_str {
            Some(p) => PathBuf::from(p),
            None => {
                set_error("Path is required");
                return FFI_ERROR;
            }
        };

        match combo.remove_media_directory(path) {
            Ok(()) => FFI_OK,
            Err(e) => {
                set_error(format!("Failed to remove media directory: {e}"));
                FFI_ERROR
            }
        }
    })
}

/// Clear all media directories and files
#[no_mangle]
pub extern "C" fn fling_clear_media_dirs() -> FFIResult {
    let rt = match get_runtime() {
        Some(rt) => rt,
        None => {
            set_error("Runtime not initialized");
            return FFI_NOT_INITIALIZED;
        }
    };

    let arc = match combo_ref() {
        Some(a) => a,
        None => {
            set_error("Not initialized");
            return FFI_NOT_INITIALIZED;
        }
    };

    rt.block_on(async {
        let mut guard = arc.write().await;
        let combo = match guard.as_mut() {
            Some(c) => c,
            None => {
                set_error("Not initialized");
                return FFI_NOT_INITIALIZED;
            }
        };

        match combo.clear_media_directories() {
            Ok(()) => FFI_OK,
            Err(e) => {
                set_error(format!("Failed to clear media directories: {e}"));
                FFI_ERROR
            }
        }
    })
}

/// Clear all media files
#[no_mangle]
pub extern "C" fn fling_clear_media() {
    if let (Some(arc), Some(rt)) = (combo_ref(), get_runtime()) {
        rt.block_on(async {
            let mut guard = arc.write().await;
            if let Some(combo) = guard.as_mut() {
                combo.clear_media_files();
                tracing::info!("FFI: Cleared media files");
            }
        });
    }
}

/// Set media directory (async, non-blocking)
#[no_mangle]
pub extern "C" fn fling_set_media_dir_async(
    path: *const c_char,
    callback: FFIIntCallback,
    context: *mut c_void,
) {
    let path_str = unsafe { c_str_to_string(path) };
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
        let mut guard = combo_arc.write().await;
        let combo = match guard.as_mut() {
            Some(c) => c,
            None => {
                set_error("Not initialized");
                callback(FFI_NOT_INITIALIZED, ctx_ptr as *mut c_void);
                return;
            }
        };

        let result = match path_str {
            Some(p) => {
                let path = PathBuf::from(p);
                match combo.set_media_directory(path) {
                    Ok(count) => count as i32,
                    Err(e) => {
                        set_error(format!("Failed to set media directory: {e}"));
                        FFI_ERROR
                    }
                }
            }
            None => {
                combo.clear_media_files();
                0
            }
        };

        callback(result, ctx_ptr as *mut c_void);
    });
}

/// Add media directory (async, non-blocking)
#[no_mangle]
pub extern "C" fn fling_add_media_dir_async(
    path: *const c_char,
    callback: FFIIntCallback,
    context: *mut c_void,
) {
    let path_str = unsafe { c_str_to_string(path) };
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
        let mut guard = combo_arc.write().await;
        let combo = match guard.as_mut() {
            Some(c) => c,
            None => {
                set_error("Not initialized");
                callback(FFI_NOT_INITIALIZED, ctx_ptr as *mut c_void);
                return;
            }
        };

        let path = match path_str {
            Some(p) => PathBuf::from(p),
            None => {
                set_error("Path is required");
                callback(FFI_ERROR, ctx_ptr as *mut c_void);
                return;
            }
        };

        let result = match combo.add_media_directory(path) {
            Ok(count) => count as i32,
            Err(e) => {
                set_error(format!("Failed to add media directory: {e}"));
                FFI_ERROR
            }
        };

        callback(result, ctx_ptr as *mut c_void);
    });
}

/// Remove media directory (async, non-blocking)
#[no_mangle]
pub extern "C" fn fling_remove_media_dir_async(
    path: *const c_char,
    callback: FFIResultCallback,
    context: *mut c_void,
) {
    let path_str = unsafe { c_str_to_string(path) };
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
        let mut guard = combo_arc.write().await;
        let combo = match guard.as_mut() {
            Some(c) => c,
            None => {
                set_error("Not initialized");
                callback(FFI_NOT_INITIALIZED, ctx_ptr as *mut c_void);
                return;
            }
        };

        let path = match path_str {
            Some(p) => PathBuf::from(p),
            None => {
                set_error("Path is required");
                callback(FFI_ERROR, ctx_ptr as *mut c_void);
                return;
            }
        };

        let result = match combo.remove_media_directory(path) {
            Ok(()) => FFI_OK,
            Err(e) => {
                set_error(format!("Failed to remove media directory: {e}"));
                FFI_ERROR
            }
        };

        callback(result, ctx_ptr as *mut c_void);
    });
}

/// Clear media directories (async, non-blocking)
#[no_mangle]
pub extern "C" fn fling_clear_media_dirs_async(callback: FFIResultCallback, context: *mut c_void) {
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
        let mut guard = combo_arc.write().await;
        let combo = match guard.as_mut() {
            Some(c) => c,
            None => {
                set_error("Not initialized");
                callback(FFI_NOT_INITIALIZED, ctx_ptr as *mut c_void);
                return;
            }
        };

        let result = match combo.clear_media_directories() {
            Ok(()) => FFI_OK,
            Err(e) => {
                set_error(format!("Failed to clear media directories: {e}"));
                FFI_ERROR
            }
        };

        callback(result, ctx_ptr as *mut c_void);
    });
}

// =============================================================================
// Folder Visibility Functions
// =============================================================================

/// Set public folders (visible to all devices browsing the server)
///
/// # Arguments
/// * `folders_json` - JSON array of folder paths, e.g. ["/path/to/public1", "/path/to/public2"]
#[no_mangle]
pub extern "C" fn fling_set_public_folders(folders_json: *const c_char) -> FFIResult {
    let json_str = match unsafe { c_str_to_string(folders_json) } {
        Some(s) => s,
        None => {
            set_error("Invalid JSON");
            return FFI_ERROR;
        }
    };

    let folders: Vec<String> = match serde_json::from_str(&json_str) {
        Ok(f) => f,
        Err(e) => {
            set_error(format!("Failed to parse folders JSON: {e}"));
            return FFI_ERROR;
        }
    };

    let paths: Vec<PathBuf> = folders.into_iter().map(PathBuf::from).collect();

    let rt = match get_runtime() {
        Some(rt) => rt,
        None => {
            set_error("Runtime not initialized");
            return FFI_NOT_INITIALIZED;
        }
    };

    let arc = match combo_ref() {
        Some(a) => a,
        None => {
            set_error("Not initialized");
            return FFI_NOT_INITIALIZED;
        }
    };

    rt.block_on(async {
        let guard = arc.read().await;
        let combo = match guard.as_ref() {
            Some(c) => c,
            None => {
                set_error("Not initialized");
                return FFI_NOT_INITIALIZED;
            }
        };

        match combo.set_public_folders(paths) {
            Ok(()) => FFI_OK,
            Err(e) => {
                set_error(format!("Failed to set public folders: {e}"));
                FFI_ERROR
            }
        }
    })
}

/// Set folders for a specific device (by IP address)
///
/// # Arguments
/// * `device_ip` - Device IP address (e.g. "192.168.1.100")
/// * `folders_json` - JSON array of folder paths, e.g. ["/path/to/folder1", "/path/to/folder2"]
#[no_mangle]
pub extern "C" fn fling_set_device_folders(
    device_ip: *const c_char,
    folders_json: *const c_char,
) -> FFIResult {
    let ip_str = match unsafe { c_str_to_string(device_ip) } {
        Some(s) => s,
        None => {
            set_error("Invalid device IP");
            return FFI_ERROR;
        }
    };

    let json_str = match unsafe { c_str_to_string(folders_json) } {
        Some(s) => s,
        None => {
            set_error("Invalid JSON");
            return FFI_ERROR;
        }
    };

    let folders: Vec<String> = match serde_json::from_str(&json_str) {
        Ok(f) => f,
        Err(e) => {
            set_error(format!("Failed to parse folders JSON: {e}"));
            return FFI_ERROR;
        }
    };

    let paths: Vec<PathBuf> = folders.into_iter().map(PathBuf::from).collect();

    let rt = match get_runtime() {
        Some(rt) => rt,
        None => {
            set_error("Runtime not initialized");
            return FFI_NOT_INITIALIZED;
        }
    };

    let arc = match combo_ref() {
        Some(a) => a,
        None => {
            set_error("Not initialized");
            return FFI_NOT_INITIALIZED;
        }
    };

    rt.block_on(async {
        let guard = arc.read().await;
        let combo = match guard.as_ref() {
            Some(c) => c,
            None => {
                set_error("Not initialized");
                return FFI_NOT_INITIALIZED;
            }
        };

        match combo.set_device_folders(&ip_str, paths) {
            Ok(()) => FFI_OK,
            Err(e) => {
                set_error(format!("Failed to set device folders: {e}"));
                FFI_ERROR
            }
        }
    })
}

/// Clear all device folder assignments
#[no_mangle]
pub extern "C" fn fling_clear_device_folders() -> FFIResult {
    let rt = match get_runtime() {
        Some(rt) => rt,
        None => {
            set_error("Runtime not initialized");
            return FFI_NOT_INITIALIZED;
        }
    };

    let arc = match combo_ref() {
        Some(a) => a,
        None => {
            set_error("Not initialized");
            return FFI_NOT_INITIALIZED;
        }
    };

    rt.block_on(async {
        let guard = arc.read().await;
        let combo = match guard.as_ref() {
            Some(c) => c,
            None => {
                set_error("Not initialized");
                return FFI_NOT_INITIALIZED;
            }
        };

        match combo.clear_device_folders() {
            Ok(()) => FFI_OK,
            Err(e) => {
                set_error(format!("Failed to clear device folders: {e}"));
                FFI_ERROR
            }
        }
    })
}

// =============================================================================
// Progress-aware Media Directory Scanning
// =============================================================================

/// Add media directory with progress callback (async, non-blocking)
///
/// Progress callback is called periodically during scanning.
/// Completion callback is called when scanning is done with file count.
#[no_mangle]
pub extern "C" fn fling_add_media_dir_with_progress(
    path: *const c_char,
    progress_cb: FFIScanProgressCallback,
    completion_cb: FFIIntCallback,
    context: *mut c_void,
) {
    use std::sync::{Arc, Mutex};

    let path_str = match unsafe { c_str_to_string(path) } {
        Some(p) => p,
        None => {
            set_error("Invalid path");
            completion_cb(FFI_ERROR, context);
            return;
        }
    };

    let rt = match get_runtime() {
        Some(rt) => rt,
        None => {
            set_error("Not initialized");
            completion_cb(FFI_NOT_INITIALIZED, context);
            return;
        }
    };

    let combo_arc = match get_combo_arc() {
        Some(arc) => arc,
        None => {
            set_error("Not initialized");
            completion_cb(FFI_NOT_INITIALIZED, context);
            return;
        }
    };

    let path_buf = PathBuf::from(&path_str);

    // Convert pointers to usize for thread safety
    let ctx_usize = context as usize;
    let progress_cb_usize = progress_cb as usize;
    let completion_cb_usize = completion_cb as usize;

    // Spawn on a background thread to avoid blocking main thread
    std::thread::spawn(move || {
        // Mutex to serialize callback invocations (rayon calls from multiple threads)
        let callback_mutex = Arc::new(Mutex::new(()));
        let callback_mutex_clone = callback_mutex.clone();

        // Create progress callback that forwards to FFI
        let ffi_progress = move |progress: &dlna_combo::ScanProgress| {
            let _lock = callback_mutex_clone.lock().ok();

            let ctx = ctx_usize as *mut c_void;
            let cb: FFIScanProgressCallback = unsafe { std::mem::transmute(progress_cb_usize) };

            let ffi_progress = FFIScanProgress {
                files_scanned: progress.files_scanned as i32,
                files_total: progress.files_total as i32,
                current_path: ptr::null(),
            };
            cb(&ffi_progress, ctx);
        };

        // Step 1: Scan files WITHOUT holding combo lock (allows discovery to proceed)
        let scan_result = dlna_combo::scan_directory_with_progress(&path_buf, ffi_progress);

        // Step 2: Briefly acquire lock to add scanned files to server
        let result = rt.block_on(async {
            let files = match scan_result {
                Ok(files) => files,
                Err(e) => return Err(e.to_string()),
            };

            let file_count = files.len();

            let mut guard = combo_arc.write().await;
            let combo = match guard.as_mut() {
                Some(c) => c,
                None => {
                    return Err("Not initialized".to_string());
                }
            };

            // Add directory to config and add files (quick operation)
            if let Some(server) = combo.server_mut() {
                server.add_scanned_files(path_buf, files);
            }

            Ok(file_count)
        });

        let ctx = ctx_usize as *mut c_void;
        let completion_cb: FFIIntCallback = unsafe { std::mem::transmute(completion_cb_usize) };

        match result {
            Ok(count) => {
                completion_cb(count as i32, ctx);
            }
            Err(e) => {
                set_error(format!("Failed to add media directory: {e}"));
                completion_cb(FFI_ERROR, ctx);
            }
        }
    });
}

/// Count media files in a directory (quick pass, no metadata extraction)
#[no_mangle]
pub extern "C" fn fling_count_media_files(path: *const c_char) -> i32 {
    let path_str = match unsafe { c_str_to_string(path) } {
        Some(p) => p,
        None => {
            set_error("Invalid path");
            return FFI_ERROR;
        }
    };

    let path_buf = PathBuf::from(&path_str);
    match dlna_combo::count_media_files(&path_buf) {
        Ok(count) => count as i32,
        Err(e) => {
            set_error(format!("Failed to count files: {e}"));
            FFI_ERROR
        }
    }
}

/// Add media directory with progress callback and global offset (for cumulative progress)
///
/// offset - number of files already processed in previous directories
/// total - total files across all directories
#[no_mangle]
pub extern "C" fn fling_add_media_dir_with_cumulative_progress(
    path: *const c_char,
    offset: i32,
    total: i32,
    progress_cb: FFIScanProgressCallback,
    completion_cb: FFIIntCallback,
    context: *mut c_void,
) {
    use std::sync::{Arc, Mutex};

    let path_str = match unsafe { c_str_to_string(path) } {
        Some(p) => p,
        None => {
            set_error("Invalid path");
            completion_cb(FFI_ERROR, context);
            return;
        }
    };

    let rt = match get_runtime() {
        Some(rt) => rt,
        None => {
            set_error("Not initialized");
            completion_cb(FFI_NOT_INITIALIZED, context);
            return;
        }
    };

    let combo_arc = match get_combo_arc() {
        Some(arc) => arc,
        None => {
            set_error("Not initialized");
            completion_cb(FFI_NOT_INITIALIZED, context);
            return;
        }
    };

    let path_buf = PathBuf::from(&path_str);
    let global_total = total;

    let ctx_usize = context as usize;
    let progress_cb_usize = progress_cb as usize;
    let completion_cb_usize = completion_cb as usize;

    std::thread::spawn(move || {
        let callback_mutex = Arc::new(Mutex::new(()));
        let callback_mutex_clone = callback_mutex.clone();

        // Progress callback with global offset
        let ffi_progress = move |progress: &dlna_combo::ScanProgress| {
            let _lock = callback_mutex_clone.lock().ok();

            let ctx = ctx_usize as *mut c_void;
            let cb: FFIScanProgressCallback = unsafe { std::mem::transmute(progress_cb_usize) };

            let ffi_progress = FFIScanProgress {
                files_scanned: offset + progress.files_scanned as i32,
                files_total: global_total,
                current_path: ptr::null(),
            };
            cb(&ffi_progress, ctx);
        };

        let scan_result = dlna_combo::scan_directory_with_progress(&path_buf, ffi_progress);

        let result = rt.block_on(async {
            let files = match scan_result {
                Ok(files) => files,
                Err(e) => return Err(e.to_string()),
            };

            let file_count = files.len();

            let mut guard = combo_arc.write().await;
            let combo = match guard.as_mut() {
                Some(c) => c,
                None => {
                    return Err("Not initialized".to_string());
                }
            };

            if let Some(server) = combo.server_mut() {
                server.add_scanned_files(path_buf, files);
            }

            Ok(file_count)
        });

        let ctx = ctx_usize as *mut c_void;
        let completion_cb: FFIIntCallback = unsafe { std::mem::transmute(completion_cb_usize) };

        match result {
            Ok(count) => {
                completion_cb(count as i32, ctx);
            }
            Err(e) => {
                set_error(format!("Failed to add media directory: {e}"));
                completion_cb(FFI_ERROR, ctx);
            }
        }
    });
}
