use super::prelude::{Duration, Error, IpAddr, Ipv4Addr, Result, SocketAddr};
use super::socket::is_multicast_route_error;
use super::xml::parse_device_description;
use crate::net::{
    local_ipv4_bind_candidates, local_ipv4_candidates, log_network_diagnostics,
    log_route_diagnostics,
};
use crate::types::Renderer;
use tracing::{debug, info, warn};
use url::{Host, Url};

/// Fetch renderer information from device description URL
pub async fn fetch_renderer_info(
    location: &str,
    preferred_local: Option<Ipv4Addr>,
) -> Result<Renderer> {
    let url: Url = location.parse()?;
    validate_location_url(&url).await?;

    // Fetch the device description XML directly
    let response = reqwest_get(location, preferred_local).await?;

    // Parse the XML to extract device info
    parse_device_description(&response, url)
}

fn allow_public_ssdp_location() -> bool {
    matches!(
        std::env::var("FLINGDLNA_ALLOW_PUBLIC_SSDP_LOCATION")
            .ok()
            .as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn is_allowed_location_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            if v4.is_unspecified() || v4.is_loopback() || v4.is_multicast() {
                return false;
            }
            // Block common cloud-metadata address explicitly.
            if v4.octets() == [169, 254, 169, 254] {
                return false;
            }
            if allow_public_ssdp_location() {
                return true;
            }
            v4.is_private() || v4.is_link_local() || is_shared_cgnat(v4)
        }
        IpAddr::V6(v6) => {
            if v6.is_unspecified() || v6.is_loopback() || v6.is_multicast() {
                return false;
            }
            if allow_public_ssdp_location() {
                return true;
            }
            v6.is_unique_local() || v6.is_unicast_link_local()
        }
    }
}

fn is_shared_cgnat(ip: Ipv4Addr) -> bool {
    // RFC 6598: 100.64.0.0/10
    let [a, b, ..] = ip.octets();
    a == 100 && (64..=127).contains(&b)
}

async fn validate_location_url(url: &Url) -> Result<()> {
    if url.scheme() != "http" {
        return Err(Error::Network(format!(
            "Unsupported URL scheme '{}' (expected http)",
            url.scheme()
        )));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::Network("Userinfo in URL is not supported".into()));
    }

    let port = url.port().unwrap_or(80);
    if port == 0 {
        return Err(Error::Network("Invalid port 0 in URL".into()));
    }

    match url.host() {
        Some(Host::Ipv4(ip)) => {
            if !is_allowed_location_ip(IpAddr::V4(ip)) {
                return Err(Error::Network(format!(
                    "Rejected LOCATION host {ip} (not a local address)"
                )));
            }
            Ok(())
        }
        Some(Host::Ipv6(ip)) => {
            if !is_allowed_location_ip(IpAddr::V6(ip)) {
                return Err(Error::Network(format!(
                    "Rejected LOCATION host {ip} (not a local address)"
                )));
            }
            Ok(())
        }
        Some(Host::Domain(host)) => {
            if allow_public_ssdp_location() {
                return Ok(());
            }

            let lookup = tokio::time::timeout(
                Duration::from_secs(2),
                tokio::net::lookup_host((host, port)),
            )
            .await
            .map_err(|_| Error::Timeout)??;

            let mut any = false;
            for addr in lookup {
                any = true;
                if !is_allowed_location_ip(addr.ip()) {
                    return Err(Error::Network(format!(
                        "Rejected LOCATION host {} (resolved to non-local address {})",
                        host,
                        addr.ip()
                    )));
                }
            }

            if !any {
                return Err(Error::Network(format!(
                    "Rejected LOCATION host {host} (no resolved addresses)"
                )));
            }

            Ok(())
        }
        None => Err(Error::Network("No host in URL".into())),
    }
}

async fn try_connect_with_local_bind(
    remote_addr: SocketAddr,
    local_ip: Ipv4Addr,
    timeout: Duration,
) -> std::io::Result<tokio::net::TcpStream> {
    if !remote_addr.is_ipv4() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "local bind only supports IPv4",
        ));
    }

    let socket = tokio::net::TcpSocket::new_v4()?;
    let local_addr = SocketAddr::new(IpAddr::V4(local_ip), 0);
    socket.bind(local_addr)?;

    match tokio::time::timeout(timeout, socket.connect(remote_addr)).await {
        Ok(result) => result,
        Err(_) => Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "connect timeout",
        )),
    }
}

fn host_header_for_url(url: &Url) -> Result<String> {
    match url.host() {
        Some(Host::Domain(d)) => Ok(d.to_string()),
        Some(Host::Ipv4(ip)) => Ok(ip.to_string()),
        Some(Host::Ipv6(ip)) => Ok(format!("[{ip}]")),
        None => Err(Error::Network("No host in URL".into())),
    }
}

async fn resolve_remote_addr(url: &Url, port: u16) -> Result<SocketAddr> {
    match url.host() {
        Some(Host::Domain(d)) => {
            let lookup = tokio::net::lookup_host((d, port)).await?;
            lookup
                .into_iter()
                .next()
                .ok_or_else(|| Error::Network(format!("No resolved addresses for {d}")))
        }
        Some(Host::Ipv4(ip)) => Ok(SocketAddr::new(IpAddr::V4(ip), port)),
        Some(Host::Ipv6(ip)) => Ok(SocketAddr::new(IpAddr::V6(ip), port)),
        None => Err(Error::Network("No host in URL".into())),
    }
}

