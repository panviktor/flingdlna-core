use super::prelude::{Error, IpAddr, Ipv4Addr, Result, SocketAddr};
use super::{SSDP_MULTICAST_ADDR, SSDP_PORT};
use crate::net::{
    ifindex_for_ipv4, local_ipv4_candidates, log_network_diagnostics, log_route_diagnostics,
};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::net::UdpSocket;
use tracing::{debug, error, trace, warn};

fn ssdp_diag_enabled() -> bool {
    matches!(
        std::env::var("FLINGDLNA_NET_DIAG").ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

/// Create SSDP listener socket
pub(super) async fn create_ssdp_listener(local_ip: IpAddr) -> Option<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};

    let socket = match Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)) {
        Ok(s) => s,
        Err(e) => {
            warn!("SSDP listener: failed to create socket: {}", e);
            return None;
        }
    };
    if let Err(e) = socket.set_reuse_address(true) {
        warn!("SSDP listener: failed to set SO_REUSEADDR: {}", e);
        return None;
    }

    #[cfg(unix)]
    if let Err(e) = socket.set_reuse_port(true) {
        warn!("SSDP listener: failed to set SO_REUSEPORT: {}", e);
        return None;
    }

    if let Err(e) = socket.set_nonblocking(true) {
        warn!("SSDP listener: failed to set nonblocking: {}", e);
        return None;
    }

    let addr: SocketAddr = format!("0.0.0.0:{SSDP_PORT}").parse().ok()?;
    if let Err(e) = socket.bind(&addr.into()) {
        warn!(
            "SSDP listener: failed to bind {} (is another UPnP service using 1900?): {}",
            addr, e
        );
        return None;
    }

    let mut join_ifaces: Vec<Ipv4Addr> = Vec::new();
    if let IpAddr::V4(v4) = local_ip {
        if !v4.is_unspecified() {
            join_ifaces.push(v4);
        }
    }
    join_ifaces.extend(local_ipv4_candidates());
    join_ifaces.push(Ipv4Addr::UNSPECIFIED);

    join_ifaces.sort();
    join_ifaces.dedup();

    // Join on all likely LAN interfaces plus INADDR_ANY.
    let mut joined = false;
    for iface in join_ifaces {
        if socket
            .join_multicast_v4(&SSDP_MULTICAST_ADDR, &iface)
            .is_ok()
        {
            joined = true;
        }
    }
    if !joined {
        warn!("SSDP listener: failed to join multicast group on any interface");
        return None;
    }

    match UdpSocket::from_std(socket.into()) {
        Ok(s) => Some(s),
        Err(e) => {
            warn!("SSDP listener: failed to convert socket: {}", e);
            None
        }
    }
}

pub(super) async fn create_sender_socket(local_ip: IpAddr) -> Result<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};

    let diag = ssdp_diag_enabled();

    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_nonblocking(true)?;
    // Allow broadcast fallback (some devices respond to SSDP broadcast).
    let _ = socket.set_broadcast(true);

    #[cfg(unix)]
    socket.set_reuse_address(true)?;

    let bind_ip = match local_ip {
        IpAddr::V4(v4) if !v4.is_unspecified() => v4,
        _ => Ipv4Addr::UNSPECIFIED,
    };

    let ifindex = if !bind_ip.is_unspecified() {
        ifindex_for_ipv4(bind_ip)
    } else {
        None
    };

    if let Some(ifindex) = ifindex {
        if let Err(e) = set_ip_bound_if(&socket, ifindex) {
            if diag {
                debug!(
                    "SSDP sender set_ip_bound_if({}, idx={}) failed: {}",
                    bind_ip, ifindex, e
                );
            }
        } else if diag {
            debug!(
                "SSDP sender set_ip_bound_if({}, idx={}) ok",
                bind_ip, ifindex
            );
        }
    }

    // Bind to INADDR_ANY; interface binding is handled via IP_BOUND_IF.
    socket.bind(&SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0).into())?;
    if diag {
        if let Ok(addr) = socket.local_addr() {
            debug!("SSDP sender socket bound to {:?}", addr);
        }
    }

    if !bind_ip.is_unspecified() {
        let result = socket.set_multicast_if_v4(&bind_ip);
        if diag {
            debug!(
                "SSDP sender set_multicast_if_v4({}) => {:?}",
                bind_ip, result
            );
        }
    }
    let _ = socket.set_multicast_ttl_v4(2);

    Ok(UdpSocket::from_std(socket.into())?)
}

