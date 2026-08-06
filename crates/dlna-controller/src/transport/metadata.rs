use super::xml::xml_escape;

/// Generate DIDL-Lite metadata for a URI
pub(crate) fn generate_uri_metadata(
    uri: &str,
    mime_type: Option<&str>,
    title: Option<&str>,
) -> String {
    let mime = mime_type.unwrap_or("video/mp4");
    let fallback_title = uri
        .rsplit('/')
        .next()
        .unwrap_or("Media")
        .split('?')
        .next()
        .unwrap_or("Media");
    let title = title.filter(|t| !t.is_empty()).unwrap_or(fallback_title);

    let upnp_class = if mime.starts_with("video/") {
        "object.item.videoItem"
    } else if mime.starts_with("audio/") {
        "object.item.audioItem.musicTrack"
    } else {
        "object.item"
    };

    format!(
        r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/">
<item id="0" parentID="-1" restricted="1">
<dc:title>{title}</dc:title>
<upnp:class>{upnp_class}</upnp:class>
<res protocolInfo="http-get:*:{mime}:DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000">{uri}</res>
</item>
</DIDL-Lite>"#,
        title = xml_escape(title),
        upnp_class = upnp_class,
        mime = mime,
        uri = xml_escape(uri),
    )
}
