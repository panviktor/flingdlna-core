use super::fetch::fetch_renderer_info;
use super::parse::{
    is_own_location, is_renderer_ssdp_response, parse_header_value, parse_location,
    parse_search_target,
};
use super::prelude::{Duration, Error, IpAddr, Ipv4Addr, Result, SocketAddr};
use super::socket::{
    create_multicast_sender_sockets, is_multicast_route_error, send_to_with_pktinfo,
};
use super::{SsdpService, MSEARCH_TARGETS, SSDP_MULTICAST_ADDR, SSDP_PORT};
use crate::net::{local_ipv4_broadcasts, local_ipv4_candidates, log_network_diagnostics};
use crate::types::Renderer;
use futures::stream::{FuturesUnordered, StreamExt};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, Semaphore};
use tracing::{debug, info, trace, warn};

type FetchTask = tokio::task::JoinHandle<(String, Result<Renderer>)>;

struct SsdpResponse {
    location: String,
    iface_ip: Ipv4Addr,
    st: Option<String>,
    usn: Option<String>,
}

impl SsdpService {
    /// Discover DLNA renderers on the network
    pub async fn discover_renderers(&self, timeout: Duration) -> Result<Vec<Renderer>> {
        self.discover_renderers_impl(timeout, None).await
    }

    /// Discover DLNA renderers with progressive updates via channel
    pub async fn discover_renderers_progressive(
        &self,
        timeout: Duration,
        tx: mpsc::Sender<Renderer>,
    ) -> Result<Vec<Renderer>> {
        self.discover_renderers_impl(timeout, Some(tx)).await
    }