/// Simple HTTP GET request
async fn reqwest_get(url: &str, preferred_local: Option<Ipv4Addr>) -> Result<String> {
    let url: Url = url.parse()?;
    if url.scheme() != "http" {
        return Err(Error::Network(format!(
            "Unsupported URL scheme '{}' (expected http)",
            url.scheme()
        )));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::Network("Userinfo in URL is not supported".into()));
    }
    let port = url.port().unwrap_or(80);
    let path = match url.query() {
        Some(q) => format!("{}?{}", url.path(), q),
        None => url.path().to_string(),
    };

    let host_header = host_header_for_url(&url)?;

    const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
    const READ_TIMEOUT: Duration = Duration::from_secs(5);
    const MAX_RESPONSE_BYTES: usize = 256 * 1024; // device descriptions are typically small

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let remote_addr = resolve_remote_addr(&url, port).await?;

    if let Some(local_ip) = preferred_local {
        info!("Fetching {} (prefer {})", remote_addr, local_ip);
    } else {
        info!("Fetching {}", remote_addr);
    }

    // Diagnostic: log network interfaces and routing
    debug!("SSDP fetch: Network diagnostic info:");
    debug!("SSDP fetch:   Target: {}", remote_addr);
    debug!(
        "SSDP fetch:   Available interfaces: {:?}",
        local_ipv4_candidates()
    );
    debug!(
        "SSDP fetch:   Bind candidates: {:?}",
        local_ipv4_bind_candidates()
    );
    if let Some(pref) = preferred_local {
        debug!("SSDP fetch:   Preferred source: {}", pref);
    }

    // Use async TcpStream for better macOS sandbox compatibility
    debug!("SSDP fetch: attempting async connect to {}", remote_addr);

    // IMPORTANT: IP_BOUND_IF doesn't work in macOS sandbox even with entitlements!
    // For TCP connections, always use simple connect and let the OS choose the route.
    // IP_BOUND_IF is only needed (and works) for UDP multicast send.
    if preferred_local.is_some() {
        debug!("SSDP fetch: preferred interface specified, but using simple connect (sandbox-compatible)");
    }

    let mut stream = {
        debug!("SSDP fetch: using simple connect (sandbox-compatible)");
        match tokio::time::timeout(CONNECT_TIMEOUT, tokio::net::TcpStream::connect(remote_addr))
            .await
        {
            Ok(Ok(s)) => s,
            Ok(Err(err)) => {
                debug!(
                    "SSDP fetch: async connect to {} failed: {} (errno: {:?}, kind: {:?})",
                    remote_addr,
                    err,
                    err.raw_os_error(),
                    err.kind()
                );
                if is_multicast_route_error(&err) {
                    log_network_diagnostics("ssdp_fetch_connect");
                    log_route_diagnostics(Some(remote_addr.ip()));
                }

                let mut fallback_stream = None;
                if let (Some(local_ip), true) = (preferred_local, remote_addr.is_ipv4()) {
                    if is_multicast_route_error(&err) {
                        debug!("SSDP fetch: retrying connect from {}", local_ip);
                        match try_connect_with_local_bind(remote_addr, local_ip, CONNECT_TIMEOUT)
                            .await
                        {
                            Ok(stream) => {
                                debug!("SSDP fetch: bound connect succeeded from {}", local_ip);
                                fallback_stream = Some(stream);
                            }
                            Err(fallback_err) => {
                                debug!(
                                    "SSDP fetch: bound connect failed from {}: {} (errno: {:?}, kind: {:?})",
                                    local_ip,
                                    fallback_err,
                                    fallback_err.raw_os_error(),
                                    fallback_err.kind()
                                );
                            }
                        }
                    }
                }

                if let Some(stream) = fallback_stream {
                    stream
                } else {
                    // Additional macOS diagnostics
                    #[cfg(target_os = "macos")]
                    {
                        if err.raw_os_error() == Some(65) {
                            // EHOSTUNREACH
                            warn!("SSDP fetch: EHOSTUNREACH (65) on macOS - possible causes:");
                            warn!(
                                "SSDP fetch:   - Multiple network interfaces (VPN/Ethernet/WiFi)"
                            );
                            warn!("SSDP fetch:   - Device on different subnet");
                            warn!("SSDP fetch:   - macOS routing selecting wrong interface");
                            warn!(
                                "SSDP fetch: Workaround: Disconnect unused networks or disable VPN"
                            );
                        }
                    }

                    return Err(Error::Io(err));
                }
            }
            Err(_) => {
                debug!("SSDP fetch: connect timeout after {:?}", CONNECT_TIMEOUT);
                return Err(Error::Timeout);
            }
        }
    };

    if let Ok(local_addr) = stream.local_addr() {
        debug!("SSDP fetch: connected from local address: {}", local_addr);
    }

    info!("Connected to {}", remote_addr);

    let request =
        format!("GET {path} HTTP/1.1\r\nHost: {host_header}\r\nConnection: close\r\n\r\n");

    stream.write_all(request.as_bytes()).await?;

    let mut response: Vec<u8> = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = tokio::time::timeout(READ_TIMEOUT, stream.read(&mut buf))
            .await
            .map_err(|_| Error::Timeout)??;
        if n == 0 {
            break;
        }
        if response.len().saturating_add(n) > MAX_RESPONSE_BYTES {
            return Err(Error::InvalidResponse(format!(
                "HTTP response exceeded {MAX_RESPONSE_BYTES} bytes"
            )));
        }
        response.extend_from_slice(&buf[..n]);
    }

    // Skip HTTP headers
    let body = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|idx| &response[idx + 4..])
        .unwrap_or(&response);

    Ok(String::from_utf8_lossy(body).to_string())
}
