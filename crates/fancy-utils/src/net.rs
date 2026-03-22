//! Network address formatting and parsing utilities.

use std::net::IpAddr;

/// Format raw IP address bytes into a human-readable string.
///
/// Supports 4-byte IPv4, 16-byte IPv6, and IPv4-mapped IPv6
/// (`::ffff:x.x.x.x`) which is automatically displayed as IPv4.
/// Unknown lengths fall back to a hex dump.
pub fn format_ip_address(bytes: &[u8]) -> String {
    match bytes.len() {
        4 => format!("{}.{}.{}.{}", bytes[0], bytes[1], bytes[2], bytes[3]),
        16 => {
            // IPv4-mapped IPv6 (::ffff:x.x.x.x)
            if bytes[..10].iter().all(|&b| b == 0)
                && bytes[10] == 0xff
                && bytes[11] == 0xff
            {
                return format!(
                    "{}.{}.{}.{}",
                    bytes[12], bytes[13], bytes[14], bytes[15]
                );
            }
            // Full IPv6
            let segments: Vec<String> = bytes
                .chunks(2)
                .map(|c| format!("{:x}", u16::from_be_bytes([c[0], c[1]])))
                .collect();
            segments.join(":")
        }
        _ => bytes.iter().map(|b| format!("{b:02x}")).collect::<String>(),
    }
}

/// Parse an IP address string into raw bytes suitable for Mumble's
/// protobuf wire format.
///
/// IPv4 addresses are stored as IPv4-mapped IPv6 (16 bytes:
/// `::ffff:x.x.x.x`).  IPv6 addresses are stored as 16 raw bytes.
pub fn parse_ip_to_bytes(addr: &str) -> Result<Vec<u8>, String> {
    let ip: IpAddr = addr.parse().map_err(|e| format!("Invalid IP address: {e}"))?;
    match ip {
        IpAddr::V4(v4) => {
            // Mumble stores IPv4 as IPv4-mapped IPv6 (16 bytes).
            let mut bytes = vec![0u8; 12];
            bytes[10] = 0xff;
            bytes[11] = 0xff;
            bytes.extend_from_slice(&v4.octets());
            Ok(bytes)
        }
        IpAddr::V6(v6) => Ok(v6.octets().to_vec()),
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "unwrap/expect acceptable in test code"
)]
mod tests {
    use super::*;

    #[test]
    fn format_ipv4() {
        assert_eq!(format_ip_address(&[192, 168, 1, 1]), "192.168.1.1");
    }

    #[test]
    fn format_ipv4_mapped_ipv6() {
        let mut addr = [0u8; 16];
        addr[10] = 0xff;
        addr[11] = 0xff;
        addr[12] = 10;
        addr[13] = 0;
        addr[14] = 0;
        addr[15] = 1;
        assert_eq!(format_ip_address(&addr), "10.0.0.1");
    }

    #[test]
    fn format_ipv6() {
        let addr: [u8; 16] = [
            0x20, 0x01, 0x0d, 0xb8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x01,
        ];
        assert_eq!(format_ip_address(&addr), "2001:db8:0:0:0:0:0:1");
    }

    #[test]
    fn format_unknown_length_hex() {
        assert_eq!(format_ip_address(&[0xab, 0xcd]), "abcd");
    }

    #[test]
    fn parse_ipv4_to_mapped_v6() {
        let bytes = parse_ip_to_bytes("10.0.0.1").expect("valid IPv4");
        assert_eq!(bytes.len(), 16);
        assert!(bytes[..10].iter().all(|&b| b == 0));
        assert_eq!(bytes[10], 0xff);
        assert_eq!(bytes[11], 0xff);
        assert_eq!(&bytes[12..], &[10, 0, 0, 1]);
    }

    #[test]
    fn parse_ipv6() {
        let bytes = parse_ip_to_bytes("2001:db8::1").expect("valid IPv6");
        assert_eq!(bytes.len(), 16);
        assert_eq!(bytes[0], 0x20);
        assert_eq!(bytes[1], 0x01);
        assert_eq!(bytes[15], 0x01);
    }

    #[test]
    fn parse_invalid_ip() {
        assert!(parse_ip_to_bytes("not-an-ip").is_err());
    }

    #[test]
    fn roundtrip_ipv4() {
        let formatted = format_ip_address(&parse_ip_to_bytes("192.168.1.1").expect("valid IPv4"));
        assert_eq!(formatted, "192.168.1.1");
    }

    #[test]
    fn roundtrip_ipv6() {
        let bytes = parse_ip_to_bytes("2001:db8::1").expect("valid IPv6");
        let formatted = format_ip_address(&bytes);
        assert_eq!(formatted, "2001:db8:0:0:0:0:0:1");
    }
}
