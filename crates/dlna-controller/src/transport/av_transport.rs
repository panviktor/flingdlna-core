use std::time::Duration;

use dlna_core::{DeviceCapabilities, MediaInfo, PlaybackInfo, Renderer, Result, TransportState};
use tracing::debug;

use crate::subtitles::generate_didl_with_subtitle;

use super::metadata::generate_uri_metadata;
use super::services::{av_transport_service_type, get_control_url};
use super::soap::send_soap_action;
use super::time::{format_duration, parse_duration};
use super::xml::{extract_xml_element, xml_escape};

/// Set the URI to play on the renderer
pub async fn set_uri(
    renderer: &Renderer,
    uri: &str,
    mime_type: Option<&str>,
    title: Option<&str>,
) -> Result<()> {
    let control_url = get_control_url(renderer)?;
    let service_type = av_transport_service_type(renderer);

    // Generate DIDL-Lite metadata for the URI
    let metadata = generate_uri_metadata(uri, mime_type, title);

    debug!("Setting URI: {} on {}", uri, renderer.friendly_name);

    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:SetAVTransportURI xmlns:u="{}">
      <InstanceID>0</InstanceID>
      <CurrentURI>{}</CurrentURI>
      <CurrentURIMetaData>{}</CurrentURIMetaData>
    </u:SetAVTransportURI>
  </s:Body>
</s:Envelope>"#,
        service_type,
        xml_escape(uri),
        xml_escape(&metadata)
    );

    send_soap_action(&control_url, "SetAVTransportURI", &body, service_type).await?;
    Ok(())
}

/// Set the URI to play on the renderer with optional subtitle
pub async fn set_uri_with_subtitle(
    renderer: &Renderer,
    uri: &str,
    mime_type: Option<&str>,
    subtitle_uri: Option<&str>,
    title: Option<&str>,
) -> Result<()> {
    let control_url = get_control_url(renderer)?;
    let service_type = av_transport_service_type(renderer);

    let mime = mime_type.unwrap_or("video/mp4");
    let fallback_title = uri
        .rsplit('/')
        .next()
        .unwrap_or("Media")
        .split('?')
        .next()
        .unwrap_or("Media");
    let title = title.filter(|t| !t.is_empty()).unwrap_or(fallback_title);

    // Use subtitle-aware DIDL generation
    let metadata = generate_didl_with_subtitle(uri, mime, title, subtitle_uri);

    if let Some(sub) = subtitle_uri {
        debug!(
            "Setting URI: {} with subtitle: {} on {}",
            uri, sub, renderer.friendly_name
        );
    } else {
        debug!("Setting URI: {} on {}", uri, renderer.friendly_name);
    }

    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:SetAVTransportURI xmlns:u="{}">
      <InstanceID>0</InstanceID>
      <CurrentURI>{}</CurrentURI>
      <CurrentURIMetaData>{}</CurrentURIMetaData>
    </u:SetAVTransportURI>
  </s:Body>
</s:Envelope>"#,
        service_type,
        xml_escape(uri),
        xml_escape(&metadata)
    );

    send_soap_action(&control_url, "SetAVTransportURI", &body, service_type).await?;
    Ok(())
}

/// Set the next URI on the renderer (optional, device-side queue hint)
pub async fn set_next_uri(
    renderer: &Renderer,
    uri: &str,
    mime_type: Option<&str>,
    title: Option<&str>,
) -> Result<()> {
    let control_url = get_control_url(renderer)?;
    let service_type = av_transport_service_type(renderer);

    let metadata = generate_uri_metadata(uri, mime_type, title);

    debug!("Setting Next URI: {} on {}", uri, renderer.friendly_name);

    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:SetNextAVTransportURI xmlns:u="{}">
      <InstanceID>0</InstanceID>
      <NewNextAVTransportURI>{}</NewNextAVTransportURI>
      <NewNextAVTransportURIMetaData>{}</NewNextAVTransportURIMetaData>
    </u:SetNextAVTransportURI>
  </s:Body>
</s:Envelope>"#,
        service_type,
        xml_escape(uri),
        xml_escape(&metadata)
    );

    send_soap_action(&control_url, "SetNextAVTransportURI", &body, service_type).await?;
    Ok(())
}

