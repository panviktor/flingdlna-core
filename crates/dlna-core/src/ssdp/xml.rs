use super::prelude::{Error, Result};
use crate::types::Renderer;
use url::Url;

/// Decode HTML entities in a string
fn decode_html_entities(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&#34;", "\"")
}

/// Parse device description XML
pub(super) fn parse_device_description(xml: &str, location: Url) -> Result<Renderer> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    // Log device description XML only at trace level (not in release)
    tracing::trace!("Device description XML from {}:\n{}", location, xml);

    #[derive(Clone, Copy)]
    enum TextTarget {
        FriendlyName,
        Udn,
        Manufacturer,
        ModelName,
        ModelNumber,
        ModelDescription,
        SerialNumber,
        PresentationUrl,
        DeviceType,
        ServiceType,
        ControlUrl,
        EventSubUrl,
        IconMimeType,
        IconWidth,
        IconHeight,
        IconDepth,
        IconUrl,
    }

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();

    let mut friendly_name: Option<String> = None;
    let mut udn: Option<String> = None;
    let mut manufacturer: Option<String> = None;
    let mut model_name: Option<String> = None;
    let mut model_number: Option<String> = None;
    let mut model_description: Option<String> = None;
    let mut serial_number: Option<String> = None;
    let mut presentation_url: Option<String> = None;
    let mut is_media_renderer = false;

    let mut av_transport_url: Option<Url> = None;
    let mut rendering_control_url: Option<Url> = None;
    let mut av_transport_service_type: Option<String> = None;
    let mut rendering_control_service_type: Option<String> = None;
    let mut av_transport_event_url: Option<Url> = None;
    let mut rendering_control_event_url: Option<Url> = None;

    let mut in_service = false;
    let mut service_type: Option<String> = None;
    let mut control_url: Option<String> = None;
    let mut event_sub_url: Option<String> = None;

    let mut in_icon = false;
    let mut icon_mime_type: Option<String> = None;
    let mut icon_width: Option<u32> = None;
    let mut icon_height: Option<u32> = None;
    let mut icon_depth: Option<u32> = None;
    let mut icon_url: Option<String> = None;
    let mut icons: Vec<crate::types::DeviceIcon> = Vec::new();

    let mut target: Option<TextTarget> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                b"friendlyName" => target = Some(TextTarget::FriendlyName),
                b"UDN" => target = Some(TextTarget::Udn),
                b"manufacturer" => target = Some(TextTarget::Manufacturer),
                b"modelName" => target = Some(TextTarget::ModelName),
                b"modelNumber" => target = Some(TextTarget::ModelNumber),
                b"modelDescription" => target = Some(TextTarget::ModelDescription),
                b"serialNumber" => target = Some(TextTarget::SerialNumber),
                b"presentationURL" => target = Some(TextTarget::PresentationUrl),
                b"deviceType" => target = Some(TextTarget::DeviceType),
                b"service" => {
                    in_service = true;
                    service_type = None;
                    control_url = None;
                    event_sub_url = None;
                }
                b"serviceType" if in_service => target = Some(TextTarget::ServiceType),
                b"controlURL" if in_service => target = Some(TextTarget::ControlUrl),
                b"eventSubURL" if in_service => target = Some(TextTarget::EventSubUrl),
                b"icon" => {
                    in_icon = true;
                    icon_mime_type = None;
                    icon_width = None;
                    icon_height = None;
                    icon_depth = None;
                    icon_url = None;
                }
                b"mimetype" if in_icon => target = Some(TextTarget::IconMimeType),
                b"width" if in_icon => target = Some(TextTarget::IconWidth),
                b"height" if in_icon => target = Some(TextTarget::IconHeight),
                b"depth" if in_icon => target = Some(TextTarget::IconDepth),
                b"url" if in_icon => target = Some(TextTarget::IconUrl),
                _ => {}
            },
            Ok(Event::End(e)) => {
                let local = e.local_name();
                let local = local.as_ref();
                if local == b"icon" {
                    in_icon = false;
                    // If we have all required icon fields, create a DeviceIcon
                    if let (Some(mime), Some(w), Some(h), Some(d), Some(url_str)) = (
                        icon_mime_type.as_ref(),
                        icon_width,
                        icon_height,
                        icon_depth,
                        icon_url.as_ref(),
                    ) {
                        if let Ok(url) = location.join(url_str) {
                            icons.push(crate::types::DeviceIcon {
                                mime_type: mime.clone(),
                                width: w,
                                height: h,
                                depth: d,
                                url,
                            });
                        }
                    }
                } else if local == b"service" {
                    in_service = false;
                    let Some(svc_type) = service_type.as_deref() else {
                        buf.clear();
                        continue;
                    };

                    let is_avt = svc_type.contains("AVTransport");
                    let is_rc = svc_type.contains("RenderingControl");

                    if is_avt || is_rc {
                        if is_avt {
                            if av_transport_service_type.is_none() {
                                av_transport_service_type = Some(svc_type.to_string());
                            }
                        } else if rendering_control_service_type.is_none() {
                            rendering_control_service_type = Some(svc_type.to_string());
                        }

                        if let Some(ref path) = control_url {
                            if let Ok(url) = location.join(path) {
                                if is_avt {
                                    av_transport_url = Some(url);
                                } else {
                                    rendering_control_url = Some(url);
                                }
                            }
                        }
                        if let Some(ref path) = event_sub_url {
                            if let Ok(url) = location.join(path) {
                                if is_avt {
                                    av_transport_event_url = Some(url);
                                } else {
                                    rendering_control_event_url = Some(url);
                                }
                            }
                        }
                    }
                } else {
                    target = match (target, local) {
                        (Some(TextTarget::FriendlyName), b"friendlyName") => None,
                        (Some(TextTarget::Udn), b"UDN") => None,
                        (Some(TextTarget::Manufacturer), b"manufacturer") => None,
                        (Some(TextTarget::ModelName), b"modelName") => None,
                        (Some(TextTarget::ModelNumber), b"modelNumber") => None,
                        (Some(TextTarget::ModelDescription), b"modelDescription") => None,
                        (Some(TextTarget::SerialNumber), b"serialNumber") => None,
                        (Some(TextTarget::PresentationUrl), b"presentationURL") => None,
                        (Some(TextTarget::DeviceType), b"deviceType") => None,
                        (Some(TextTarget::ServiceType), b"serviceType") => None,
                        (Some(TextTarget::ControlUrl), b"controlURL") => None,
                        (Some(TextTarget::EventSubUrl), b"eventSubURL") => None,
                        (Some(TextTarget::IconMimeType), b"mimetype") => None,
                        (Some(TextTarget::IconWidth), b"width") => None,
                        (Some(TextTarget::IconHeight), b"height") => None,
                        (Some(TextTarget::IconDepth), b"depth") => None,
                        (Some(TextTarget::IconUrl), b"url") => None,
                        _ => target,
                    };
                }
            }
            Ok(Event::Text(e)) => {
                if let Some(t) = target {
                    let text = e
                        .unescape()
                        .map_err(|e| Error::Xml(e.to_string()))?
                        .trim()
                        .to_string();
                    match t {
                        TextTarget::FriendlyName => {
                            if friendly_name.is_none() && !text.is_empty() {
                                friendly_name = Some(decode_html_entities(&text));
                            }
                        }
                        TextTarget::Udn => {
                            if udn.is_none() && !text.is_empty() {
                                udn = Some(text);
                            }
                        }
                        TextTarget::Manufacturer => {
                            if manufacturer.is_none() && !text.is_empty() {
                                manufacturer = Some(text);
                            }
                        }
                        TextTarget::ModelName => {
                            if model_name.is_none() && !text.is_empty() {
                                model_name = Some(text);
                            }
                        }
                        TextTarget::ModelNumber => {
                            if model_number.is_none() && !text.is_empty() {
                                model_number = Some(text);
                            }
                        }
                        TextTarget::ModelDescription => {
                            if model_description.is_none() && !text.is_empty() {
                                model_description = Some(text);
                            }
                        }
                        TextTarget::SerialNumber => {
                            if serial_number.is_none() && !text.is_empty() {
                                serial_number = Some(text);
                            }
                        }
                        TextTarget::PresentationUrl => {
                            if presentation_url.is_none() && !text.is_empty() {
                                presentation_url = Some(text);
                            }
                        }
                        TextTarget::DeviceType => {
                            if !text.is_empty() && text.contains("MediaRenderer") {
                                is_media_renderer = true;
                            }
                        }
                        TextTarget::ServiceType => {
                            if service_type.is_none() && !text.is_empty() {
                                service_type = Some(text);
                            }
                        }
                        TextTarget::ControlUrl => {
                            if control_url.is_none() && !text.is_empty() {
                                control_url = Some(text);
                            }
                        }
                        TextTarget::EventSubUrl => {
                            if event_sub_url.is_none() && !text.is_empty() {
                                event_sub_url = Some(text);
                            }
                        }
                        TextTarget::IconMimeType => {
                            if icon_mime_type.is_none() && !text.is_empty() {
                                icon_mime_type = Some(text);
                            }
                        }
                        TextTarget::IconWidth => {
                            if icon_width.is_none() && !text.is_empty() {
                                icon_width = text.parse().ok();
                            }
                        }
                        TextTarget::IconHeight => {
                            if icon_height.is_none() && !text.is_empty() {
                                icon_height = text.parse().ok();
                            }
                        }
                        TextTarget::IconDepth => {
                            if icon_depth.is_none() && !text.is_empty() {
                                icon_depth = text.parse().ok();
                            }
                        }
                        TextTarget::IconUrl => {
                            if icon_url.is_none() && !text.is_empty() {
                                icon_url = Some(text);
                            }
                        }
                    }
                }
            }
            Ok(Event::CData(e)) => {
                if let Some(t) = target {
                    let text = String::from_utf8_lossy(e.as_ref()).trim().to_string();
                    if text.is_empty() {
                        buf.clear();
                        continue;
                    }
                    match t {
                        TextTarget::FriendlyName => {
                            if friendly_name.is_none() {
                                friendly_name = Some(decode_html_entities(&text));
                            }
                        }
                        TextTarget::Udn => {
                            if udn.is_none() {
                                udn = Some(text);
                            }
                        }
                        TextTarget::Manufacturer => {
                            if manufacturer.is_none() {
                                manufacturer = Some(text);
                            }
                        }
                        TextTarget::ModelName => {
                            if model_name.is_none() {
                                model_name = Some(text);
                            }
                        }
                        TextTarget::ModelNumber => {
                            if model_number.is_none() {
                                model_number = Some(text);
                            }
                        }
                        TextTarget::ModelDescription => {
                            if model_description.is_none() {
                                model_description = Some(text);
                            }
                        }
                        TextTarget::SerialNumber => {
                            if serial_number.is_none() {
                                serial_number = Some(text);
                            }
                        }
                        TextTarget::PresentationUrl => {
                            if presentation_url.is_none() {
                                presentation_url = Some(text);
                            }
                        }
                        TextTarget::DeviceType => {
                            if text.contains("MediaRenderer") {
                                is_media_renderer = true;
                            }
                        }
                        TextTarget::ServiceType => {
                            if service_type.is_none() {
                                service_type = Some(text);
                            }
                        }
                        TextTarget::ControlUrl => {
                            if control_url.is_none() {
                                control_url = Some(text);
                            }
                        }
                        TextTarget::EventSubUrl => {
                            if event_sub_url.is_none() {
                                event_sub_url = Some(text);
                            }
                        }
                        TextTarget::IconMimeType => {
                            if icon_mime_type.is_none() {
                                icon_mime_type = Some(text);
                            }
                        }
                        TextTarget::IconWidth => {
                            if icon_width.is_none() {
                                icon_width = text.parse().ok();
                            }
                        }
                        TextTarget::IconHeight => {
                            if icon_height.is_none() {
                                icon_height = text.parse().ok();
                            }
                        }
                        TextTarget::IconDepth => {
                            if icon_depth.is_none() {
                                icon_depth = text.parse().ok();
                            }
                        }
                        TextTarget::IconUrl => {
                            if icon_url.is_none() {
                                icon_url = Some(text);
                            }
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(Error::Xml(e.to_string())),
            _ => {}
        }
        buf.clear();
    }

    let friendly_name = friendly_name.unwrap_or_else(|| "Unknown Device".to_string());
    let udn = udn.unwrap_or_else(|| format!("uuid:{}", uuid::Uuid::new_v4()));

    if !is_media_renderer {
        return Err(Error::InvalidResponse(
            "Not a MediaRenderer device".to_string(),
        ));
    }
    if av_transport_url.is_none() {
        return Err(Error::InvalidResponse(
            "MediaRenderer without AVTransport service".to_string(),
        ));
    }

    // Log parsed icons for debugging
    tracing::info!("Found {} device icon(s)", icons.len());
    for icon in &icons {
        tracing::info!(
            "  Icon: {}x{} {}bpp, {}, URL: {}",
            icon.width,
            icon.height,
            icon.depth,
            icon.mime_type,
            icon.url
        );
    }

    // Parse presentation URL (relative to location)
    let presentation = presentation_url
        .as_deref()
        .and_then(|url_str| location.join(url_str).ok());

    // Log additional device info
    if let Some(ref num) = model_number {
        tracing::info!("  Model number: {}", num);
    }
    if let Some(ref desc) = model_description {
        tracing::info!("  Model description: {}", desc);
    }
    if let Some(ref serial) = serial_number {
        tracing::info!("  Serial number: {}", serial);
    }
    if let Some(ref pres_url) = presentation {
        tracing::info!("  Presentation URL: {}", pres_url);
    }

    Ok(Renderer {
        friendly_name,
        location,
        udn,
        av_transport_url,
        av_transport_service_type,
        rendering_control_url,
        rendering_control_service_type,
        av_transport_event_url,
        rendering_control_event_url,
        manufacturer,
        model_name,
        model_number,
        model_description,
        serial_number,
        presentation_url: presentation,
        mac_address: None,
        icons,
        firmware_version: None, // DLNA devices don't expose firmware version in device description
        capabilities: None,     // DLNA capabilities are queried via GetDeviceCapabilities
    })
}
