//! Wake-on-LAN support for waking up sleeping devices
//!
//! Sends magic packets to wake devices before attempting to connect.

use std::net::{IpAddr, UdpSocket};
use std::process::Command;
use tracing::{debug, info, warn};

/// MAC address (6 bytes)
#[derive(Debug, Clone)]
pub struct MacAddress([u8; 6]);

impl MacAddress {
    /// Parse MAC address from string (formats: "AA:BB:CC:DD:EE:FF" or "AA-BB-CC-DD-EE-FF")
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let parts: Vec<&str> = if s.contains(':') {
            s.split(':').collect()
        } else if s.contains('-') {
            s.split('-').collect()
        } else {
            return None;
        };

        if parts.len() != 6 {
            return None;
        }

        let mut bytes = [0u8; 6];
        for (i, part) in parts.iter().enumerate() {
            bytes[i] = u8::from_str_radix(part, 16).ok()?;
        }

        Some(MacAddress(bytes))
    }

    /// Get raw bytes
    pub fn bytes(&self) -> &[u8; 6] {
        &self.0
    }
}

/// Get MAC address for an IP from the system ARP table
pub fn get_mac_for_ip(ip: IpAddr) -> Option<MacAddress> {
    let ip_str = ip.to_string();

    // Try `arp` command (works on macOS and Linux)
    let output = Command::new("arp").arg("-n").arg(&ip_str).output().ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    debug!("ARP output for {}: {}", ip_str, stdout);

    // Parse MAC from output
    // macOS format: "192.0.2.1 (192.0.2.1) at aa:bb:cc:dd:ee:ff on en0 ..."
    // Linux format: "192.0.2.1 ether aa:bb:cc:dd:ee:ff C en0"
    for line in stdout.lines() {
        if line.contains(&ip_str) {
            // Find MAC address pattern (xx:xx:xx:xx:xx:xx)
            for word in line.split_whitespace() {
                if word.contains(':') && word.len() == 17 {
                    if let Some(mac) = MacAddress::parse(word) {
                        info!("Found MAC for {}: {}", ip_str, word);
                        return Some(mac);
                    }
                }
            }
        }
    }

    warn!("Could not find MAC address for {} in ARP table", ip_str);
    None
}

/// Send Wake-on-LAN magic packet to wake a device
pub fn send_wol(mac: &MacAddress) -> std::io::Result<()> {
    // Magic packet: 6 bytes of 0xFF followed by MAC address repeated 16 times
    let mut packet = vec![0xFFu8; 6];
    for _ in 0..16 {
        packet.extend_from_slice(mac.bytes());
    }

    // Send to broadcast address on port 9 (standard WoL port)
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_broadcast(true)?;

    // Send to multiple broadcast addresses for better compatibility
    let broadcasts = [
        "255.255.255.255:9",
        "255.255.255.255:7",
        "192.168.1.255:9", // Common subnet broadcast
    ];

    for addr in &broadcasts {
        match socket.send_to(&packet, addr) {
            Ok(_) => debug!("Sent WoL packet to {}", addr),
            Err(e) => debug!("Failed to send WoL to {}: {}", addr, e),
        }
    }

    info!(
        "Wake-on-LAN packet sent for MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        mac.bytes()[0],
        mac.bytes()[1],
        mac.bytes()[2],
        mac.bytes()[3],
        mac.bytes()[4],
        mac.bytes()[5]
    );

    Ok(())
}

/// Try to wake a device by IP address
/// Returns true if WoL packet was sent, false if MAC couldn't be found
pub fn wake_by_ip(ip: IpAddr) -> bool {
    if let Some(mac) = get_mac_for_ip(ip) {
        if let Err(e) = send_wol(&mac) {
            warn!("Failed to send WoL: {}", e);
            return false;
        }
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mac() {
        let mac = MacAddress::parse("aa:bb:cc:dd:ee:ff").unwrap();
        assert_eq!(mac.bytes(), &[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);

        let mac2 = MacAddress::parse("AA-BB-CC-DD-EE-FF").unwrap();
        assert_eq!(mac2.bytes(), &[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
    }
}
