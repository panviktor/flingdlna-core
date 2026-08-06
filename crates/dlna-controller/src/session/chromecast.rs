//! Chromecast renderer session
//!
//! Note: CastDevice is not Send + Sync (uses Rc internally),
//! so we don't use persistent connections. Instead, we use the
//! existing chromecast module functions which create connections as needed.

use super::{RendererProtocol, RendererSession, SessionEvent};
use crate::chromecast::ChromecastDevice;
use async_trait::async_trait;
use dlna_core::{PlaybackInfo, Result, TransportState};
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::info;

/// Chromecast session
pub struct ChromecastSession {
    device: ChromecastDevice,
    /// Event broadcaster
    event_tx: broadcast::Sender<SessionEvent>,
    /// Shutdown signal
    shutdown_tx: Option<tokio::sync::mpsc::Sender<()>>,
}

impl ChromecastSession {
    /// Create a new Chromecast session
    pub async fn new(device: ChromecastDevice) -> Result<Self> {
        info!("Creating Chromecast session for {}", device.name);

        let (event_tx, _) = broadcast::channel(100);
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::channel(1);

        let session = Self {
            device,
            event_tx,
            shutdown_tx: Some(shutdown_tx),
        };

        // Start status polling task (to emit events)
        let device_clone = session.device.clone();
        let event_tx_clone = session.event_tx.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        // Poll status and emit events
                        if let Ok(status) = crate::chromecast::get_status(&device_clone) {
                            // Emit position update event
                            let _ = event_tx_clone.send(SessionEvent::PositionChanged {
                                position_secs: status.position_secs as u64,
                            });

                            // Emit state event
                            let state_str = match status.state {
                                crate::chromecast::PlayState::Playing => "PLAYING",
                                crate::chromecast::PlayState::Paused => "PAUSED",
                                crate::chromecast::PlayState::Buffering => "BUFFERING",
                                crate::chromecast::PlayState::Idle => "STOPPED",
                                crate::chromecast::PlayState::Unknown => "UNKNOWN",
                            };
                            let _ = event_tx_clone.send(SessionEvent::StateChanged {
                                state: state_str.to_string(),
                            });

                            // Emit volume event
                            let volume_percent = (status.volume * 100.0) as u8;
                            let _ = event_tx_clone.send(SessionEvent::VolumeChanged {
                                volume: volume_percent,
                            });

                            let _ = event_tx_clone.send(SessionEvent::MuteChanged {
                                muted: status.muted,
                            });
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("Chromecast {} session polling stopped", device_clone.name);
                        break;
                    }
                }
            }
        });

        Ok(session)
    }
}

#[async_trait]
impl RendererSession for ChromecastSession {
    fn protocol(&self) -> RendererProtocol {
        RendererProtocol::Chromecast
    }

    fn device_id(&self) -> &str {
        &self.device.id
    }

    fn device_name(&self) -> &str {
        &self.device.name
    }

    async fn is_connected(&self) -> bool {
        // Chromecast is connectionless in this implementation
        true
    }

    fn subscribe_events(&self) -> Option<broadcast::Receiver<SessionEvent>> {
        Some(self.event_tx.subscribe())
    }

    async fn play_url(&self, url: &str, content_type: Option<&str>) -> Result<()> {
        tokio::task::spawn_blocking({
            let device = self.device.clone();
            let url = url.to_string();
            let content_type = content_type.map(String::from);
            move || crate::chromecast::load_url(&device, &url, content_type.as_deref(), None)
        })
        .await
        .map_err(|e| dlna_core::Error::Upnp(format!("Task join error: {e}")))?
    }

    async fn play_url_at_position(
        &self,
        url: &str,
        content_type: Option<&str>,
        position_secs: f64,
    ) -> Result<()> {
        // Chromecast supports native start position via LoadOptions.current_time
        tokio::task::spawn_blocking({
            let device = self.device.clone();
            let url = url.to_string();
            let content_type = content_type.map(String::from);
            move || {
                crate::chromecast::load_url_at_position(
                    &device,
                    &url,
                    content_type.as_deref(),
                    Some(position_secs),
                    None,
                )
            }
        })
        .await
        .map_err(|e| dlna_core::Error::Upnp(format!("Task join error: {e}")))?
    }

    async fn play_request(&self, request: &super::PlayRequest) -> Result<()> {
        tokio::task::spawn_blocking({
            let device = self.device.clone();
            let url = request.url.clone();
            let content_type = request.content_type.clone();
            let title = request.title.clone();
            move || {
                crate::chromecast::load_url(
                    &device,
                    &url,
                    content_type.as_deref(),
                    title.as_deref(),
                )
            }
        })
        .await
        .map_err(|e| dlna_core::Error::Upnp(format!("Task join error: {e}")))?
    }

