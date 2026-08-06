use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicBool, Ordering};

use super::selection::{
    best_local_ipv4, local_ipv4_bind_candidates, local_ipv4_broadcasts, local_ipv4_candidates,
};

/// Log detailed network diagnostics (best effort, platform-specific).
pub fn log_network_diagnostics(context: &str) {
    if !net_diag_enabled() {
        return;
    }

    static LOGGED: AtomicBool = AtomicBool::new(false);
    let allow_repeat = std::env::var("FLINGDLNA_NET_DIAG_ALWAYS")
        .ok()
        .as_deref()
        .map(is_truthy)
        .unwrap_or(false);
    if !allow_repeat && LOGGED.swap(true, Ordering::Relaxed) {
        return;
    }

    tracing::info!("NET DIAG [{}] ===============================", context);
    tracing::info!(
        "NET DIAG: FLINGDLNA_LOCAL_IPV4={:?}",
        std::env::var("FLINGDLNA_LOCAL_IPV4").ok()
    );
    tracing::info!("NET DIAG: best_local_ipv4={:?}", best_local_ipv4().ok());
    tracing::info!(
        "NET DIAG: local_ipv4_candidates={:?}",
        local_ipv4_candidates()
    );
    tracing::info!(
        "NET DIAG: local_ipv4_broadcasts={:?}",
        local_ipv4_broadcasts()
    );
    tracing::info!(
        "NET DIAG: local_ipv4_bind_candidates={:?}",
        local_ipv4_bind_candidates()
    );

    #[cfg(unix)]
    {
        for entry in collect_interface_diagnostics() {
            tracing::info!(
                "NET DIAG: iface={} idx={} flags={} ipv4={:?} netmask={:?} bcast={:?} dst={:?}",
                entry.name,
                entry.ifindex,
                entry.flags,
                entry.ipv4,
                entry.netmask,
                entry.broadcast,
                entry.dstaddr
            );
        }
        log_udp_send_probes();
        log_route_diagnostics(None);
    }
}

fn net_diag_enabled() -> bool {
    std::env::var("FLINGDLNA_NET_DIAG")
        .ok()
        .as_deref()
        .map(is_truthy)
        .unwrap_or(false)
}

fn is_truthy(v: &str) -> bool {
    matches!(v, "1" | "true" | "TRUE" | "yes" | "YES")
}

fn cmd_diag_enabled() -> bool {
    std::env::var("FLINGDLNA_NET_DIAG_CMD")
        .ok()
        .as_deref()
        .map(is_truthy)
        .unwrap_or(false)
}

#[cfg(unix)]
#[derive(Default)]
struct InterfaceDiag {
    name: String,
    ifindex: u32,
    flags: String,
    ipv4: Option<Ipv4Addr>,
    netmask: Option<Ipv4Addr>,
    broadcast: Option<Ipv4Addr>,
    dstaddr: Option<Ipv4Addr>,
}

#[cfg(unix)]
fn collect_interface_diagnostics() -> Vec<InterfaceDiag> {
    use libc::{freeifaddrs, getifaddrs, ifaddrs, sockaddr, sockaddr_in, AF_INET};
    use std::ffi::CStr;

    let mut out = Vec::new();
    let mut ifap: *mut ifaddrs = std::ptr::null_mut();
    let rc = unsafe { getifaddrs(&mut ifap) };
    if rc != 0 || ifap.is_null() {
        return out;
    }

    let mut cur = ifap;
    while !cur.is_null() {
        let ifa = unsafe { &*cur };
        let addr = ifa.ifa_addr as *const sockaddr;

        let name = unsafe {
            if ifa.ifa_name.is_null() {
                ""
            } else {
                CStr::from_ptr(ifa.ifa_name).to_str().unwrap_or("")
            }
        };

        let mut diag = InterfaceDiag {
            name: name.to_string(),
            ifindex: unsafe {
                if ifa.ifa_name.is_null() {
                    0
                } else {
                    libc::if_nametoindex(ifa.ifa_name)
                }
            },
            flags: format_flags(ifa.ifa_flags as i32),
            ..Default::default()
        };

        if !addr.is_null() && unsafe { (*addr).sa_family as i32 } == AF_INET {
            let sin = unsafe { &*(ifa.ifa_addr as *const sockaddr_in) };
            diag.ipv4 = Some(Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr)));
        }

        let netmask = ifa.ifa_netmask as *const sockaddr;
        if !netmask.is_null() && unsafe { (*netmask).sa_family as i32 } == AF_INET {
            let sin = unsafe { &*(ifa.ifa_netmask as *const sockaddr_in) };
            diag.netmask = Some(Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr)));
        }

        let dst = ifa.ifa_dstaddr as *const sockaddr;
        if !dst.is_null() && unsafe { (*dst).sa_family as i32 } == AF_INET {
            let sin = unsafe { &*(ifa.ifa_dstaddr as *const sockaddr_in) };
            diag.dstaddr = Some(Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr)));
        }

        let flags = ifa.ifa_flags as i32;
        let has_broadcast = (flags & libc::IFF_BROADCAST) != 0;
        if has_broadcast {
            let baddr = ifa.ifa_dstaddr as *const sockaddr;
            if !baddr.is_null() && unsafe { (*baddr).sa_family as i32 } == AF_INET {
                let sin = unsafe { &*(baddr as *const sockaddr_in) };
                diag.broadcast = Some(Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr)));
            }
        }

        if diag.ipv4.is_some() || !diag.name.is_empty() {
            out.push(diag);
        }

        cur = ifa.ifa_next;
    }

    unsafe { freeifaddrs(ifap) };
    out
}

