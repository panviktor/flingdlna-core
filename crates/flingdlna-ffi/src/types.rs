use std::ffi::{c_char, c_void};

use dlna_combo::{MediaFile, Renderer};
use dlna_core::{DeviceCapabilities, MediaInfo};

use crate::helpers::{free_c_string, option_string_to_c, string_to_c};

// =============================================================================
// Callback Types for Async API
// =============================================================================

/// Callback for operations that return just a result code
pub type FFIResultCallback = extern "C" fn(result: FFIResult, context: *mut c_void);

/// Callback for operations that return renderer list
pub type FFIRendererListCallback =
    extern "C" fn(result: *mut FFIRendererList, context: *mut c_void);

/// Callback for operations that return media list
pub type FFIMediaListCallback = extern "C" fn(result: *mut FFIMediaList, context: *mut c_void);

/// Callback for operations that return playback status
pub type FFIStatusCallback = extern "C" fn(result: *mut FFIPlaybackStatus, context: *mut c_void);

/// Callback for operations that return volume info
pub type FFIVolumeCallback = extern "C" fn(result: *mut FFIVolumeInfo, context: *mut c_void);

/// Callback for operations that return i32 (like file count)
pub type FFIIntCallback = extern "C" fn(result: i32, context: *mut c_void);

// =============================================================================
// Scan Progress Types
// =============================================================================

/// Progress information during media scanning
#[repr(C)]
pub struct FFIScanProgress {
    /// Number of files scanned so far
    pub files_scanned: i32,
    /// Total number of files to scan
    pub files_total: i32,
    /// Current file path being scanned (may be null)
    pub current_path: *const c_char,
}

/// Callback for scan progress updates
/// Called periodically during scanning with progress info
pub type FFIScanProgressCallback =
    extern "C" fn(progress: *const FFIScanProgress, context: *mut c_void);

/// Callback for session events (protocol-agnostic: DLNA + Chromecast)
/// Called when any renderer session emits an event
pub type FFISessionEventCallback =
    extern "C" fn(event: *const FFISessionEvent, context: *mut c_void);

/// Callback for progressive renderer discovery
/// Called once for each discovered renderer, then with NULL to signal completion
pub type FFIRendererCallback = extern "C" fn(renderer: *mut FFIRenderer, context: *mut c_void);

// =============================================================================
// FFI Result Type
// =============================================================================

pub type FFIResult = i32;

pub const FFI_OK: FFIResult = 0;
pub const FFI_ERROR: FFIResult = -1;
pub const FFI_NOT_FOUND: FFIResult = -2;
pub const FFI_TIMEOUT: FFIResult = -3;
pub const FFI_NOT_INITIALIZED: FFIResult = -4;

// =============================================================================
// Playability Types
// =============================================================================

pub const FFI_PLAYABLE: i32 = 0;
pub const FFI_MAYBE: i32 = 1;
pub const FFI_UNSUPPORTED: i32 = 2;

/// Playability information for a media request
#[repr(C)]
pub struct FFIPlayability {
    pub status: i32,
    pub reason: *mut c_char,
}

impl FFIPlayability {
    pub(crate) fn from_report(report: &dlna_controller::PlayabilityReport) -> Self {
        let status = match report.playability {
            dlna_controller::Playability::Playable => FFI_PLAYABLE,
            dlna_controller::Playability::Maybe => FFI_MAYBE,
            dlna_controller::Playability::Unsupported => FFI_UNSUPPORTED,
        };

        Self {
            status,
            reason: option_string_to_c(report.reason.as_deref()),
        }
    }

    pub(crate) unsafe fn free(&mut self) {
        free_c_string(self.reason);
    }
}

// =============================================================================
// FFI Types
// =============================================================================

/// Device type enum (C-compatible)
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FFIDeviceType {
    Dlna = 0,
    Chromecast = 1,
}

/// Device icon information
#[repr(C)]
pub struct FFIDeviceIcon {
    pub mime_type: *mut c_char,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub url: *mut c_char,
}