/// Set the next URI with optional subtitle (device-side queue hint)
pub async fn set_next_uri_with_subtitle(
    renderer: &Renderer,
    uri: &str,
    mime_type: Option<&str>,
    subtitle_uri: Option<&str>,
    title: Option<&str>,
) -> Result<()> {
    let control_url = get_control_url(renderer)?;
    let service_type = av_transport_service_type(renderer);

    let mime = mime_type.unwrap_or("video/mp4");
    let fallback_title = uri
        .rsplit('/')
        .next()
        .unwrap_or("Media")
        .split('?')
        .next()
        .unwrap_or("Media");
    let title = title.filter(|t| !t.is_empty()).unwrap_or(fallback_title);

    let metadata = generate_didl_with_subtitle(uri, mime, title, subtitle_uri);

    if let Some(sub) = subtitle_uri {
        debug!(
            "Setting Next URI: {} with subtitle: {} on {}",
            uri, sub, renderer.friendly_name
        );
    } else {
        debug!("Setting Next URI: {} on {}", uri, renderer.friendly_name);
    }

    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:SetNextAVTransportURI xmlns:u="{}">
      <InstanceID>0</InstanceID>
      <NewNextAVTransportURI>{}</NewNextAVTransportURI>
      <NewNextAVTransportURIMetaData>{}</NewNextAVTransportURIMetaData>
    </u:SetNextAVTransportURI>
  </s:Body>
</s:Envelope>"#,
        service_type,
        xml_escape(uri),
        xml_escape(&metadata)
    );

    send_soap_action(&control_url, "SetNextAVTransportURI", &body, service_type).await?;
    Ok(())
}

/// Start playback
pub async fn play(renderer: &Renderer) -> Result<()> {
    let control_url = get_control_url(renderer)?;
    let service_type = av_transport_service_type(renderer);

    debug!("Play on {}", renderer.friendly_name);

    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:Play xmlns:u="{service_type}">
      <InstanceID>0</InstanceID>
      <Speed>1</Speed>
    </u:Play>
  </s:Body>
</s:Envelope>"#
    );

    send_soap_action(&control_url, "Play", &body, service_type).await?;
    Ok(())
}

/// Pause playback
pub async fn pause(renderer: &Renderer) -> Result<()> {
    let control_url = get_control_url(renderer)?;
    let service_type = av_transport_service_type(renderer);

    debug!("Pause on {}", renderer.friendly_name);

    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:Pause xmlns:u="{service_type}">
      <InstanceID>0</InstanceID>
    </u:Pause>
  </s:Body>
</s:Envelope>"#
    );

    send_soap_action(&control_url, "Pause", &body, service_type).await?;
    Ok(())
}

/// Stop playback
pub async fn stop(renderer: &Renderer) -> Result<()> {
    let control_url = get_control_url(renderer)?;
    let service_type = av_transport_service_type(renderer);

    debug!("Stop on {}", renderer.friendly_name);

    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:Stop xmlns:u="{service_type}">
      <InstanceID>0</InstanceID>
    </u:Stop>
  </s:Body>
</s:Envelope>"#
    );

    send_soap_action(&control_url, "Stop", &body, service_type).await?;
    Ok(())
}

/// Seek to a position
pub async fn seek(renderer: &Renderer, position: Duration) -> Result<()> {
    let control_url = get_control_url(renderer)?;
    let service_type = av_transport_service_type(renderer);

    let time_str = format_duration(position);
    debug!("Seek to {} on {}", time_str, renderer.friendly_name);

    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:Seek xmlns:u="{service_type}">
      <InstanceID>0</InstanceID>
      <Unit>REL_TIME</Unit>
      <Target>{time_str}</Target>
    </u:Seek>
  </s:Body>
</s:Envelope>"#
    );

    send_soap_action(&control_url, "Seek", &body, service_type).await?;
    Ok(())
}

