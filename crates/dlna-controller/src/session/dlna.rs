//! DLNA/UPnP renderer session with eventing support

use super::{RendererProtocol, RendererSession, SessionEvent};
use crate::eventing::{EventManager, UpnpEvent};
use async_trait::async_trait;
use dlna_core::{PlaybackInfo, Renderer, Result, TransportState};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{info, warn};

/// DLNA/UPnP renderer session with event subscription
pub struct DlnaSession {
    renderer: Renderer,
    event_manager: Option<Arc<EventManager>>,
    /// Event broadcaster (converts UPnP events to SessionEvents)
    event_tx: broadcast::Sender<SessionEvent>,
    /// Shutdown signal
    shutdown_tx: Option<tokio::sync::mpsc::Sender<()>>,
}

impl DlnaSession {
    /// Create a new DLNA session
    pub async fn new(
        renderer: Renderer,
        event_manager: Option<Arc<EventManager>>,
        local_ip: &str,
    ) -> Result<Self> {
        info!("Creating DLNA session for {}", renderer.friendly_name);

        let (event_tx, _) = broadcast::channel(100);
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::channel(1);

        let session = Self {
            renderer,
            event_manager,
            event_tx,
            shutdown_tx: Some(shutdown_tx),
        };

        // Subscribe to UPnP events if EventManager is available
        if let Some(ref event_mgr) = session.event_manager {
            if let Err(e) = event_mgr.subscribe(&session.renderer, local_ip).await {
                warn!(
                    "Failed to subscribe to events for {}: {}",
                    session.renderer.friendly_name, e
                );
            } else {
                info!(
                    "Subscribed to UPnP events for {}",
                    session.renderer.friendly_name
                );

                // Start event forwarding task
                let mut event_rx = event_mgr.subscribe_events();
                let event_tx_clone = session.event_tx.clone();
                let device_udn = session.renderer.udn.clone();

                tokio::spawn(async move {
                    loop {
                        tokio::select! {
                            result = event_rx.recv() => {
                                match result {
                                    Ok(upnp_event) => {
                                        // Convert UPnP event to SessionEvent and forward
                                        if let Some(session_event) = Self::convert_upnp_event(&upnp_event, &device_udn) {
                                            let _ = event_tx_clone.send(session_event);
                                        }
                                    }
                                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                                        warn!("DLNA event receiver lagged, {} events skipped", skipped);
                                    }
                                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                                        info!("DLNA event channel closed");
                                        break;
                                    }
                                }
                            }
                            _ = shutdown_rx.recv() => {
                                info!("DLNA event forwarding stopped");
                                break;
                            }
                        }
                    }
                });
            }
        }

        Ok(session)
    }

    /// Convert UPnP event to SessionEvent (filter by device UDN)
    fn convert_upnp_event(event: &UpnpEvent, device_udn: &str) -> Option<SessionEvent> {
        match event {
            UpnpEvent::VolumeChanged {
                device_udn: udn,
                volume,
            } if udn == device_udn => Some(SessionEvent::VolumeChanged { volume: *volume }),

            UpnpEvent::MuteChanged {
                device_udn: udn,
                muted,
            } if udn == device_udn => Some(SessionEvent::MuteChanged { muted: *muted }),

            UpnpEvent::TransportStateChanged {
                device_udn: udn,
                state,
            } if udn == device_udn => Some(SessionEvent::StateChanged {
                state: state.clone(),
            }),

            UpnpEvent::CurrentUriChanged {
                device_udn: udn,
                uri,
            } if udn == device_udn => Some(SessionEvent::UriChanged { uri: uri.clone() }),

            UpnpEvent::PositionChanged {
                device_udn: udn,
                position,
            } if udn == device_udn => Some(SessionEvent::PositionChanged {
                position_secs: position.as_secs(),
            }),

            UpnpEvent::DurationChanged {
                device_udn: udn,
                duration,
            } if udn == device_udn => Some(SessionEvent::DurationChanged {
                duration_secs: duration.as_secs(),
            }),

            UpnpEvent::PlayModeChanged {
                device_udn: udn,
                mode,
            } if udn == device_udn => Some(SessionEvent::PlayModeChanged { mode: mode.clone() }),

            UpnpEvent::SubscriptionLost {
                device_udn: udn,
                service,
            } if udn == device_udn => Some(SessionEvent::SessionLost {
                reason: format!("Subscription lost for service: {service}"),
            }),

            _ => None, // Event is for a different device
        }
    }
}

#[async_trait]
impl RendererSession for DlnaSession {
    fn protocol(&self) -> RendererProtocol {
        RendererProtocol::Dlna
    }

    fn device_id(&self) -> &str {
        &self.renderer.udn
    }

    fn device_name(&self) -> &str {
        &self.renderer.friendly_name
    }

    async fn is_connected(&self) -> bool {
        // DLNA is connectionless, always "connected" if URLs are valid
        self.renderer.av_transport_url.is_some()
    }

    fn subscribe_events(&self) -> Option<broadcast::Receiver<SessionEvent>> {
        Some(self.event_tx.subscribe())
    }

    async fn play_url(&self, url: &str, content_type: Option<&str>) -> Result<()> {
        use crate::transport;
        transport::set_uri(&self.renderer, url, content_type, None).await?;
        transport::play(&self.renderer).await
    }