/// Renderer device information
#[repr(C)]
pub struct FFIRenderer {
    pub name: *mut c_char,
    pub location: *mut c_char,
    pub device_type: FFIDeviceType,
    pub upnp_device_type: *mut c_char, // UPnP device type string (e.g. "urn:schemas-upnp-org:device:MediaRenderer:1")
    pub manufacturer: *mut c_char,
    pub model: *mut c_char,
    pub mac_address: *mut c_char,
    pub icons: *mut FFIDeviceIcon,
    pub icons_count: usize,
    pub firmware_version: *mut c_char, // Firmware version (from Chromecast mDNS 've' field or DLNA device description)
    pub capabilities: *mut c_char,     // Capabilities string (from Chromecast mDNS 'ca' field)
}

impl FFIRenderer {
    pub(crate) fn from_renderer(r: &Renderer) -> Self {
        // Determine device type based on location URL scheme
        let device_type = if r.location.scheme() == "cast" {
            FFIDeviceType::Chromecast
        } else {
            FFIDeviceType::Dlna
        };

        // UPnP device type string for DLNA devices (e.g. "urn:schemas-upnp-org:device:MediaRenderer:1")
        // For Chromecast, we don't have a UPnP device type
        let upnp_device_type = if device_type == FFIDeviceType::Dlna {
            // Try to get from model_name or just use generic MediaRenderer
            option_string_to_c(Some("urn:schemas-upnp-org:device:MediaRenderer:1"))
        } else {
            std::ptr::null_mut()
        };

        // Convert icons to FFI format
        let icons_count = r.icons.len();
        let icons = if icons_count > 0 {
            let ffi_icons: Vec<FFIDeviceIcon> = r
                .icons
                .iter()
                .map(|icon| FFIDeviceIcon {
                    mime_type: string_to_c(&icon.mime_type),
                    width: icon.width,
                    height: icon.height,
                    depth: icon.depth,
                    url: string_to_c(icon.url.as_str()),
                })
                .collect();

            Box::into_raw(ffi_icons.into_boxed_slice()) as *mut FFIDeviceIcon
        } else {
            std::ptr::null_mut()
        };

        Self {
            name: string_to_c(&r.friendly_name),
            location: string_to_c(r.location.as_str()),
            device_type,
            upnp_device_type,
            manufacturer: option_string_to_c(r.manufacturer.as_deref()),
            model: option_string_to_c(r.model_name.as_deref()),
            mac_address: option_string_to_c(r.mac_address.as_deref()),
            icons,
            icons_count,
            firmware_version: option_string_to_c(r.firmware_version.as_deref()),
            capabilities: option_string_to_c(r.capabilities.as_deref()),
        }
    }

    pub(crate) unsafe fn free(&mut self) {
        free_c_string(self.name);
        free_c_string(self.location);
        // device_type is an enum, not a pointer - no need to free
        free_c_string(self.upnp_device_type);
        free_c_string(self.manufacturer);
        free_c_string(self.model);
        free_c_string(self.mac_address);
        free_c_string(self.firmware_version);
        free_c_string(self.capabilities);

        // Free icons array
        if !self.icons.is_null() && self.icons_count > 0 {
            let icons_slice = std::slice::from_raw_parts_mut(self.icons, self.icons_count);
            for icon in icons_slice {
                free_c_string(icon.mime_type);
                free_c_string(icon.url);
            }
            drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                self.icons,
                self.icons_count,
            )));
        }
    }
}

/// List of renderers
#[repr(C)]
pub struct FFIRendererList {
    pub items: *mut FFIRenderer,
    pub count: usize,
}

/// Playback status
#[repr(C)]
pub struct FFIPlaybackStatus {
    pub state: *mut c_char,
    pub position_secs: u64,
    pub duration_secs: u64,
    pub current_uri: *mut c_char,
}

/// Volume information
#[repr(C)]
pub struct FFIVolumeInfo {
    pub volume: u8,
    pub muted: bool,
}