/// Get current position information
pub async fn get_position_info(renderer: &Renderer) -> Result<PlaybackInfo> {
    let control_url = get_control_url(renderer)?;
    let service_type = av_transport_service_type(renderer);

    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:GetPositionInfo xmlns:u="{service_type}">
      <InstanceID>0</InstanceID>
    </u:GetPositionInfo>
  </s:Body>
</s:Envelope>"#
    );

    let response = send_soap_action(&control_url, "GetPositionInfo", &body, service_type).await?;

    let position = extract_xml_element(&response, "RelTime")
        .and_then(|s| parse_duration(&s))
        .unwrap_or(Duration::ZERO);

    let duration = extract_xml_element(&response, "TrackDuration")
        .and_then(|s| parse_duration(&s))
        .unwrap_or(Duration::ZERO);

    let current_uri = extract_xml_element(&response, "TrackURI");

    // Also get transport state
    let state = get_transport_state(renderer)
        .await
        .unwrap_or(TransportState::Unknown);

    Ok(PlaybackInfo {
        position,
        duration,
        state,
        current_uri,
    })
}

/// Get transport state
pub async fn get_transport_state(renderer: &Renderer) -> Result<TransportState> {
    let control_url = get_control_url(renderer)?;
    let service_type = av_transport_service_type(renderer);

    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:GetTransportInfo xmlns:u="{service_type}">
      <InstanceID>0</InstanceID>
    </u:GetTransportInfo>
  </s:Body>
</s:Envelope>"#
    );

    let response = send_soap_action(&control_url, "GetTransportInfo", &body, service_type).await?;

    let state_str = extract_xml_element(&response, "CurrentTransportState")
        .unwrap_or_else(|| "UNKNOWN".to_string());

    Ok(TransportState::from_str(&state_str))
}

/// Get media information (duration, metadata, etc.)
///
/// This retrieves information about the currently loaded media from the device.
/// Use this as a fallback when local metadata parsing (ffprobe) is unavailable.
pub async fn get_media_info(renderer: &Renderer) -> Result<MediaInfo> {
    let control_url = get_control_url(renderer)?;
    let service_type = av_transport_service_type(renderer);

    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:GetMediaInfo xmlns:u="{service_type}">
      <InstanceID>0</InstanceID>
    </u:GetMediaInfo>
  </s:Body>
</s:Envelope>"#
    );

    let response = send_soap_action(&control_url, "GetMediaInfo", &body, service_type).await?;
    parse_media_info_response(&response, &renderer.friendly_name)
}

/// Parse MediaInfo from GetMediaInfo SOAP response
fn parse_media_info_response(response: &str, device_name: &str) -> Result<MediaInfo> {
    // Parse number of tracks (default to 1 if not provided)
    let nr_tracks = extract_xml_element(response, "NrTracks")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    // Parse media duration (optional - many devices don't provide this)
    let media_duration =
        extract_xml_element(response, "MediaDuration").and_then(|s| parse_duration(&s));

    // Parse current URI
    let current_uri = extract_xml_element(response, "CurrentURI");

    // Parse play medium (NETWORK, HDD, CD-DA, etc.)
    let play_medium = extract_xml_element(response, "PlayMedium");

    // Parse next URI (if supported by device)
    let next_uri = extract_xml_element(response, "NextURI");

    debug!(
        "GetMediaInfo for {}: tracks={}, duration={:?}, medium={:?}",
        device_name, nr_tracks, media_duration, play_medium
    );

    Ok(MediaInfo {
        nr_tracks,
        media_duration,
        current_uri,
        play_medium,
        next_uri,
    })
}

/// Get device capabilities (supported media types, record formats, etc.)
///
/// This retrieves the device's supported formats and capabilities.
/// Useful for format negotiation and feature detection.
pub async fn get_device_capabilities(renderer: &Renderer) -> Result<DeviceCapabilities> {
    let control_url = get_control_url(renderer)?;
    let service_type = av_transport_service_type(renderer);

    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:GetDeviceCapabilities xmlns:u="{service_type}">
      <InstanceID>0</InstanceID>
    </u:GetDeviceCapabilities>
  </s:Body>
</s:Envelope>"#
    );

    let response =
        send_soap_action(&control_url, "GetDeviceCapabilities", &body, service_type).await?;
    parse_device_capabilities_response(&response, &renderer.friendly_name)
}

