use std::time::{Duration, Instant};

use url::Url;

/// UPnP event types
#[derive(Debug, Clone)]
pub enum UpnpEvent {
    /// Volume changed (0-100)
    VolumeChanged { device_udn: String, volume: u8 },
    /// Mute state changed
    MuteChanged { device_udn: String, muted: bool },
    /// Transport state changed (PLAYING, PAUSED, STOPPED, etc.)
    TransportStateChanged { device_udn: String, state: String },
    /// Current track URI changed
    CurrentUriChanged { device_udn: String, uri: String },
    /// Playback position changed (RelativeTimePosition)
    PositionChanged {
        device_udn: String,
        position: Duration,
    },
    /// Track duration changed (CurrentTrackDuration)
    DurationChanged {
        device_udn: String,
        duration: Duration,
    },
    /// Play mode changed (NORMAL, SHUFFLE, REPEAT_ONE, REPEAT_ALL)
    PlayModeChanged { device_udn: String, mode: String },
    /// Subscription expired or failed
    SubscriptionLost { device_udn: String, service: String },
}

/// Active subscription info
#[derive(Debug, Clone)]
pub(crate) struct Subscription {
    /// Subscription ID returned by device
    pub sid: String,
    /// Device UDN
    pub device_udn: String,
    /// Service type (AVTransport or RenderingControl)
    pub service: String,
    /// Event subscription URL
    pub event_url: Url,
    /// When subscription expires
    pub expires_at: Instant,
    /// Timeout duration for renewal
    #[allow(dead_code)]
    pub timeout: Duration,
}