/// Media file information
#[repr(C)]
pub struct FFIMediaFile {
    pub id: *mut c_char,
    pub filename: *mut c_char,
    pub path: *mut c_char,
    pub mime_type: *mut c_char,
    pub size: u64,
    pub duration_secs: u64,
    pub title: *mut c_char,
    pub artist: *mut c_char,
    // Video-specific metadata
    pub year: u32,                // 0 = unknown
    pub width: u32,               // 0 = unknown
    pub height: u32,              // 0 = unknown
    pub video_codec: *mut c_char, // null = unknown
    pub audio_codec: *mut c_char, // null = unknown
}

impl FFIMediaFile {
    pub(crate) fn from_media_file(f: &MediaFile) -> Self {
        let (width, height) = f.resolution.unwrap_or((0, 0));
        Self {
            id: string_to_c(&f.id),
            filename: string_to_c(&f.filename),
            path: string_to_c(&f.path.to_string_lossy()),
            mime_type: string_to_c(&f.mime_type),
            size: f.size,
            duration_secs: f.duration.map(|d| d.as_secs()).unwrap_or(0),
            title: option_string_to_c(f.title.as_deref()),
            artist: option_string_to_c(f.artist.as_deref()),
            year: f.year.unwrap_or(0),
            width,
            height,
            video_codec: option_string_to_c(f.video_codec.as_deref()),
            audio_codec: option_string_to_c(f.audio_codec.as_deref()),
        }
    }

    pub(crate) unsafe fn free(&mut self) {
        free_c_string(self.id);
        free_c_string(self.filename);
        free_c_string(self.path);
        free_c_string(self.mime_type);
        free_c_string(self.title);
        free_c_string(self.artist);
        free_c_string(self.video_codec);
        free_c_string(self.audio_codec);
    }
}

/// List of media files
#[repr(C)]
pub struct FFIMediaList {
    pub items: *mut FFIMediaFile,
    pub count: usize,
}

/// Media information from GetMediaInfo (UPnP AVTransport)
#[repr(C)]
pub struct FFIMediaInfo {
    pub nr_tracks: u32,
    pub media_duration_secs: u64, // 0 if unknown
    pub has_duration: bool,       // true if duration is valid
    pub current_uri: *mut c_char,
    pub play_medium: *mut c_char, // e.g., "NETWORK", "HDD", "CD-DA"
    pub next_uri: *mut c_char,
}

impl FFIMediaInfo {
    pub(crate) fn from_media_info(info: &MediaInfo) -> Self {
        Self {
            nr_tracks: info.nr_tracks,
            media_duration_secs: info
                .media_duration
                .as_ref()
                .map(|d| d.as_secs())
                .unwrap_or(0),
            has_duration: info.media_duration.is_some(),
            current_uri: option_string_to_c(info.current_uri.as_deref()),
            play_medium: option_string_to_c(info.play_medium.as_deref()),
            next_uri: option_string_to_c(info.next_uri.as_deref()),
        }
    }

    pub(crate) unsafe fn free(&mut self) {
        free_c_string(self.current_uri);
        free_c_string(self.play_medium);
        free_c_string(self.next_uri);
    }
}

/// Device capabilities from GetDeviceCapabilities (UPnP AVTransport)
#[repr(C)]
pub struct FFIDeviceCapabilities {
    pub play_media: *mut *mut c_char, // Array of strings (e.g., ["NETWORK", "HDD"])
    pub play_media_count: usize,
    pub rec_media: *mut *mut c_char, // Usually ["NOT_IMPLEMENTED"]
    pub rec_media_count: usize,
    pub rec_quality_modes: *mut *mut c_char, // Usually ["NOT_IMPLEMENTED"]
    pub rec_quality_modes_count: usize,
}

