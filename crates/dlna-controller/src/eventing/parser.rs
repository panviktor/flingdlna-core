use tokio::sync::broadcast;

use super::types::UpnpEvent;

/// Parse UPnP event XML and emit corresponding events
pub(super) fn parse_and_emit_events(
    xml: &str,
    device_udn: &str,
    event_tx: &broadcast::Sender<UpnpEvent>,
) {
    let Some(last_change_xml) = extract_last_change(xml) else {
        return;
    };

    if let Some(volume) = extract_val_attr(&last_change_xml, b"Volume") {
        if let Ok(volume) = volume.parse::<u8>() {
            let _ = event_tx.send(UpnpEvent::VolumeChanged {
                device_udn: device_udn.to_string(),
                volume,
            });
        }
    }

    if let Some(mute) = extract_val_attr(&last_change_xml, b"Mute") {
        let muted = mute == "1" || mute.eq_ignore_ascii_case("true");
        let _ = event_tx.send(UpnpEvent::MuteChanged {
            device_udn: device_udn.to_string(),
            muted,
        });
    }

    if let Some(state) = extract_val_attr(&last_change_xml, b"TransportState") {
        let _ = event_tx.send(UpnpEvent::TransportStateChanged {
            device_udn: device_udn.to_string(),
            state,
        });
    }

    let uri = extract_val_attr(&last_change_xml, b"CurrentTrackURI")
        .or_else(|| extract_val_attr(&last_change_xml, b"AVTransportURI"));
    if let Some(uri) = uri {
        let _ = event_tx.send(UpnpEvent::CurrentUriChanged {
            device_udn: device_udn.to_string(),
            uri,
        });
    }

    // Parse position (RelativeTimePosition or AbsoluteTimePosition)
    if let Some(pos_str) = extract_val_attr(&last_change_xml, b"RelativeTimePosition")
        .or_else(|| extract_val_attr(&last_change_xml, b"AbsoluteTimePosition"))
    {
        if let Some(position) = crate::transport::parse_duration(&pos_str) {
            tracing::debug!("Event: position changed to {:?}", position);
            let _ = event_tx.send(UpnpEvent::PositionChanged {
                device_udn: device_udn.to_string(),
                position,
            });
        }
    }

    // Parse duration (CurrentTrackDuration or CurrentMediaDuration)
    if let Some(dur_str) = extract_val_attr(&last_change_xml, b"CurrentTrackDuration")
        .or_else(|| extract_val_attr(&last_change_xml, b"CurrentMediaDuration"))
    {
        if let Some(duration) = crate::transport::parse_duration(&dur_str) {
            tracing::debug!("Event: duration changed to {:?}", duration);
            let _ = event_tx.send(UpnpEvent::DurationChanged {
                device_udn: device_udn.to_string(),
                duration,
            });
        }
    }

    // Parse play mode (CurrentPlayMode)
    if let Some(mode) = extract_val_attr(&last_change_xml, b"CurrentPlayMode") {
        tracing::debug!("Event: play mode changed to {}", mode);
        let _ = event_tx.send(UpnpEvent::PlayModeChanged {
            device_udn: device_udn.to_string(),
            mode,
        });
    }
}

fn extract_last_change(xml: &str) -> Option<String> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut in_last_change = false;
    let mut out = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                if e.local_name().as_ref() == b"LastChange" {
                    in_last_change = true;
                }
            }
            Ok(Event::End(e)) => {
                if e.local_name().as_ref() == b"LastChange" {
                    break;
                }
            }
            Ok(Event::Text(e)) if in_last_change => {
                out.push_str(&e.unescape().ok()?);
            }
            Ok(Event::CData(e)) if in_last_change => {
                out.push_str(&String::from_utf8_lossy(e.as_ref()));
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
        buf.clear();
    }

    if out.trim().is_empty() {
        None
    } else {
        Some(out)
    }
}

fn extract_val_attr(last_change_xml: &str, element: &[u8]) -> Option<String> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(last_change_xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                if e.local_name().as_ref() == element {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"val" {
                            if let Ok(v) = attr.unescape_value() {
                                return Some(v.to_string());
                            }
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
        buf.clear();
    }

    None
}