    /// Internal implementation of renderer discovery
    async fn discover_renderers_impl(
        &self,
        timeout: Duration,
        progressive_tx: Option<mpsc::Sender<Renderer>>,
    ) -> Result<Vec<Renderer>> {
        debug!("Starting renderer discovery (timeout: {:?})", timeout);

        let started_at = tokio::time::Instant::now();
        let ssdp_deadline = started_at + timeout;
        let fetch_grace = timeout.min(Duration::from_secs(6));
        let fetch_deadline = ssdp_deadline + fetch_grace;

        let mut local_ips: HashSet<Ipv4Addr> = local_ipv4_candidates().into_iter().collect();
        if let IpAddr::V4(v4) = self.local_ip {
            local_ips.insert(v4);
        }
        let self_udn = self
            .server_info
            .read()
            .ok()
            .and_then(|guard| guard.as_ref().map(|info| info.udn.clone()));

        let sockets = create_multicast_sender_sockets(None).await?;
        if sockets.is_empty() {
            return Err(Error::Network(
                "Failed to create SSDP sender sockets".into(),
            ));
        }

        info!(
            "SSDP discovery: interfaces={:?}, timeout={:?}",
            sockets.iter().map(|(ip, _)| *ip).collect::<Vec<_>>(),
            timeout
        );
        let broadcasts = local_ipv4_broadcasts();
        if !broadcasts.is_empty() {
            info!("SSDP discovery: broadcasts={:?}", broadcasts);
        }

        let multicast_addr = SocketAddr::new(IpAddr::V4(SSDP_MULTICAST_ADDR), SSDP_PORT);
        let any_send_ok =
            send_msearch_attempts(&sockets, &broadcasts, multicast_addr, timeout).await;
        if !any_send_ok {
            log_network_diagnostics("ssdp_discovery_send_failed");
            return Err(Error::Network("Failed to send SSDP discovery".into()));
        }

        let mut seen_locations: HashSet<String> = HashSet::new();
        let (mut rx, recv_packets, recv_no_location) = spawn_ssdp_receivers(sockets, ssdp_deadline);

        let per_fetch_timeout = Duration::from_secs(8);
        let fetch_sem = Arc::new(Semaphore::new(8));
        let mut renderers: HashMap<String, Renderer> = HashMap::new();
        let mut fetches: FuturesUnordered<FetchTask> = FuturesUnordered::new();

        let mut rx_closed = false;
        let mut fetch_failures_logged: usize = 0;
        let mut failure_kinds: HashMap<&'static str, usize> = HashMap::new();

        loop {
            if fetch_deadline
                .saturating_duration_since(tokio::time::Instant::now())
                .is_zero()
            {
                break;
            }

            if rx_closed && fetches.is_empty() {
                break;
            }

            tokio::select! {
                _ = tokio::time::sleep_until(fetch_deadline) => {
                    break;
                }
                msg = rx.recv(), if !rx_closed => {
                    match msg {
                        Some(resp) => {
                            let location = resp.location;
                            let iface_ip = resp.iface_ip;
                            let st = resp.st.as_deref();
                            let usn = resp.usn.as_deref();

                            if !is_renderer_ssdp_response(st, usn) {
                                trace!(
                                    "SSDP discovery: skip LOCATION={} (st={:?}, usn={:?})",
                                    location,
                                    st,
                                    usn
                                );
                                continue;
                            }

                            if let (Some(usn), Some(self_udn)) =
                                (usn, self_udn.as_deref())
                            {
                                if usn.contains(self_udn) {
                                    trace!("SSDP discovery: skip self USN {}", usn);
                                    continue;
                                }
                            }

                            if is_own_location(&location, &local_ips, self.http_port) {
                                trace!("SSDP discovery: skip self LOCATION={}", location);
                                continue;
                            }

                            if !seen_locations.insert(location.clone()) {
                                continue;
                            }
                            if seen_locations.len() <= 10 {
                                info!("SSDP discovery: LOCATION={} via {}", location, iface_ip);
                            } else {
                                debug!("SSDP discovery: LOCATION={} via {}", location, iface_ip);
                            }

                            let sem = fetch_sem.clone();
                            let remaining = fetch_deadline
                                .saturating_duration_since(tokio::time::Instant::now());
                            let timeout = per_fetch_timeout.min(remaining);
                            fetches.push(tokio::spawn(async move {
                                let _permit = sem.acquire().await.expect("semaphore closed");
                                let res = match tokio::time::timeout(
                                    timeout,
                                    fetch_renderer_info(&location, Some(iface_ip)),
                                )
                                .await
                                {
                                    Ok(r) => r,
                                    Err(_) => Err(Error::Timeout),
                                };
                                (location, res)
                            }));
                        }
                        None => {
                            rx_closed = true;
                        }
                    }
                }
                Some(joined) = fetches.next(), if !fetches.is_empty() => {
                    match joined {
                        Ok((location, Ok(renderer))) => {
                            debug!("Discovered renderer: {}", renderer.friendly_name);

                            // Send to progressive channel if provided
                            if let Some(ref tx) = progressive_tx {
                                let _ = tx.send(renderer.clone()).await;
                            }

                            renderers.insert(location, renderer);
                        }
                        Ok((location, Err(e))) => {
                            let kind: &'static str = match &e {
                                Error::Timeout => "timeout",
                                Error::InvalidResponse(_) => "invalid_response",
                                Error::Xml(_) => "xml",
                                Error::Http(_) => "http",
                                Error::Network(_) => "network",
                                Error::Ssdp(_) => "ssdp",
                                Error::Upnp(_) => "upnp",
                                Error::DeviceNotFound(_) => "device_not_found",
                                Error::ServiceNotFound(_) => "service_not_found",
                                Error::Io(_) => "io",
                                Error::FileNotFound(_) => "file_not_found",
                                Error::InvalidRange(_) => "invalid_range",
                                Error::Config(_) => "config",
                                Error::Unsupported(_) => "unsupported",
                            };
                            *failure_kinds.entry(kind).or_insert(0) += 1;

                            if fetch_failures_logged < 10 {
                                fetch_failures_logged += 1;
                                warn!("SSDP discovery: drop LOCATION={} ({})", location, e);

                                // Provide helpful diagnostics for common issues
                                if matches!(e, Error::Network(_) | Error::Io(_)) {
                                    let err_str = e.to_string();
                                    if err_str.contains("No route to host") || err_str.contains("os error 65") {
                                        warn!("SSDP discovery: Network routing issue detected. Possible causes:");
                                        warn!("SSDP discovery:   - VPN or multiple network interfaces active");
                                        warn!("SSDP discovery:   - Device on different subnet");
                                        warn!("SSDP discovery:   - macOS firewall or network settings");
                                        warn!("SSDP discovery: Try: Disable VPN, check network settings, or set FLINGDLNA_LOCAL_IPV4");
                                    }
                                }
                            } else {
                                trace!("SSDP discovery: drop LOCATION={} ({})", location, e);
                            }
                        }
                        Err(e) => {
                            trace!("Fetch task join error: {}", e);
                        }
                    }
                }
            }
        }

        info!(
            "SSDP discovery: {} unique LOCATION(s), {} renderer(s) in {:?}",
            seen_locations.len(),
            renderers.len(),
            started_at.elapsed()
        );
        if !failure_kinds.is_empty() {
            let mut kinds: Vec<(&'static str, usize)> = failure_kinds.into_iter().collect();
            kinds.sort_by(|a, b| b.1.cmp(&a.1));
            info!("SSDP discovery: dropped LOCATION(s) by reason={:?}", kinds);
        }
        info!(
            "SSDP discovery: recv_packets={}, recv_no_location={}",
            recv_packets.load(Ordering::Relaxed),
            recv_no_location.load(Ordering::Relaxed),
        );

        let result: Vec<Renderer> = renderers.into_values().collect();
        debug!(
            "Discovery complete: found {} renderer(s) in {:?}",
            result.len(),
            started_at.elapsed()
        );
        Ok(result)
    }
}

fn msearch_attempts(timeout: Duration) -> usize {
    if timeout >= Duration::from_secs(6) {
        3
    } else {
        2
    }
}

fn build_msearch_request(st: &str, mx: u64) -> String {
    format!(
        "M-SEARCH * HTTP/1.1\r\n\
         HOST: {SSDP_MULTICAST_ADDR}:{SSDP_PORT}\r\n\
         MAN: \"ssdp:discover\"\r\n\
         MX: {mx}\r\n\
         ST: {st}\r\n\
         \r\n"
    )
}

