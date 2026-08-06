use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr};
use url::{Host, Url};

pub(super) fn format_host_for_url(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => format!("[{v6}]"),
    }
}

pub(super) fn build_location_url(local_ip: IpAddr, http_port: u16) -> String {
    format!(
        "http://{}:{}/description.xml",
        format_host_for_url(local_ip),
        http_port
    )
}

/// Parse LOCATION header from SSDP response
pub(super) fn parse_location(response: &str) -> Option<String> {
    response
        .lines()
        .find(|line| line.to_uppercase().starts_with("LOCATION:"))
        .map(|line| line[9..].trim().to_string())
}

pub(super) fn parse_header_value(response: &str, header: &str) -> Option<String> {
    response.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.trim().eq_ignore_ascii_case(header) {
            Some(value.trim().to_string())
        } else {
            None
        }
    })
}

pub(super) fn parse_search_target(response: &str) -> Option<String> {
    parse_header_value(response, "ST").or_else(|| parse_header_value(response, "NT"))
}

pub(super) fn is_renderer_ssdp_response(st: Option<&str>, usn: Option<&str>) -> bool {
    let st_ok = st.map(is_renderer_ssdp_target).unwrap_or(false);
    let usn_ok = usn.map(is_renderer_ssdp_target).unwrap_or(false);

    if st.is_some() {
        st_ok || usn_ok
    } else if usn.is_some() {
        usn_ok
    } else {
        true
    }
}

fn is_renderer_ssdp_target(value: &str) -> bool {
    let v = value.to_ascii_lowercase();
    if v.contains("mediaserver") {
        return false;
    }
    v.contains("mediarenderer") || v.contains("avtransport") || v.contains("renderingcontrol")
}

pub(super) fn is_own_location(
    location: &str,
    local_ips: &HashSet<Ipv4Addr>,
    http_port: u16,
) -> bool {
    let url: Url = match location.parse() {
        Ok(url) => url,
        Err(_) => return false,
    };
    let port = url.port().unwrap_or(80);
    if port != http_port || http_port == 0 {
        return false;
    }
    match url.host() {
        Some(Host::Ipv4(ip)) => local_ips.contains(&ip),
        _ => false,
    }
}