/// Parse DeviceCapabilities from GetDeviceCapabilities SOAP response
fn parse_device_capabilities_response(
    response: &str,
    device_name: &str,
) -> Result<DeviceCapabilities> {
    // Parse play media (comma-separated list, e.g., "NETWORK,HDD")
    let play_media = extract_xml_element(response, "PlayMedia")
        .map(|s| s.split(',').map(|m| m.trim().to_string()).collect())
        .unwrap_or_default();

    // Parse record media (usually "NOT_IMPLEMENTED")
    let rec_media = extract_xml_element(response, "RecMedia")
        .map(|s| s.split(',').map(|m| m.trim().to_string()).collect())
        .unwrap_or_default();

    // Parse record quality modes (usually "NOT_IMPLEMENTED")
    let rec_quality_modes = extract_xml_element(response, "RecQualityModes")
        .map(|s| s.split(',').map(|m| m.trim().to_string()).collect())
        .unwrap_or_default();

    debug!(
        "GetDeviceCapabilities for {}: play_media={:?}, rec_media={:?}",
        device_name, play_media, rec_media
    );

    Ok(DeviceCapabilities {
        play_media,
        rec_media,
        rec_quality_modes,
    })
}

/// Get current transport settings (play mode, record quality mode)
///
/// Returns the current play mode (NORMAL, SHUFFLE, REPEAT_ONE, REPEAT_ALL)
/// Note: Many devices don't support this action or always return "NORMAL"
pub async fn get_transport_settings(renderer: &Renderer) -> Result<String> {
    let control_url = get_control_url(renderer)?;
    let service_type = av_transport_service_type(renderer);

    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:GetTransportSettings xmlns:u="{service_type}">
      <InstanceID>0</InstanceID>
    </u:GetTransportSettings>
  </s:Body>
</s:Envelope>"#
    );

    let response =
        send_soap_action(&control_url, "GetTransportSettings", &body, service_type).await?;

    // Extract PlayMode (NORMAL, SHUFFLE, REPEAT_ONE, REPEAT_ALL, etc.)
    let play_mode =
        extract_xml_element(&response, "PlayMode").unwrap_or_else(|| "NORMAL".to_string());

    debug!(
        "GetTransportSettings for {}: play_mode={}",
        renderer.friendly_name, play_mode
    );

    Ok(play_mode)
}

/// Set play mode (shuffle/repeat)
///
/// # Arguments
/// * `mode` - Play mode: "NORMAL", "SHUFFLE", "REPEAT_ONE", "REPEAT_ALL"
///
/// Note: Many devices don't support this action. Use graceful degradation:
/// - If this fails with UPnP error 401 (Invalid Action), implement shuffle/repeat in app
pub async fn set_play_mode(renderer: &Renderer, mode: &str) -> Result<()> {
    let control_url = get_control_url(renderer)?;
    let service_type = av_transport_service_type(renderer);

    debug!("SetPlayMode for {}: mode={}", renderer.friendly_name, mode);

    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:SetPlayMode xmlns:u="{service_type}">
      <InstanceID>0</InstanceID>
      <NewPlayMode>{mode}</NewPlayMode>
    </u:SetPlayMode>
  </s:Body>
</s:Envelope>"#
    );

    send_soap_action(&control_url, "SetPlayMode", &body, service_type).await?;
    Ok(())
}

/// Skip to next track
///
/// Note: Most DLNA TVs don't support this action (only for multi-track media like playlists)
/// Use app-level queue management as fallback
pub async fn next_track(renderer: &Renderer) -> Result<()> {
    let control_url = get_control_url(renderer)?;
    let service_type = av_transport_service_type(renderer);

    debug!("Next track for {}", renderer.friendly_name);

    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:Next xmlns:u="{service_type}">
      <InstanceID>0</InstanceID>
    </u:Next>
  </s:Body>
</s:Envelope>"#
    );

    send_soap_action(&control_url, "Next", &body, service_type).await?;
    Ok(())
}