pub(super) fn is_multicast_route_error(err: &io::Error) -> bool {
    match err.kind() {
        io::ErrorKind::NetworkUnreachable | io::ErrorKind::HostUnreachable => true,
        _ => matches!(
            err.raw_os_error(),
            // macOS: EHOSTUNREACH(65), ENETUNREACH(51)
            Some(65) | Some(51)
            // Linux: EHOSTUNREACH(113), ENETUNREACH(101)
            | Some(113) | Some(101)
        ),
    }
}

/// Track if we've already warned about multicast fallback (avoid log spam)
static MULTICAST_FALLBACK_WARNED: AtomicBool = AtomicBool::new(false);

pub(super) async fn send_multicast_with_fallback(
    socket: &UdpSocket,
    data: &[u8],
    dst: SocketAddr,
    local_ip: IpAddr,
    label: &'static str,
) -> Result<()> {
    let diag = ssdp_diag_enabled();

    match socket.send_to(data, dst).await {
        Ok(_) => Ok(()),
        Err(e) if is_multicast_route_error(&e) => {
            log_network_diagnostics("ssdp_multicast_route_error");
            log_route_diagnostics(Some(IpAddr::V4(SSDP_MULTICAST_ADDR)));
            let mut last_err: Option<io::Error> = None;
            for (iface_ip, sock) in create_multicast_sender_sockets(local_ip_v4(local_ip)).await? {
                match sock.send_to(data, dst).await {
                    Ok(_) => {
                        trace!("{}: fallback send succeeded via {}", label, iface_ip);
                        return Ok(());
                    }
                    Err(send_err) => {
                        if is_multicast_route_error(&send_err) {
                            match send_to_with_pktinfo(&sock, data, dst, iface_ip) {
                                Ok(_) => {
                                    trace!(
                                        "{}: fallback send succeeded via {} (IP_PKTINFO)",
                                        label,
                                        iface_ip
                                    );
                                    return Ok(());
                                }
                                Err(pkt_err) => {
                                    if diag {
                                        debug!(
                                            "{}: fallback send via {} failed: {} (IP_PKTINFO: {})",
                                            label, iface_ip, send_err, pkt_err
                                        );
                                    }
                                }
                            }
                        }
                        last_err = Some(send_err);
                    }
                }
            }
            if let Some(err) = last_err {
                if !MULTICAST_FALLBACK_WARNED.swap(true, Ordering::Relaxed) {
                    warn!(
                        "Multicast send failed ({}), fallback also failed ({}). \
                         (set FLINGDLNA_LOCAL_IPV4 if you have multiple NICs/VPN)",
                        e, err
                    );
                }
                error!("{}: fallback send failed on all interfaces: {}", label, err);
                Err(err.into())
            } else {
                if !MULTICAST_FALLBACK_WARNED.swap(true, Ordering::Relaxed) {
                    warn!(
                        "Multicast send failed ({}), fallback also failed. \
                         (set FLINGDLNA_LOCAL_IPV4 if you have multiple NICs/VPN)",
                        e
                    );
                }
                Err(Error::Network(format!(
                    "{label}: fallback send failed on all interfaces"
                )))
            }
        }
        Err(e) => Err(e.into()),
    }
}

fn local_ip_v4(ip: IpAddr) -> Option<Ipv4Addr> {
    match ip {
        IpAddr::V4(v4) if !v4.is_unspecified() => Some(v4),
        _ => None,
    }
}

pub(super) async fn create_multicast_sender_sockets(
    prefer: Option<Ipv4Addr>,
) -> Result<Vec<(Ipv4Addr, UdpSocket)>> {
    let mut candidates = local_ipv4_candidates();
    let diag = ssdp_diag_enabled();

    if let Some(p) = prefer {
        if !candidates.contains(&p) {
            candidates.insert(0, p);
        }
    }

    if candidates.is_empty() {
        candidates.push(Ipv4Addr::UNSPECIFIED);
    }

    let mut out = Vec::new();
    for ip in candidates {
        match create_sender_socket(IpAddr::V4(ip)).await {
            Ok(s) => {
                if diag {
                    debug!("SSDP sender socket created for {}", ip);
                }
                out.push((ip, s));
            }
            Err(e) => {
                debug!("Failed to create SSDP sender socket for {}: {}", ip, e);
            }
        }
    }
    Ok(out)
}

