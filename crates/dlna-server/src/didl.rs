//! DIDL-Lite XML generation for ContentDirectory responses

use crate::description::xml_escape;
use dlna_core::MediaFile;
use std::time::Duration;

/// Generate a DIDL-Lite container element
pub fn generate_container(id: &str, parent_id: &str, title: &str, child_count: usize) -> String {
    format!(
        r#"<container id="{id}" parentID="{parent_id}" restricted="1" childCount="{child_count}">
  <dc:title>{title}</dc:title>
  <upnp:class>object.container</upnp:class>
</container>"#,
        id = xml_escape(id),
        parent_id = xml_escape(parent_id),
        title = xml_escape(title),
        child_count = child_count,
    )
}

/// Generate a DIDL-Lite item element for a media file
pub fn generate_item(file: &MediaFile, base_url: &str) -> String {
    let upnp_class = file.media_type.upnp_class();
    let res_url = format!("{}/media/{}", base_url, file.id);
    let protocol_info = format!(
        "http-get:*:{}:DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000",
        file.mime_type
    );

    let duration_attr = file
        .duration
        .map(|d| format!(r#" duration="{}""#, format_duration(d)))
        .unwrap_or_default();

    let mut item = format!(
        r#"<item id="{id}" parentID="{parent_id}" restricted="1">
  <dc:title>{title}</dc:title>
  <upnp:class>{upnp_class}</upnp:class>"#,
        id = xml_escape(&file.id),
        parent_id = xml_escape(&file.parent_id),
        title = xml_escape(file.display_title()),
        upnp_class = upnp_class,
    );

    // Add artist for audio
    if let Some(ref artist) = file.artist {
        item.push_str(&format!(
            "\n  <upnp:artist>{}</upnp:artist>",
            xml_escape(artist)
        ));
    }

    // Add album for audio
    if let Some(ref album) = file.album {
        item.push_str(&format!(
            "\n  <upnp:album>{}</upnp:album>",
            xml_escape(album)
        ));
    }

    // Add resource element
    item.push_str(&format!(
        r#"
  <res protocolInfo="{protocol_info}" size="{size}"{duration}>{url}</res>
</item>"#,
        protocol_info = xml_escape(&protocol_info),
        size = file.size,
        duration = duration_attr,
        url = xml_escape(&res_url),
    ));

    item
}

/// Generate a complete DIDL-Lite document
pub fn generate_didl_lite(items: &[String]) -> String {
    format!(
        r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/">
{items}
</DIDL-Lite>"#,
        items = items.join("\n")
    )
}

/// Format duration as HH:MM:SS
pub fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let secs = secs % 60;
    format!("{hours:02}:{mins:02}:{secs:02}")
}

/// Generate root container items
pub fn generate_root_containers(
    video_count: usize,
    audio_count: usize,
    image_count: usize,
) -> Vec<String> {
    vec![
        generate_container("video", "0", "Video", video_count),
        generate_container("audio", "0", "Music", audio_count),
        generate_container("image", "0", "Photos", image_count),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(Duration::from_secs(0)), "00:00:00");
        assert_eq!(format_duration(Duration::from_secs(61)), "00:01:01");
        assert_eq!(format_duration(Duration::from_secs(3661)), "01:01:01");
    }
}