    async fn play_url_at_position(
        &self,
        url: &str,
        content_type: Option<&str>,
        position_secs: f64,
    ) -> Result<()> {
        use crate::transport;

        // DLNA doesn't support native start position, so we:
        // 1. SetURI + Play
        // 2. Wait for PLAYING state with position > 0
        // 3. Seek to target position

        transport::set_uri(&self.renderer, url, content_type, None).await?;
        transport::play(&self.renderer).await?;

        // Wait for transport to be ready (PLAYING state and position > 0)
        // This avoids 701 "Transition not available" errors
        for attempt in 1..=20 {
            tokio::time::sleep(Duration::from_millis(300)).await;

            if let Ok(info) = transport::get_position_info(&self.renderer).await {
                if info.state == TransportState::Playing && info.position.as_secs() > 0 {
                    info!(
                        "DLNA ready to seek after {} attempts, position={}s",
                        attempt,
                        info.position.as_secs()
                    );
                    break;
                }
            }
        }

        // Small extra delay for stability
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Seek to target position
        transport::seek(&self.renderer, Duration::from_secs_f64(position_secs)).await
    }

    async fn play_request(&self, request: &super::PlayRequest) -> Result<()> {
        use crate::transport;
        transport::set_uri_with_subtitle(
            &self.renderer,
            &request.url,
            request.content_type.as_deref(),
            request.subtitle_url.as_deref(),
            request.title.as_deref(),
        )
        .await?;
        transport::play(&self.renderer).await
    }

    async fn play_request_at_position(
        &self,
        request: &super::PlayRequest,
        position_secs: f64,
    ) -> Result<()> {
        use crate::transport;

        transport::set_uri_with_subtitle(
            &self.renderer,
            &request.url,
            request.content_type.as_deref(),
            request.subtitle_url.as_deref(),
            request.title.as_deref(),
        )
        .await?;
        transport::play(&self.renderer).await?;

        for attempt in 1..=20 {
            tokio::time::sleep(Duration::from_millis(300)).await;

            if let Ok(info) = transport::get_position_info(&self.renderer).await {
                if info.state == TransportState::Playing && info.position.as_secs() > 0 {
                    info!(
                        "DLNA ready to seek after {} attempts, position={}s",
                        attempt,
                        info.position.as_secs()
                    );
                    break;
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
        transport::seek(&self.renderer, Duration::from_secs_f64(position_secs)).await
    }

    async fn play(&self) -> Result<()> {
        use crate::transport;
        transport::play(&self.renderer).await
    }

    async fn pause(&self) -> Result<()> {
        use crate::transport;
        transport::pause(&self.renderer).await
    }

    async fn stop(&self) -> Result<()> {
        use crate::transport;
        transport::stop(&self.renderer).await
    }

    async fn seek(&self, position: Duration) -> Result<()> {
        use crate::transport;
        transport::seek(&self.renderer, position).await
    }

    async fn get_status(&self) -> Result<PlaybackInfo> {
        use crate::transport;
        transport::get_position_info(&self.renderer).await
    }

    async fn get_volume(&self) -> Result<(u8, bool)> {
        use crate::transport;
        let volume = transport::get_volume(&self.renderer).await?;
        let muted = transport::get_mute(&self.renderer).await?;
        Ok((volume, muted))
    }

    async fn set_volume(&self, volume: u8) -> Result<()> {
        use crate::transport;
        transport::set_volume(&self.renderer, volume).await
    }

    async fn set_mute(&self, muted: bool) -> Result<()> {
        use crate::transport;
        transport::set_mute(&self.renderer, muted).await
    }

    async fn get_play_mode(&self) -> Result<String> {
        use crate::transport;
        transport::get_transport_settings(&self.renderer).await
    }

    async fn set_play_mode(&self, mode: &str) -> Result<()> {
        use crate::transport;
        transport::set_play_mode(&self.renderer, mode).await
    }

    async fn next_track(&self) -> Result<()> {
        use crate::transport;
        transport::next_track(&self.renderer).await
    }

    async fn previous_track(&self) -> Result<()> {
        use crate::transport;
        transport::previous_track(&self.renderer).await
    }

    async fn set_next_request(&self, request: &super::PlayRequest) -> Result<()> {
        use crate::transport;
        transport::set_next_uri_with_subtitle(
            &self.renderer,
            &request.url,
            request.content_type.as_deref(),
            request.subtitle_url.as_deref(),
            request.title.as_deref(),
        )
        .await
    }

    async fn load_queue(
        &self,
        items: &[super::PlayRequest],
        start_index: usize,
        _options: &super::QueueOptions,
    ) -> Result<()> {
        let next_index = start_index.saturating_add(1);
        let Some(next_item) = items.get(next_index) else {
            return Ok(());
        };
        if let Err(err) = self.set_next_request(next_item).await {
            warn!(
                "SetNextAVTransportURI not supported by {}: {}",
                self.renderer.friendly_name, err
            );
        }
        Ok(())
    }

    async fn close(&self) -> Result<()> {
        info!("Closing DLNA session for {}", self.renderer.friendly_name);

        // Signal event forwarding task to stop
        if let Some(ref tx) = self.shutdown_tx {
            let _ = tx.send(()).await;
        }

        // Unsubscribe from events
        if let Some(ref event_mgr) = self.event_manager {
            if let Err(e) = event_mgr.unsubscribe(&self.renderer).await {
                warn!(
                    "Failed to unsubscribe from events for {}: {}",
                    self.renderer.friendly_name, e
                );
            }
        }

        Ok(())
    }
}