/// Skip to previous track
///
/// Note: Most DLNA TVs don't support this action (only for multi-track media like playlists)
/// Use app-level queue management as fallback
pub async fn previous_track(renderer: &Renderer) -> Result<()> {
    let control_url = get_control_url(renderer)?;
    let service_type = av_transport_service_type(renderer);

    debug!("Previous track for {}", renderer.friendly_name);

    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:Previous xmlns:u="{service_type}">
      <InstanceID>0</InstanceID>
    </u:Previous>
  </s:Body>
</s:Envelope>"#
    );

    send_soap_action(&control_url, "Previous", &body, service_type).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_media_info_complete() {
        let response = r#"<?xml version="1.0"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <u:GetMediaInfoResponse xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
      <NrTracks>1</NrTracks>
      <MediaDuration>1:30:45</MediaDuration>
      <CurrentURI>http://192.168.1.100:8080/video.mp4</CurrentURI>
      <CurrentURIMetaData></CurrentURIMetaData>
      <NextURI></NextURI>
      <NextURIMetaData></NextURIMetaData>
      <PlayMedium>NETWORK</PlayMedium>
      <RecordMedium>NOT_IMPLEMENTED</RecordMedium>
      <WriteStatus>NOT_IMPLEMENTED</WriteStatus>
    </u:GetMediaInfoResponse>
  </s:Body>
</s:Envelope>"#;

        let result = parse_media_info_response(response, "Test Device").unwrap();

        assert_eq!(result.nr_tracks, 1);
        assert_eq!(result.media_duration, Some(Duration::from_secs(5445))); // 1:30:45 = 5445 seconds
        assert_eq!(
            result.current_uri,
            Some("http://192.168.1.100:8080/video.mp4".to_string())
        );
        assert_eq!(result.play_medium, Some("NETWORK".to_string()));
        assert_eq!(result.next_uri, Some("".to_string()));
    }

    #[test]
    fn test_parse_media_info_minimal() {
        // Some devices provide minimal info
        let response = r#"<?xml version="1.0"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <u:GetMediaInfoResponse xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
      <NrTracks>0</NrTracks>
      <MediaDuration>0:00:00</MediaDuration>
      <CurrentURI></CurrentURI>
      <CurrentURIMetaData></CurrentURIMetaData>
      <NextURI></NextURI>
      <NextURIMetaData></NextURIMetaData>
      <PlayMedium>NONE</PlayMedium>
      <RecordMedium>NOT_IMPLEMENTED</RecordMedium>
      <WriteStatus>NOT_IMPLEMENTED</WriteStatus>
    </u:GetMediaInfoResponse>
  </s:Body>
</s:Envelope>"#;

        let result = parse_media_info_response(response, "Test Device").unwrap();

        assert_eq!(result.nr_tracks, 0);
        assert_eq!(result.media_duration, Some(Duration::ZERO));
        assert_eq!(result.current_uri, Some("".to_string()));
        assert_eq!(result.play_medium, Some("NONE".to_string()));
    }

    #[test]
    fn test_parse_media_info_missing_duration() {
        // Some devices don't provide MediaDuration
        let response = r#"<?xml version="1.0"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <u:GetMediaInfoResponse xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
      <NrTracks>1</NrTracks>
      <CurrentURI>http://example.com/stream.m3u8</CurrentURI>
      <PlayMedium>NETWORK</PlayMedium>
    </u:GetMediaInfoResponse>
  </s:Body>
</s:Envelope>"#;

        let result = parse_media_info_response(response, "Test Device").unwrap();

        assert_eq!(result.nr_tracks, 1);
        assert_eq!(result.media_duration, None); // Duration not provided
        assert_eq!(
            result.current_uri,
            Some("http://example.com/stream.m3u8".to_string())
        );
        assert_eq!(result.play_medium, Some("NETWORK".to_string()));
    }

    #[test]
    fn test_parse_media_info_no_tracks() {
        // When NrTracks is missing, default to 1
        let response = r#"<?xml version="1.0"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <u:GetMediaInfoResponse xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
      <MediaDuration>0:45:30</MediaDuration>
      <CurrentURI>http://192.168.1.100:8080/audio.mp3</CurrentURI>
      <PlayMedium>HDD</PlayMedium>
    </u:GetMediaInfoResponse>
  </s:Body>
</s:Envelope>"#;

        let result = parse_media_info_response(response, "Test Device").unwrap();

        assert_eq!(result.nr_tracks, 1); // Default value
        assert_eq!(result.media_duration, Some(Duration::from_secs(2730))); // 45:30
        assert_eq!(result.play_medium, Some("HDD".to_string()));
    }

    #[test]
    fn test_parse_media_info_with_next_uri() {
        let response = r#"<?xml version="1.0"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <u:GetMediaInfoResponse xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
      <NrTracks>2</NrTracks>
      <MediaDuration>0:03:45</MediaDuration>
      <CurrentURI>http://example.com/track1.mp3</CurrentURI>
      <NextURI>http://example.com/track2.mp3</NextURI>
      <PlayMedium>NETWORK</PlayMedium>
    </u:GetMediaInfoResponse>
  </s:Body>
</s:Envelope>"#;

        let result = parse_media_info_response(response, "Test Device").unwrap();

        assert_eq!(result.nr_tracks, 2);
        assert_eq!(
            result.next_uri,
            Some("http://example.com/track2.mp3".to_string())
        );
    }

    #[test]
    fn test_parse_device_capabilities_full() {
        let response = r#"<?xml version="1.0"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <u:GetDeviceCapabilitiesResponse xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
      <PlayMedia>NETWORK,HDD,CD-DA</PlayMedia>
      <RecMedia>NOT_IMPLEMENTED</RecMedia>
      <RecQualityModes>NOT_IMPLEMENTED</RecQualityModes>
    </u:GetDeviceCapabilitiesResponse>
  </s:Body>
</s:Envelope>"#;

        let result = parse_device_capabilities_response(response, "Test Device").unwrap();

        assert_eq!(result.play_media, vec!["NETWORK", "HDD", "CD-DA"]);
        assert_eq!(result.rec_media, vec!["NOT_IMPLEMENTED"]);
        assert_eq!(result.rec_quality_modes, vec!["NOT_IMPLEMENTED"]);
    }

    #[test]
    fn test_parse_device_capabilities_network_only() {
        let response = r#"<?xml version="1.0"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <u:GetDeviceCapabilitiesResponse xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
      <PlayMedia>NETWORK</PlayMedia>
      <RecMedia>NOT_IMPLEMENTED</RecMedia>
      <RecQualityModes>NOT_IMPLEMENTED</RecQualityModes>
    </u:GetDeviceCapabilitiesResponse>
  </s:Body>
</s:Envelope>"#;

        let result = parse_device_capabilities_response(response, "Test Device").unwrap();

        assert_eq!(result.play_media, vec!["NETWORK"]);
    }

    #[test]
    fn test_parse_device_capabilities_minimal() {
        // Some devices might not provide all fields
        let response = r#"<?xml version="1.0"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <u:GetDeviceCapabilitiesResponse xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
      <PlayMedia>NETWORK</PlayMedia>
    </u:GetDeviceCapabilitiesResponse>
  </s:Body>
</s:Envelope>"#;

        let result = parse_device_capabilities_response(response, "Test Device").unwrap();

        assert_eq!(result.play_media, vec!["NETWORK"]);
        assert_eq!(result.rec_media, Vec::<String>::new()); // Empty vec
        assert_eq!(result.rec_quality_modes, Vec::<String>::new());
    }

    #[test]
    fn test_parse_device_capabilities_empty() {
        // Edge case: no capabilities at all
        let response = r#"<?xml version="1.0"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <u:GetDeviceCapabilitiesResponse xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
    </u:GetDeviceCapabilitiesResponse>
  </s:Body>
</s:Envelope>"#;

        let result = parse_device_capabilities_response(response, "Test Device").unwrap();

        assert!(result.play_media.is_empty());
        assert!(result.rec_media.is_empty());
        assert!(result.rec_quality_modes.is_empty());
    }
}