#[cfg(unix)]
fn format_flags(flags: i32) -> String {
    let mut out = Vec::new();
    if (flags & libc::IFF_UP) != 0 {
        out.push("UP");
    }
    if (flags & libc::IFF_RUNNING) != 0 {
        out.push("RUNNING");
    }
    if (flags & libc::IFF_LOOPBACK) != 0 {
        out.push("LOOPBACK");
    }
    if (flags & libc::IFF_POINTOPOINT) != 0 {
        out.push("P2P");
    }
    if (flags & libc::IFF_MULTICAST) != 0 {
        out.push("MULTICAST");
    }
    if (flags & libc::IFF_BROADCAST) != 0 {
        out.push("BROADCAST");
    }
    out.join("|")
}

/// Log routing table diagnostics using system commands (macOS only).
#[cfg(target_os = "macos")]
pub fn log_route_diagnostics(target: Option<std::net::IpAddr>) {
    if !cmd_diag_enabled() {
        return;
    }

    static CMD_LOGGED_BASE: AtomicBool = AtomicBool::new(false);
    static CMD_LOGGED_TARGET: AtomicBool = AtomicBool::new(false);
    let allow_repeat = std::env::var("FLINGDLNA_NET_DIAG_ALWAYS")
        .ok()
        .as_deref()
        .map(is_truthy)
        .unwrap_or(false);

    if target.is_some() {
        if !allow_repeat && CMD_LOGGED_TARGET.swap(true, Ordering::Relaxed) {
            return;
        }
    } else if !allow_repeat && CMD_LOGGED_BASE.swap(true, Ordering::Relaxed) {
        return;
    }

    tracing::info!("NET DIAG: route_cmds: enabled target={:?}", target);

    if let Some(ip) = target {
        if ip.is_ipv4() {
            log_cmd("/sbin/route", &["-n", "get", &ip.to_string()]);
        }
    }

    log_cmd("/sbin/route", &["-n", "get", "239.255.255.250"]);
    log_cmd("/usr/sbin/netstat", &["-rn", "-f", "inet"]);
    log_cmd("/usr/sbin/scutil", &["--nwi"]);
}

#[cfg(not(target_os = "macos"))]
pub fn log_route_diagnostics(_target: Option<std::net::IpAddr>) {}

#[cfg(target_os = "macos")]
fn log_cmd(cmd: &str, args: &[&str]) {
    use std::process::Command;

    let output = Command::new(cmd).args(args).output();
    match output {
        Ok(out) => {
            let status = out.status;
            let mut text = String::new();
            text.push_str(&String::from_utf8_lossy(&out.stdout));
            if !out.stderr.is_empty() {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str("[stderr]\n");
                text.push_str(&String::from_utf8_lossy(&out.stderr));
            }
            let trimmed = trim_output(&text, 4000, 80);
            tracing::info!(
                "NET DIAG: cmd='{} {:?}' status={} output:\n{}",
                cmd,
                args,
                status,
                trimmed
            );
        }
        Err(e) => {
            tracing::info!("NET DIAG: cmd='{} {:?}' failed: {}", cmd, args, e);
        }
    }
}

#[cfg(target_os = "macos")]
fn trim_output(input: &str, max_chars: usize, max_lines: usize) -> String {
    let mut out = String::new();
    let mut chars = 0usize;
    for (lines, line) in input.lines().enumerate() {
        if lines >= max_lines || chars >= max_chars {
            out.push_str("\n[output trimmed]");
            break;
        }
        let line_len = line.len();
        if chars + line_len > max_chars {
            let take = max_chars.saturating_sub(chars);
            out.push_str(&line[..take.min(line.len())]);
            out.push_str("\n[output trimmed]");
            break;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line);
        chars += line_len;
    }
    out
}

#[cfg(unix)]
fn log_udp_send_probes() {
    use socket2::{Domain, Protocol, Socket, Type};
    use std::net::{IpAddr, SocketAddr};

    let probes = local_ipv4_broadcasts();
    if probes.is_empty() {
        tracing::info!("NET DIAG: udp_probe: no broadcast-capable interfaces");
        return;
    }

    let payload = b"FLINGDLNA_DIAG";
    let multicast_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(239, 255, 255, 250)), 1900);

    for (ip, bcast) in probes {
        let socket = match Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)) {
            Ok(s) => s,
            Err(e) => {
                tracing::info!(
                    "NET DIAG: udp_probe: socket create failed for {}: {}",
                    ip,
                    e
                );
                continue;
            }
        };

        let _ = socket.set_broadcast(true);
        let bind_addr = SocketAddr::new(IpAddr::V4(ip), 0);
        if let Err(e) = socket.bind(&bind_addr.into()) {
            tracing::info!("NET DIAG: udp_probe: bind {} failed: {}", ip, e);
            continue;
        }

        let mc_if_res = socket.set_multicast_if_v4(&ip);
        tracing::info!(
            "NET DIAG: udp_probe: set_multicast_if_v4({}) => {:?}",
            ip,
            mc_if_res
        );

        let send_mc = socket.send_to(payload, &multicast_addr.into());
        tracing::info!(
            "NET DIAG: udp_probe: send multicast {} => {:?}",
            multicast_addr,
            send_mc
        );

        let bcast_addr = SocketAddr::new(IpAddr::V4(bcast), 1900);
        let send_bc = socket.send_to(payload, &bcast_addr.into());
        tracing::info!(
            "NET DIAG: udp_probe: send broadcast {} => {:?}",
            bcast_addr,
            send_bc
        );
    }
}
