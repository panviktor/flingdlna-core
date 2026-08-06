//! ContentDirectory SOAP service implementation

use crate::description::xml_escape;
use crate::didl::{
    generate_container, generate_didl_lite, generate_item, generate_root_containers,
};
use crate::state::ServerState;
use dlna_core::{MediaType, Result};
use std::sync::Arc;
use tracing::debug;

/// Browse request parameters
#[derive(Debug)]
pub struct BrowseRequest {
    pub object_id: String,
    pub browse_flag: BrowseFlag,
    pub filter: String,
    pub starting_index: u32,
    pub requested_count: u32,
    pub sort_criteria: String,
}

#[derive(Debug, PartialEq)]
pub enum BrowseFlag {
    BrowseMetadata,
    BrowseDirectChildren,
}

/// Browse response
#[derive(Debug)]
pub struct BrowseResponse {
    pub result: String,
    pub number_returned: u32,
    pub total_matches: u32,
    pub update_id: u32,
}

impl BrowseResponse {
    /// Convert to SOAP response XML
    pub fn to_soap_response(&self) -> String {
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:BrowseResponse xmlns:u="urn:schemas-upnp-org:service:ContentDirectory:1">
      <Result>{result}</Result>
      <NumberReturned>{number_returned}</NumberReturned>
      <TotalMatches>{total_matches}</TotalMatches>
      <UpdateID>{update_id}</UpdateID>
    </u:BrowseResponse>
  </s:Body>
</s:Envelope>"#,
            result = xml_escape(&self.result),
            number_returned = self.number_returned,
            total_matches = self.total_matches,
            update_id = self.update_id,
        )
    }
}

/// Parse a Browse SOAP request
pub fn parse_browse_request(body: &str) -> Result<BrowseRequest> {
    // Simple XML parsing - in production would use proper XML parser
    let object_id = extract_tag_content(body, "ObjectID").unwrap_or("0".to_string());
    let browse_flag_str = extract_tag_content(body, "BrowseFlag").unwrap_or_default();
    let filter = extract_tag_content(body, "Filter").unwrap_or("*".to_string());
    let starting_index = extract_tag_content(body, "StartingIndex")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let requested_count = extract_tag_content(body, "RequestedCount")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let sort_criteria = extract_tag_content(body, "SortCriteria").unwrap_or_default();

    let browse_flag = if browse_flag_str == "BrowseMetadata" {
        BrowseFlag::BrowseMetadata
    } else {
        BrowseFlag::BrowseDirectChildren
    };

    Ok(BrowseRequest {
        object_id,
        browse_flag,
        filter,
        starting_index,
        requested_count,
        sort_criteria,
    })
}

/// Handle a Browse request
///
/// `client_ip` is used to filter content based on folder visibility settings.
/// If None, only public folders will be visible.
pub fn handle_browse(
    state: &Arc<ServerState>,
    request: &BrowseRequest,
    base_url: &str,
    client_ip: Option<&str>,
) -> Result<BrowseResponse> {
    debug!(
        "Browse request: object_id={}, flag={:?}, client_ip={:?}",
        request.object_id, request.browse_flag, client_ip
    );

    let update_id = state.content_update_id();

    match request.object_id.as_str() {
        "0" => {
            // Root container
            if request.browse_flag == BrowseFlag::BrowseMetadata {
                // Return metadata about root container
                let items = vec![generate_container("0", "-1", "Root", 3)];
                Ok(BrowseResponse {
                    result: generate_didl_lite(&items),
                    number_returned: 1,
                    total_matches: 1,
                    update_id,
                })
            } else {
                // Return child containers (counts filtered by visibility)
                let video_count = state
                    .files_by_type_for_client(MediaType::Video, client_ip)
                    .len();
                let audio_count = state
                    .files_by_type_for_client(MediaType::Audio, client_ip)
                    .len();
                let image_count = state
                    .files_by_type_for_client(MediaType::Image, client_ip)
                    .len();

                let items = generate_root_containers(video_count, audio_count, image_count);
                Ok(BrowseResponse {
                    result: generate_didl_lite(&items),
                    number_returned: 3,
                    total_matches: 3,
                    update_id,
                })
            }
        }
        "video" => browse_media_type(
            state,
            MediaType::Video,
            request,
            base_url,
            update_id,
            client_ip,
        ),
        "audio" => browse_media_type(
            state,
            MediaType::Audio,
            request,
            base_url,
            update_id,
            client_ip,
        ),
        "image" => browse_media_type(
            state,
            MediaType::Image,
            request,
            base_url,
            update_id,
            client_ip,
        ),
        _ => {
            // Check if it's a specific file (with visibility check)
            if let Some(file) = state.get_file_for_client(&request.object_id, client_ip) {
                let items = vec![generate_item(&file, base_url)];
                Ok(BrowseResponse {
                    result: generate_didl_lite(&items),
                    number_returned: 1,
                    total_matches: 1,
                    update_id,
                })
            } else {
                // Unknown object ID or not visible - return empty result
                Ok(BrowseResponse {
                    result: generate_didl_lite(&[]),
                    number_returned: 0,
                    total_matches: 0,
                    update_id,
                })
            }
        }
    }
}