impl FFIDeviceCapabilities {
    pub(crate) fn from_device_capabilities(caps: &DeviceCapabilities) -> Self {
        // Convert Vec<String> to C array of c_char*
        let play_media_count = caps.play_media.len();
        let play_media = if play_media_count > 0 {
            let c_strings: Vec<*mut c_char> =
                caps.play_media.iter().map(|s| string_to_c(s)).collect();
            Box::into_raw(c_strings.into_boxed_slice()) as *mut *mut c_char
        } else {
            std::ptr::null_mut()
        };

        let rec_media_count = caps.rec_media.len();
        let rec_media = if rec_media_count > 0 {
            let c_strings: Vec<*mut c_char> =
                caps.rec_media.iter().map(|s| string_to_c(s)).collect();
            Box::into_raw(c_strings.into_boxed_slice()) as *mut *mut c_char
        } else {
            std::ptr::null_mut()
        };

        let rec_quality_modes_count = caps.rec_quality_modes.len();
        let rec_quality_modes = if rec_quality_modes_count > 0 {
            let c_strings: Vec<*mut c_char> = caps
                .rec_quality_modes
                .iter()
                .map(|s| string_to_c(s))
                .collect();
            Box::into_raw(c_strings.into_boxed_slice()) as *mut *mut c_char
        } else {
            std::ptr::null_mut()
        };

        Self {
            play_media,
            play_media_count,
            rec_media,
            rec_media_count,
            rec_quality_modes,
            rec_quality_modes_count,
        }
    }

    pub(crate) unsafe fn free(&mut self) {
        // Free play_media array
        if !self.play_media.is_null() && self.play_media_count > 0 {
            let slice = std::slice::from_raw_parts_mut(self.play_media, self.play_media_count);
            for ptr in slice {
                free_c_string(*ptr);
            }
            drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                self.play_media,
                self.play_media_count,
            )));
        }

        // Free rec_media array
        if !self.rec_media.is_null() && self.rec_media_count > 0 {
            let slice = std::slice::from_raw_parts_mut(self.rec_media, self.rec_media_count);
            for ptr in slice {
                free_c_string(*ptr);
            }
            drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                self.rec_media,
                self.rec_media_count,
            )));
        }

        // Free rec_quality_modes array
        if !self.rec_quality_modes.is_null() && self.rec_quality_modes_count > 0 {
            let slice = std::slice::from_raw_parts_mut(
                self.rec_quality_modes,
                self.rec_quality_modes_count,
            );
            for ptr in slice {
                free_c_string(*ptr);
            }
            drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                self.rec_quality_modes,
                self.rec_quality_modes_count,
            )));
        }
    }
}

/// Callback for operations that return media info
pub type FFIMediaInfoCallback = extern "C" fn(result: *mut FFIMediaInfo, context: *mut c_void);

/// Callback for operations that return device capabilities
pub type FFIDeviceCapabilitiesCallback =
    extern "C" fn(result: *mut FFIDeviceCapabilities, context: *mut c_void);

// =============================================================================
// Session Events (Protocol-Agnostic: DLNA + Chromecast)
// =============================================================================

/// Session event type (protocol-agnostic)
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FFISessionEventType {
    /// Volume changed (0-100)
    SessVolumeChanged = 0,
    /// Mute state changed
    SessMuteChanged = 1,
    /// Playback state changed (PLAYING, PAUSED, STOPPED, etc.)
    SessStateChanged = 2,
    /// Current URI changed
    SessUriChanged = 3,
    /// Playback position changed (in seconds)
    SessPositionChanged = 4,
    /// Media duration changed (in seconds)
    SessDurationChanged = 5,
    /// Play mode changed (NORMAL, SHUFFLE, REPEAT, etc.)
    SessPlayModeChanged = 6,
    /// Connection lost or session ended
    SessSessionLost = 7,
}

/// Session event data (protocol-agnostic: DLNA + Chromecast)
/// Contains event type and associated data
#[repr(C)]
pub struct FFISessionEvent {
    /// Type of event
    pub event_type: FFISessionEventType,
    /// Device ID (UDN for DLNA, ID for Chromecast)
    pub device_id: *mut c_char,

