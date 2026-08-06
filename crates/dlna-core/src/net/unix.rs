use std::net::Ipv4Addr;

#[cfg(unix)]
pub(super) fn best_local_ipv4_unix() -> Option<Ipv4Addr> {
    use libc::{freeifaddrs, getifaddrs, ifaddrs, sockaddr, sockaddr_in, AF_INET};
    use std::borrow::Cow;
    use std::ffi::CStr;

    let mut ifap: *mut ifaddrs = std::ptr::null_mut();
    let rc = unsafe { getifaddrs(&mut ifap) };
    if rc != 0 || ifap.is_null() {
        return None;
    }

    let mut preferred = Vec::new();
    let mut fallback = Vec::new();

    let mut cur = ifap;
    while !cur.is_null() {
        let ifa = unsafe { &*cur };
        let addr = ifa.ifa_addr as *const sockaddr;
        if !addr.is_null() && unsafe { (*addr).sa_family as i32 } == AF_INET {
            let flags = ifa.ifa_flags as i32;
            let is_up = (flags & libc::IFF_UP) != 0;
            let is_loopback = (flags & libc::IFF_LOOPBACK) != 0;
            let is_p2p = (flags & libc::IFF_POINTOPOINT) != 0;
            let has_multicast = (flags & libc::IFF_MULTICAST) != 0;

            if is_up && has_multicast && !is_loopback && !is_p2p {
                let sin = unsafe { &*(ifa.ifa_addr as *const sockaddr_in) };
                let ip = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));

                if ip.is_unspecified()
                    || ip.is_loopback()
                    || ip.is_multicast()
                    || ip.octets() == [255, 255, 255, 255]
                {
                    cur = ifa.ifa_next;
                    continue;
                }

                let name_cow: Cow<'_, str> = unsafe {
                    if ifa.ifa_name.is_null() {
                        Cow::Borrowed("")
                    } else {
                        CStr::from_ptr(ifa.ifa_name).to_string_lossy()
                    }
                };

                let score = score_interface(name_cow.as_ref(), ip);
                fallback.push((score, ip));

                if is_preferred_interface(name_cow.as_ref()) {
                    preferred.push((score, ip));
                }
            }
        }

        cur = ifa.ifa_next;
    }

    unsafe { freeifaddrs(ifap) };

    preferred.sort_by_key(|(score, _)| *score);
    fallback.sort_by_key(|(score, _)| *score);

    preferred.pop().or_else(|| fallback.pop()).map(|(_, ip)| ip)
}

#[cfg(unix)]
pub(super) fn local_ipv4_candidates_unix() -> Vec<Ipv4Addr> {
    let mut preferred = Vec::new();
    let mut fallback = Vec::new();

    for (score, ip, preferred_iface, _broadcast, _ifindex) in collect_ipv4_interfaces() {
        if preferred_iface {
            preferred.push((score, ip));
        } else {
            fallback.push((score, ip));
        }
    }

    preferred.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.octets().cmp(&b.1.octets())));
    fallback.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.octets().cmp(&b.1.octets())));

    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (_, ip) in preferred.into_iter().chain(fallback.into_iter()) {
        if seen.insert(ip) {
            out.push(ip);
        }
    }
    out
}

#[cfg(unix)]
pub(super) fn local_ipv4_broadcasts_unix() -> Vec<(Ipv4Addr, Ipv4Addr)> {
    let mut entries = collect_ipv4_interfaces();
    entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.octets().cmp(&b.1.octets())));

    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (_, ip, _preferred_iface, broadcast, _ifindex) in entries {
        if let Some(bcast) = broadcast {
            if seen.insert(ip) {
                out.push((ip, bcast));
            }
        }
    }
    out
}

#[cfg(unix)]
pub(super) fn local_ipv4_bind_candidates_unix() -> Vec<(Ipv4Addr, u32)> {
    let mut preferred = Vec::new();
    let mut fallback = Vec::new();

    for (score, ip, preferred_iface, _broadcast, ifindex) in collect_ipv4_interfaces() {
        let entry = (score, ip, ifindex);
        if preferred_iface {
            preferred.push(entry);
        } else {
            fallback.push(entry);
        }
    }

    preferred.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.octets().cmp(&b.1.octets())));
    fallback.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.octets().cmp(&b.1.octets())));

    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (_, ip, ifindex) in preferred.into_iter().chain(fallback.into_iter()) {
        if seen.insert(ip) {
            out.push((ip, ifindex));
        }
    }
    out
}