fn browse_media_type(
    state: &Arc<ServerState>,
    media_type: MediaType,
    request: &BrowseRequest,
    base_url: &str,
    update_id: u32,
    client_ip: Option<&str>,
) -> Result<BrowseResponse> {
    let container_id = match media_type {
        MediaType::Video => "video",
        MediaType::Audio => "audio",
        MediaType::Image => "image",
    };

    if request.browse_flag == BrowseFlag::BrowseMetadata {
        let files = state.files_by_type_for_client(media_type, client_ip);
        let items = vec![generate_container(
            container_id,
            "0",
            &media_type.to_string(),
            files.len(),
        )];
        return Ok(BrowseResponse {
            result: generate_didl_lite(&items),
            number_returned: 1,
            total_matches: 1,
            update_id,
        });
    }

    let files = state.files_by_type_for_client(media_type, client_ip);
    let total = files.len() as u32;

    let start = request.starting_index as usize;
    let count = if request.requested_count == 0 {
        files.len()
    } else {
        request.requested_count as usize
    };

    let items: Vec<String> = files
        .iter()
        .skip(start)
        .take(count)
        .map(|f| generate_item(f, base_url))
        .collect();

    let number_returned = items.len() as u32;

    Ok(BrowseResponse {
        result: generate_didl_lite(&items),
        number_returned,
        total_matches: total,
        update_id,
    })
}

/// Extract content from an XML tag (simple implementation)
fn extract_tag_content(xml: &str, tag: &str) -> Option<String> {
    let start_tag = format!("<{tag}");
    let end_tag = format!("</{tag}>");

    let start_pos = xml.find(&start_tag)?;
    let content_start = xml[start_pos..].find('>')? + start_pos + 1;
    let content_end = xml[content_start..].find(&end_tag)? + content_start;

    Some(xml[content_start..content_end].to_string())
}

/// Generate GetSystemUpdateID response
pub fn get_system_update_id_response(update_id: u32) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:GetSystemUpdateIDResponse xmlns:u="urn:schemas-upnp-org:service:ContentDirectory:1">
      <Id>{update_id}</Id>
    </u:GetSystemUpdateIDResponse>
  </s:Body>
</s:Envelope>"#
    )
}

/// Generate GetSearchCapabilities response
pub fn get_search_capabilities_response() -> &'static str {
    r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:GetSearchCapabilitiesResponse xmlns:u="urn:schemas-upnp-org:service:ContentDirectory:1">
      <SearchCaps></SearchCaps>
    </u:GetSearchCapabilitiesResponse>
  </s:Body>
</s:Envelope>"#
}

/// Generate GetSortCapabilities response
pub fn get_sort_capabilities_response() -> &'static str {
    r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:GetSortCapabilitiesResponse xmlns:u="urn:schemas-upnp-org:service:ContentDirectory:1">
      <SortCaps></SortCaps>
    </u:GetSortCapabilitiesResponse>
  </s:Body>
</s:Envelope>"#
}

/// Generate ConnectionManager GetProtocolInfo response
pub fn get_protocol_info_response() -> &'static str {
    r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:GetProtocolInfoResponse xmlns:u="urn:schemas-upnp-org:service:ConnectionManager:1">
      <Source>http-get:*:video/mp4:*,http-get:*:video/x-matroska:*,http-get:*:video/avi:*,http-get:*:audio/mpeg:*,http-get:*:audio/flac:*,http-get:*:image/jpeg:*,http-get:*:image/png:*</Source>
      <Sink></Sink>
    </u:GetProtocolInfoResponse>
  </s:Body>
</s:Envelope>"#
}

/// Generate ConnectionManager GetCurrentConnectionIDs response
pub fn get_current_connection_ids_response() -> &'static str {
    r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:GetCurrentConnectionIDsResponse xmlns:u="urn:schemas-upnp-org:service:ConnectionManager:1">
      <ConnectionIDs>0</ConnectionIDs>
    </u:GetCurrentConnectionIDsResponse>
  </s:Body>
</s:Envelope>"#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_tag_content() {
        let xml = r#"<ObjectID>0</ObjectID>"#;
        assert_eq!(extract_tag_content(xml, "ObjectID"), Some("0".to_string()));
    }
}
