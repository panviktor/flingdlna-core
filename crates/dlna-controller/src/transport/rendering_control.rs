use dlna_core::{Error, Renderer, Result};
use tracing::debug;

use super::services::{get_rendering_control_url, rendering_control_service_type};
use super::soap::send_soap_action;
use super::xml::extract_xml_element;

// ============== Volume Control (RenderingControl) ==============

/// Get current volume level (0-100)
pub async fn get_volume(renderer: &Renderer) -> Result<u8> {
    let control_url = get_rendering_control_url(renderer)?;
    let service_type = rendering_control_service_type(renderer);

    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:GetVolume xmlns:u="{service_type}">
      <InstanceID>0</InstanceID>
      <Channel>Master</Channel>
    </u:GetVolume>
  </s:Body>
</s:Envelope>"#
    );

    let response = send_soap_action(&control_url, "GetVolume", &body, service_type).await?;

    let volume_str =
        extract_xml_element(&response, "CurrentVolume").unwrap_or_else(|| "0".to_string());

    volume_str
        .parse()
        .map_err(|_| Error::Xml("Invalid volume value".into()))
}

/// Set volume level (0-100)
pub async fn set_volume(renderer: &Renderer, volume: u8) -> Result<()> {
    let control_url = get_rendering_control_url(renderer)?;
    let service_type = rendering_control_service_type(renderer);
    let volume = volume.min(100); // Clamp to 0-100

    debug!("Setting volume to {} on {}", volume, renderer.friendly_name);

    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:SetVolume xmlns:u="{service_type}">
      <InstanceID>0</InstanceID>
      <Channel>Master</Channel>
      <DesiredVolume>{volume}</DesiredVolume>
    </u:SetVolume>
  </s:Body>
</s:Envelope>"#
    );

    send_soap_action(&control_url, "SetVolume", &body, service_type).await?;
    Ok(())
}

/// Get mute state
pub async fn get_mute(renderer: &Renderer) -> Result<bool> {
    let control_url = get_rendering_control_url(renderer)?;
    let service_type = rendering_control_service_type(renderer);

    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:GetMute xmlns:u="{service_type}">
      <InstanceID>0</InstanceID>
      <Channel>Master</Channel>
    </u:GetMute>
  </s:Body>
</s:Envelope>"#
    );

    let response = send_soap_action(&control_url, "GetMute", &body, service_type).await?;

    let mute_str = extract_xml_element(&response, "CurrentMute").unwrap_or_else(|| "0".to_string());

    Ok(mute_str == "1" || mute_str.to_lowercase() == "true")
}

/// Set mute state
pub async fn set_mute(renderer: &Renderer, mute: bool) -> Result<()> {
    let control_url = get_rendering_control_url(renderer)?;
    let service_type = rendering_control_service_type(renderer);

    debug!("Setting mute to {} on {}", mute, renderer.friendly_name);

    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:SetMute xmlns:u="{}">
      <InstanceID>0</InstanceID>
      <Channel>Master</Channel>
      <DesiredMute>{}</DesiredMute>
    </u:SetMute>
  </s:Body>
</s:Envelope>"#,
        service_type,
        if mute { "1" } else { "0" }
    );

    send_soap_action(&control_url, "SetMute", &body, service_type).await?;
    Ok(())
}
