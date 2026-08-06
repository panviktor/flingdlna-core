use crate::error::{Error, Result};
use std::net::Ipv4Addr;

pub fn best_local_ipv4() -> Result<Ipv4Addr> {
    if let Ok(v) = std::env::var("FLINGDLNA_LOCAL_IPV4") {
        let ip: Ipv4Addr = v
            .trim()
            .parse()
            .map_err(|e| Error::Network(format!("Invalid FLINGDLNA_LOCAL_IPV4 '{v}': {e}")))?;
        if ip.is_unspecified() || ip.is_loopback() || ip.is_multicast() {
            return Err(Error::Network(format!(
                "Invalid FLINGDLNA_LOCAL_IPV4 '{ip}' (must be a LAN IPv4)"
            )));
        }
        return Ok(ip);
    }

    #[cfg(unix)]
    if let Some(ip) = super::unix::best_local_ipv4_unix() {
        return Ok(ip);
    }

    match local_ip_address::local_ip() {
        Ok(std::net::IpAddr::V4(ip)) => Ok(ip),
        Ok(std::net::IpAddr::V6(_)) => Err(Error::Network("No local IPv4 address found".into())),
        Err(e) => Err(Error::Network(format!("Failed to get local IP: {e}"))),
    }
}

pub fn local_ipv4_candidates() -> Vec<Ipv4Addr> {
    #[cfg(unix)]
    let mut out = super::unix::local_ipv4_candidates_unix();
    #[cfg(not(unix))]
    let mut out = Vec::new();

    if out.is_empty() {
        if let Ok(std::net::IpAddr::V4(ip)) = local_ip_address::local_ip() {
            out.push(ip);
        }
    }

    let mut ordered = Vec::new();
    let mut seen = std::collections::HashSet::new();

    if let Ok(v) = std::env::var("FLINGDLNA_LOCAL_IPV4") {
        if let Ok(ip) = v.trim().parse::<Ipv4Addr>() {
            if !ip.is_unspecified() && !ip.is_loopback() && !ip.is_multicast() && seen.insert(ip) {
                ordered.push(ip);
            }
        }
    }

    for ip in out {
        if seen.insert(ip) {
            ordered.push(ip);
        }
    }

    ordered
}

pub fn local_ipv4_broadcasts() -> Vec<(Ipv4Addr, Ipv4Addr)> {
    #[cfg(unix)]
    {
        return super::unix::local_ipv4_broadcasts_unix();
    }

    #[allow(unreachable_code)]
    Vec::new()
}

pub fn local_ipv4_bind_candidates() -> Vec<(Ipv4Addr, u32)> {
    #[cfg(unix)]
    {
        return super::unix::local_ipv4_bind_candidates_unix();
    }

    #[allow(unreachable_code)]
    {
        if let Ok(std::net::IpAddr::V4(ip)) = local_ip_address::local_ip() {
            return vec![(ip, 0)];
        }
        Vec::new()
    }
}

pub fn ifindex_for_ipv4(ip: Ipv4Addr) -> Option<u32> {
    local_ipv4_bind_candidates()
        .into_iter()
        .find(|(candidate, _)| *candidate == ip)
        .map(|(_, ifindex)| ifindex)
}