    // Event-specific data (check event_type to see which are valid)
    /// Volume value (0-100) - valid for VolumeChanged
    pub volume: u8,
    /// Mute state - valid for MuteChanged
    pub muted: bool,
    /// State string - valid for StateChanged
    pub state: *mut c_char,
    /// URI string - valid for UriChanged
    pub uri: *mut c_char,
    /// Position in seconds - valid for PositionChanged
    pub position_secs: u64,
    /// Duration in seconds - valid for DurationChanged
    pub duration_secs: u64,
    /// Play mode string - valid for PlayModeChanged
    pub mode: *mut c_char,
    /// Reason string - valid for SessionLost
    pub reason: *mut c_char,
}

impl FFISessionEvent {
    /// Convert from Rust SessionEvent to FFI-compatible format
    pub(crate) fn from_session_event(
        event: &dlna_controller::SessionEvent,
        device_id: &str,
    ) -> Self {
        use dlna_controller::SessionEvent;
        use std::ptr::null_mut;

        match event {
            SessionEvent::VolumeChanged { volume } => Self {
                event_type: FFISessionEventType::SessVolumeChanged,
                device_id: string_to_c(device_id),
                volume: *volume,
                muted: false,
                state: null_mut(),
                uri: null_mut(),
                position_secs: 0,
                duration_secs: 0,
                mode: null_mut(),
                reason: null_mut(),
            },
            SessionEvent::MuteChanged { muted } => Self {
                event_type: FFISessionEventType::SessMuteChanged,
                device_id: string_to_c(device_id),
                volume: 0,
                muted: *muted,
                state: null_mut(),
                uri: null_mut(),
                position_secs: 0,
                duration_secs: 0,
                mode: null_mut(),
                reason: null_mut(),
            },
            SessionEvent::StateChanged { state } => Self {
                event_type: FFISessionEventType::SessStateChanged,
                device_id: string_to_c(device_id),
                volume: 0,
                muted: false,
                state: string_to_c(state),
                uri: null_mut(),
                position_secs: 0,
                duration_secs: 0,
                mode: null_mut(),
                reason: null_mut(),
            },
            SessionEvent::UriChanged { uri } => Self {
                event_type: FFISessionEventType::SessUriChanged,
                device_id: string_to_c(device_id),
                volume: 0,
                muted: false,
                state: null_mut(),
                uri: string_to_c(uri),
                position_secs: 0,
                duration_secs: 0,
                mode: null_mut(),
                reason: null_mut(),
            },
            SessionEvent::PositionChanged { position_secs } => Self {
                event_type: FFISessionEventType::SessPositionChanged,
                device_id: string_to_c(device_id),
                volume: 0,
                muted: false,
                state: null_mut(),
                uri: null_mut(),
                position_secs: *position_secs,
                duration_secs: 0,
                mode: null_mut(),
                reason: null_mut(),
            },
            SessionEvent::DurationChanged { duration_secs } => Self {
                event_type: FFISessionEventType::SessDurationChanged,
                device_id: string_to_c(device_id),
                volume: 0,
                muted: false,
                state: null_mut(),
                uri: null_mut(),
                position_secs: 0,
                duration_secs: *duration_secs,
                mode: null_mut(),
                reason: null_mut(),
            },
            SessionEvent::PlayModeChanged { mode } => Self {
                event_type: FFISessionEventType::SessPlayModeChanged,
                device_id: string_to_c(device_id),
                volume: 0,
                muted: false,
                state: null_mut(),
                uri: null_mut(),
                position_secs: 0,
                duration_secs: 0,
                mode: string_to_c(mode),
                reason: null_mut(),
            },
            SessionEvent::SessionLost { reason } => Self {
                event_type: FFISessionEventType::SessSessionLost,
                device_id: string_to_c(device_id),
                volume: 0,
                muted: false,
                state: null_mut(),
                uri: null_mut(),
                position_secs: 0,
                duration_secs: 0,
                mode: null_mut(),
                reason: string_to_c(reason),
            },
        }
    }

    /// Free all allocated C strings in this event
    pub(crate) unsafe fn free(&mut self) {
        free_c_string(self.device_id);
        free_c_string(self.state);
        free_c_string(self.uri);
        free_c_string(self.mode);
        free_c_string(self.reason);
    }
}