#[cfg(unix)]
fn collect_ipv4_interfaces() -> Vec<(i32, Ipv4Addr, bool, Option<Ipv4Addr>, u32)> {
    use libc::{freeifaddrs, getifaddrs, ifaddrs, sockaddr, sockaddr_in, AF_INET};
    use std::borrow::Cow;
    use std::ffi::CStr;

    let mut ifap: *mut ifaddrs = std::ptr::null_mut();
    let rc = unsafe { getifaddrs(&mut ifap) };
    if rc != 0 || ifap.is_null() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut cur = ifap;
    while !cur.is_null() {
        let ifa = unsafe { &*cur };
        let addr = ifa.ifa_addr as *const sockaddr;
        if !addr.is_null() && unsafe { (*addr).sa_family as i32 } == AF_INET {
            let flags = ifa.ifa_flags as i32;
            let is_up = (flags & libc::IFF_UP) != 0;
            let is_loopback = (flags & libc::IFF_LOOPBACK) != 0;
            let is_p2p = (flags & libc::IFF_POINTOPOINT) != 0;
            let has_multicast = (flags & libc::IFF_MULTICAST) != 0;

            if is_up && has_multicast && !is_loopback && !is_p2p {
                let sin = unsafe { &*(ifa.ifa_addr as *const sockaddr_in) };
                let ip = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));

                if ip.is_unspecified()
                    || ip.is_loopback()
                    || ip.is_multicast()
                    || ip.octets() == [255, 255, 255, 255]
                {
                    cur = ifa.ifa_next;
                    continue;
                }

                let name_cow: Cow<'_, str> = unsafe {
                    if ifa.ifa_name.is_null() {
                        Cow::Borrowed("")
                    } else {
                        CStr::from_ptr(ifa.ifa_name).to_string_lossy()
                    }
                };
                let name = name_cow.as_ref();
                let preferred_iface = is_preferred_interface(name);
                let score = score_interface(name, ip);

                let ifindex = unsafe {
                    if ifa.ifa_name.is_null() {
                        0
                    } else {
                        libc::if_nametoindex(ifa.ifa_name)
                    }
                };
                let broadcast = resolve_broadcast(ifa, ip);
                out.push((score, ip, preferred_iface, broadcast, ifindex));
            }
        }

        cur = ifa.ifa_next;
    }

    unsafe { freeifaddrs(ifap) };
    out
}

#[cfg(unix)]
fn resolve_broadcast(ifa: &libc::ifaddrs, ip: Ipv4Addr) -> Option<Ipv4Addr> {
    use libc::{sockaddr, sockaddr_in, AF_INET};

    let flags = ifa.ifa_flags as i32;
    let has_broadcast = (flags & libc::IFF_BROADCAST) != 0;

    let mut bcast = None;
    if has_broadcast {
        let baddr = ifa.ifa_dstaddr as *const sockaddr;
        if !baddr.is_null() && unsafe { (*baddr).sa_family as i32 } == AF_INET {
            let sin = unsafe { &*(baddr as *const sockaddr_in) };
            let b = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
            if !b.is_unspecified() && !b.is_multicast() {
                bcast = Some(b);
            }
        }
    }

    if bcast.is_none() {
        let netmask = ifa.ifa_netmask as *const sockaddr;
        if !netmask.is_null() && unsafe { (*netmask).sa_family as i32 } == AF_INET {
            let sin = unsafe { &*(netmask as *const sockaddr_in) };
            let mask = u32::from_be(sin.sin_addr.s_addr);
            if mask != 0 {
                let ip_u32 = u32::from(ip);
                let b = Ipv4Addr::from(ip_u32 | !mask);
                if !b.is_unspecified() && !b.is_multicast() {
                    bcast = Some(b);
                }
            }
        }
    }

    bcast
}

fn is_shared_cgnat(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    a == 100 && (64..=127).contains(&b)
}

fn is_preferred_interface(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    if n.starts_with("lo")
        || n.starts_with("utun")
        || n.starts_with("awdl")
        || n.starts_with("llw")
        || n.starts_with("p2p")
        || n.starts_with("bridge")
        || n.starts_with("vmnet")
        || n.starts_with("vboxnet")
        || n.starts_with("docker")
        || n.starts_with("br-")
        || n.starts_with("virbr")
        || n.starts_with("wg")
        || n.starts_with("tailscale")
        || n.starts_with("zt")
        || n.starts_with("tun")
        || n.starts_with("tap")
    {
        return false;
    }

    n.starts_with("en") || n.starts_with("eth") || n.starts_with("wlan") || n.starts_with("wl")
}

fn score_interface(name: &str, ip: Ipv4Addr) -> i32 {
    let n = name.to_ascii_lowercase();

    let kind_score = if ip.is_private() || is_shared_cgnat(ip) {
        3
    } else if ip.is_link_local() {
        2
    } else {
        1
    };

    let name_bonus = if n.starts_with("en") {
        if n.len() == 3 && n.chars().nth(2).is_some_and(|c| c.is_ascii_digit()) {
            110
        } else {
            100
        }
    } else if n.starts_with("eth") {
        95
    } else if n.starts_with("wlan") || n.starts_with("wl") {
        90
    } else {
        0
    };

    let ip_bonus = match ip.octets() {
        [192, 168, ..] => 5,
        [10, ..] => 4,
        [172, b, ..] if (16..=31).contains(&b) => 3,
        _ => 0,
    };

    (kind_score * 1000) + (name_bonus * 10) + ip_bonus
}