    async fn play_request_at_position(
        &self,
        request: &super::PlayRequest,
        position_secs: f64,
    ) -> Result<()> {
        tokio::task::spawn_blocking({
            let device = self.device.clone();
            let url = request.url.clone();
            let content_type = request.content_type.clone();
            let title = request.title.clone();
            move || {
                crate::chromecast::load_url_at_position(
                    &device,
                    &url,
                    content_type.as_deref(),
                    Some(position_secs),
                    title.as_deref(),
                )
            }
        })
        .await
        .map_err(|e| dlna_core::Error::Upnp(format!("Task join error: {e}")))?
    }

    async fn load_queue(
        &self,
        items: &[super::PlayRequest],
        start_index: usize,
        options: &super::QueueOptions,
    ) -> Result<()> {
        let items = items.to_vec();
        let repeat_mode = options.repeat_mode.clone();
        tokio::task::spawn_blocking({
            let device = self.device.clone();
            move || {
                crate::chromecast::load_queue(&device, &items, start_index, repeat_mode.as_deref())
            }
        })
        .await
        .map_err(|e| dlna_core::Error::Upnp(format!("Task join error: {e}")))?
    }

    async fn play(&self) -> Result<()> {
        tokio::task::spawn_blocking({
            let device = self.device.clone();
            move || crate::chromecast::play(&device)
        })
        .await
        .map_err(|e| dlna_core::Error::Upnp(format!("Task join error: {e}")))?
    }

    async fn pause(&self) -> Result<()> {
        tokio::task::spawn_blocking({
            let device = self.device.clone();
            move || crate::chromecast::pause(&device)
        })
        .await
        .map_err(|e| dlna_core::Error::Upnp(format!("Task join error: {e}")))?
    }

    async fn stop(&self) -> Result<()> {
        tokio::task::spawn_blocking({
            let device = self.device.clone();
            move || crate::chromecast::stop(&device)
        })
        .await
        .map_err(|e| dlna_core::Error::Upnp(format!("Task join error: {e}")))?
    }

    async fn seek(&self, position: Duration) -> Result<()> {
        tokio::task::spawn_blocking({
            let device = self.device.clone();
            let position_secs = position.as_secs_f32();
            move || crate::chromecast::seek(&device, position_secs)
        })
        .await
        .map_err(|e| dlna_core::Error::Upnp(format!("Task join error: {e}")))?
    }

    async fn get_status(&self) -> Result<PlaybackInfo> {
        let status = tokio::task::spawn_blocking({
            let device = self.device.clone();
            move || crate::chromecast::get_status(&device)
        })
        .await
        .map_err(|e| dlna_core::Error::Upnp(format!("Task join error: {e}")))??;

        let state = match status.state {
            crate::chromecast::PlayState::Playing => TransportState::Playing,
            crate::chromecast::PlayState::Paused => TransportState::Paused,
            crate::chromecast::PlayState::Buffering => TransportState::Transitioning,
            crate::chromecast::PlayState::Idle => TransportState::Stopped,
            crate::chromecast::PlayState::Unknown => TransportState::Stopped,
        };

        Ok(PlaybackInfo {
            state,
            position: Duration::from_secs_f32(status.position_secs),
            duration: status
                .duration_secs
                .map(Duration::from_secs_f32)
                .unwrap_or_default(),
            current_uri: None,
        })
    }

    async fn get_volume(&self) -> Result<(u8, bool)> {
        let (volume, muted) = tokio::task::spawn_blocking({
            let device = self.device.clone();
            move || crate::chromecast::get_volume(&device)
        })
        .await
        .map_err(|e| dlna_core::Error::Upnp(format!("Task join error: {e}")))??;

        let volume_percent = (volume * 100.0) as u8;
        Ok((volume_percent, muted))
    }

    async fn set_volume(&self, volume: u8) -> Result<()> {
        tokio::task::spawn_blocking({
            let device = self.device.clone();
            let level = (volume as f32 / 100.0).clamp(0.0, 1.0);
            move || crate::chromecast::set_volume(&device, level)
        })
        .await
        .map_err(|e| dlna_core::Error::Upnp(format!("Task join error: {e}")))?
    }

    async fn set_mute(&self, muted: bool) -> Result<()> {
        // Note: rust_cast doesn't have set_muted, we'd need to add it
        // For now, just return success
        info!(
            "Set mute {} for Chromecast {} (not implemented in rust_cast)",
            muted, self.device.name
        );
        Ok(())
    }

    async fn close(&self) -> Result<()> {
        info!("Closing Chromecast session for {}", self.device.name);

        // Signal polling task to stop
        if let Some(ref tx) = self.shutdown_tx {
            let _ = tx.send(()).await;
        }

        Ok(())
    }
}
