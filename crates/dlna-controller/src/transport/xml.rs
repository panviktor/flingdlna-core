/// Extract content from an XML element (simple implementation)
pub(crate) fn extract_xml_element(xml: &str, element: &str) -> Option<String> {
    let start_tag = format!("<{element}");
    let end_tag = format!("</{element}>");

    let start_pos = xml.find(&start_tag)?;
    let content_start = xml[start_pos..].find('>')? + start_pos + 1;
    let content_end = xml[content_start..].find(&end_tag)? + content_start;

    Some(xml[content_start..content_end].trim().to_string())
}

/// Escape special characters for XML
pub(crate) fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
