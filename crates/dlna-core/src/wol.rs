//! Wake-on-LAN implementation
//!
//! Sends magic packets to wake up devices on the network.

use crate::{Error, Result};
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use tracing::{debug, info};

/// Send a Wake-on-LAN magic packet to wake up a device
///
/// # Arguments
/// * `mac_address` - MAC address in format "AA:BB:CC:DD:EE:FF" or "AA-BB-CC-DD-EE-FF"
/// * `broadcast_addr` - Optional broadcast address (default: 255.255.255.255)
///
/// # Example
/// ```ignore
/// wol::wake("AA:BB:CC:DD:EE:FF", None)?;
/// ```
pub fn wake(mac_address: &str, broadcast_addr: Option<Ipv4Addr>) -> Result<()> {
    let mac_bytes = parse_mac_address(mac_address)?;
    let magic_packet = build_magic_packet(&mac_bytes);

    let broadcast = broadcast_addr.unwrap_or(Ipv4Addr::new(255, 255, 255, 255));
    let dest = SocketAddr::from((broadcast, 9)); // WOL uses port 9

    let socket = UdpSocket::bind("0.0.0.0:0")
        .map_err(|e| Error::Network(format!("Failed to bind UDP socket: {e}")))?;

    socket
        .set_broadcast(true)
        .map_err(|e| Error::Network(format!("Failed to enable broadcast: {e}")))?;

    socket
        .send_to(&magic_packet, dest)
        .map_err(|e| Error::Network(format!("Failed to send magic packet: {e}")))?;

    info!("Sent Wake-on-LAN packet to {}", mac_address);
    debug!("Broadcast to {} on port 9", broadcast);

    Ok(())
}

/// Parse MAC address string into bytes
fn parse_mac_address(mac: &str) -> Result<[u8; 6]> {
    let cleaned: String = mac.chars().filter(|c| c.is_ascii_hexdigit()).collect();

    if cleaned.len() != 12 {
        return Err(Error::Config(format!(
            "Invalid MAC address '{}': expected 12 hex digits, got {}",
            mac,
            cleaned.len()
        )));
    }

    let mut bytes = [0u8; 6];
    for i in 0..6 {
        bytes[i] = u8::from_str_radix(&cleaned[i * 2..i * 2 + 2], 16)
            .map_err(|_| Error::Config(format!("Invalid MAC address '{mac}'")))?;
    }

    Ok(bytes)
}

/// Build the WOL magic packet
///
/// Format: 6 bytes of 0xFF followed by 16 repetitions of the MAC address
fn build_magic_packet(mac: &[u8; 6]) -> [u8; 102] {
    let mut packet = [0u8; 102];

    // First 6 bytes: 0xFF
    for byte in &mut packet[0..6] {
        *byte = 0xFF;
    }

    // Repeat MAC address 16 times
    for i in 0..16 {
        let offset = 6 + i * 6;
        packet[offset..offset + 6].copy_from_slice(mac);
    }

    packet
}

/// Get MAC address from ARP table for a given IP
///
/// Works on macOS and Linux by parsing `arp -n` output
pub fn get_mac_from_arp(ip: &str) -> Option<String> {
    use crate::process::run_command_capture_stdout;
    use std::time::Duration;

    #[cfg(target_os = "macos")]
    let output = run_command_capture_stdout("arp", &["-n", ip], Duration::from_secs(1), 16 * 1024)
        .ok()??;

    #[cfg(target_os = "linux")]
    let output = run_command_capture_stdout("arp", &["-n", ip], Duration::from_secs(1), 16 * 1024)
        .ok()??;

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    return None;

    if !output.success {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_arp_output(&stdout, ip)
}

/// Parse ARP command output to extract MAC address
fn parse_arp_output(output: &str, ip: &str) -> Option<String> {
    for line in output.lines() {
        if !line.contains(ip) {
            continue;
        }

        // Look for MAC pattern: XX:XX:XX:XX:XX:XX or XX-XX-XX-XX-XX-XX
        for word in line.split_whitespace() {
            let cleaned: String = word
                .chars()
                .filter(|c| c.is_ascii_hexdigit() || *c == ':' || *c == '-')
                .collect();

            // Check if it looks like a MAC address
            let hex_count = cleaned.chars().filter(|c| c.is_ascii_hexdigit()).count();
            let sep_count = cleaned.chars().filter(|c| *c == ':' || *c == '-').count();

            if hex_count == 12 && sep_count == 5 {
                // Normalize to AA:BB:CC:DD:EE:FF format
                let normalized: String = cleaned
                    .chars()
                    .filter(|c| c.is_ascii_hexdigit())
                    .collect::<String>()
                    .to_uppercase();

                if normalized.len() == 12 {
                    return Some(format!(
                        "{}:{}:{}:{}:{}:{}",
                        &normalized[0..2],
                        &normalized[2..4],
                        &normalized[4..6],
                        &normalized[6..8],
                        &normalized[8..10],
                        &normalized[10..12]
                    ));
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mac_colon() {
        let mac = parse_mac_address("AA:BB:CC:DD:EE:FF").unwrap();
        assert_eq!(mac, [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
    }

    #[test]
    fn test_parse_mac_dash() {
        let mac = parse_mac_address("AA-BB-CC-DD-EE-FF").unwrap();
        assert_eq!(mac, [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
    }

    #[test]
    fn test_parse_mac_no_separator() {
        let mac = parse_mac_address("AABBCCDDEEFF").unwrap();
        assert_eq!(mac, [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
    }

    #[test]
    fn test_parse_mac_lowercase() {
        let mac = parse_mac_address("aa:bb:cc:dd:ee:ff").unwrap();
        assert_eq!(mac, [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
    }

    #[test]
    fn test_parse_mac_invalid() {
        assert!(parse_mac_address("invalid").is_err());
        assert!(parse_mac_address("AA:BB:CC").is_err());
        assert!(parse_mac_address("").is_err());
    }

    #[test]
    fn test_build_magic_packet() {
        let mac = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        let packet = build_magic_packet(&mac);

        // Check header (6 bytes of 0xFF)
        assert_eq!(&packet[0..6], &[0xFF; 6]);

        // Check first MAC repetition
        assert_eq!(&packet[6..12], &mac);

        // Check last MAC repetition
        assert_eq!(&packet[96..102], &mac);

        // Total length
        assert_eq!(packet.len(), 102);
    }

    #[test]
    fn test_parse_arp_output_macos() {
        let output = "? (192.168.1.100) at aa:bb:cc:dd:ee:ff on en0 ifscope [ethernet]";
        let mac = parse_arp_output(output, "192.168.1.100");
        assert_eq!(mac, Some("AA:BB:CC:DD:EE:FF".to_string()));
    }

    #[test]
    fn test_parse_arp_output_linux() {
        let output = "192.168.1.100    ether   aa:bb:cc:dd:ee:ff   C                     eth0";
        let mac = parse_arp_output(output, "192.168.1.100");
        assert_eq!(mac, Some("AA:BB:CC:DD:EE:FF".to_string()));
    }
}