#[cfg(target_os = "macos")]
pub(super) fn send_to_with_pktinfo(
    socket: &UdpSocket,
    data: &[u8],
    dst: SocketAddr,
    iface_ip: Ipv4Addr,
) -> io::Result<usize> {
    use std::mem::{size_of, zeroed};
    use std::os::fd::AsRawFd;

    let dst_ip = match dst.ip() {
        IpAddr::V4(ip) => ip,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "IP_PKTINFO only supports IPv4",
            ))
        }
    };

    let ifindex = ifindex_for_ipv4(iface_ip).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "No ifindex for interface IP")
    })?;

    let mut dst_addr: libc::sockaddr_in = unsafe { zeroed() };
    dst_addr.sin_len = size_of::<libc::sockaddr_in>() as u8;
    dst_addr.sin_family = libc::AF_INET as libc::sa_family_t;
    dst_addr.sin_port = dst.port().to_be();
    dst_addr.sin_addr = libc::in_addr {
        s_addr: u32::from_be_bytes(dst_ip.octets()),
    };

    let mut iov = libc::iovec {
        iov_base: data.as_ptr() as *mut libc::c_void,
        iov_len: data.len(),
    };

    let cmsg_len = unsafe { libc::CMSG_LEN(size_of::<libc::in_pktinfo>() as u32) } as usize;
    let cmsg_space = unsafe { libc::CMSG_SPACE(size_of::<libc::in_pktinfo>() as u32) } as usize;
    let mut cmsg_buf = vec![0u8; cmsg_space];

    let mut msg: libc::msghdr = unsafe { zeroed() };
    msg.msg_name = &mut dst_addr as *mut _ as *mut libc::c_void;
    msg.msg_namelen = size_of::<libc::sockaddr_in>() as libc::socklen_t;
    msg.msg_iov = &mut iov as *mut libc::iovec;
    msg.msg_iovlen = 1;
    msg.msg_control = cmsg_buf.as_mut_ptr() as *mut libc::c_void;
    msg.msg_controllen = cmsg_buf.len() as libc::socklen_t;

    let cmsg = unsafe { libc::CMSG_FIRSTHDR(&msg) };
    if cmsg.is_null() {
        return Err(io::Error::other("CMSG_FIRSTHDR failed"));
    }

    unsafe {
        (*cmsg).cmsg_level = libc::IPPROTO_IP;
        (*cmsg).cmsg_type = libc::IP_PKTINFO;
        (*cmsg).cmsg_len = cmsg_len as libc::socklen_t;

        let pktinfo = libc::CMSG_DATA(cmsg) as *mut libc::in_pktinfo;
        (*pktinfo).ipi_ifindex = ifindex;
        (*pktinfo).ipi_spec_dst = libc::in_addr {
            s_addr: u32::from_be_bytes(iface_ip.octets()),
        };
        (*pktinfo).ipi_addr = libc::in_addr { s_addr: 0 };
    }

    let ret = unsafe { libc::sendmsg(socket.as_raw_fd(), &msg, 0) };
    if ret < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(ret as usize)
}

#[cfg(not(target_os = "macos"))]
pub(super) fn send_to_with_pktinfo(
    _socket: &UdpSocket,
    _data: &[u8],
    _dst: SocketAddr,
    _iface_ip: Ipv4Addr,
) -> io::Result<usize> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "IP_PKTINFO send not supported",
    ))
}

#[cfg(target_os = "macos")]
fn set_ip_bound_if(socket: &socket2::Socket, ifindex: u32) -> std::io::Result<()> {
    use std::mem::size_of;
    use std::os::fd::AsRawFd;

    const IP_BOUND_IF: libc::c_int = 25;
    let fd = socket.as_raw_fd();
    let idx = ifindex as libc::c_uint;
    let ret = unsafe {
        libc::setsockopt(
            fd,
            libc::IPPROTO_IP,
            IP_BOUND_IF,
            &idx as *const _ as *const libc::c_void,
            size_of::<libc::c_uint>() as libc::socklen_t,
        )
    };
    if ret == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(target_os = "macos"))]
fn set_ip_bound_if(_socket: &socket2::Socket, _ifindex: u32) -> std::io::Result<()> {
    Ok(())
}