async fn send_msearch_attempts(
    sockets: &[(Ipv4Addr, tokio::net::UdpSocket)],
    broadcasts: &[(Ipv4Addr, Ipv4Addr)],
    multicast_addr: SocketAddr,
    timeout: Duration,
) -> bool {
    let mx = timeout.as_secs().max(1);
    let attempts = msearch_attempts(timeout);
    let mut any_send_ok = false;

    for attempt in 0..attempts {
        for st in MSEARCH_TARGETS {
            let request = build_msearch_request(st, mx);
            let (ok, total, sent_ok) =
                send_msearch_request(sockets, broadcasts, multicast_addr, &request, st).await;
            any_send_ok |= sent_ok;
            info!(
                "SSDP discovery: attempt {}/{} st='{}' ok={}/{}",
                attempt + 1,
                attempts,
                st,
                ok,
                total
            );
        }

        if attempt + 1 < attempts {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    any_send_ok
}

async fn send_msearch_request(
    sockets: &[(Ipv4Addr, tokio::net::UdpSocket)],
    broadcasts: &[(Ipv4Addr, Ipv4Addr)],
    multicast_addr: SocketAddr,
    request: &str,
    st: &str,
) -> (usize, usize, bool) {
    let payload = request.as_bytes();
    let mut ok = 0usize;
    let mut total = 0usize;
    let mut any_send_ok = false;

    for (iface_ip, socket) in sockets {
        total += 1;
        match socket.send_to(payload, multicast_addr).await {
            Ok(_) => {
                any_send_ok = true;
                ok += 1;
                trace!("Sent M-SEARCH({}) via {}", st, iface_ip);
            }
            Err(e) => {
                if is_multicast_route_error(&e) {
                    match send_to_with_pktinfo(socket, payload, multicast_addr, *iface_ip) {
                        Ok(_) => {
                            any_send_ok = true;
                            ok += 1;
                            trace!("Sent M-SEARCH({}) via {} (IP_PKTINFO)", st, iface_ip);
                        }
                        Err(pkt_err) => {
                            debug!(
                                "Failed to send M-SEARCH({}) via {}: {} (IP_PKTINFO: {})",
                                st, iface_ip, e, pkt_err
                            );
                        }
                    }
                } else {
                    debug!("Failed to send M-SEARCH({}) via {}: {}", st, iface_ip, e);
                }
            }
        }

        if let Some(bcast) = broadcast_for_iface(broadcasts, *iface_ip) {
            let bcast_addr = SocketAddr::new(IpAddr::V4(bcast), SSDP_PORT);
            match socket.send_to(payload, bcast_addr).await {
                Ok(_) => {
                    any_send_ok = true;
                    trace!("Sent SSDP broadcast({}) via {}", st, iface_ip);
                }
                Err(e) => {
                    debug!(
                        "Failed to send SSDP broadcast({}) via {}: {}",
                        st, iface_ip, e
                    );
                }
            }
        }
    }

    (ok, total, any_send_ok)
}

fn broadcast_for_iface(
    broadcasts: &[(Ipv4Addr, Ipv4Addr)],
    iface_ip: Ipv4Addr,
) -> Option<Ipv4Addr> {
    broadcasts
        .iter()
        .find_map(|(ip, bcast)| (*ip == iface_ip).then_some(*bcast))
}

fn spawn_ssdp_receivers(
    sockets: Vec<(Ipv4Addr, tokio::net::UdpSocket)>,
    ssdp_deadline: tokio::time::Instant,
) -> (
    mpsc::Receiver<SsdpResponse>,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
) {
    let recv_packets = Arc::new(AtomicUsize::new(0));
    let recv_no_location = Arc::new(AtomicUsize::new(0));
    // Use bounded channel to prevent memory exhaustion from discovery floods
    let (tx, rx) = mpsc::channel::<SsdpResponse>(1000);

    for (iface_ip, socket) in sockets {
        let tx = tx.clone();
        let recv_packets = recv_packets.clone();
        let recv_no_location = recv_no_location.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 2048];
            loop {
                let remaining =
                    ssdp_deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    break;
                }
                match tokio::time::timeout(remaining, socket.recv_from(&mut buf)).await {
                    Ok(Ok((len, _addr))) => {
                        recv_packets.fetch_add(1, Ordering::Relaxed);
                        let response = String::from_utf8_lossy(&buf[..len]);
                        if let Some(location) = parse_location(&response) {
                            let st = parse_search_target(&response);
                            let usn = parse_header_value(&response, "USN");
                            let _ = tx
                                .send(SsdpResponse {
                                    location,
                                    iface_ip,
                                    st,
                                    usn,
                                })
                                .await;
                        } else {
                            recv_no_location.fetch_add(1, Ordering::Relaxed);
                            trace!("SSDP response via {} without LOCATION", iface_ip);
                        }
                    }
                    Ok(Err(e)) => {
                        trace!("SSDP recv error via {}: {}", iface_ip, e);
                    }
                    Err(_) => {}
                }
            }
        });
    }
    drop(tx);

    (rx, recv_packets, recv_no_location)
}
