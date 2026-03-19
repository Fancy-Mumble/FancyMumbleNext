//! Shared utility functions.

/// Decode a hex string into raw bytes (best-effort, ignores invalid chars).
pub fn hex_to_bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks(2)
        .filter_map(|pair| {
            if pair.len() == 2 {
                let hi = hex_nibble(pair[0])?;
                let lo = hex_nibble(pair[1])?;
                Some((hi << 4) | lo)
            } else {
                None
            }
        })
        .collect()
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_to_bytes_basic() {
        assert_eq!(hex_to_bytes("deadbeef"), vec![0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn hex_to_bytes_uppercase() {
        assert_eq!(hex_to_bytes("AABB"), vec![0xaa, 0xbb]);
    }

    #[test]
    fn hex_to_bytes_empty() {
        assert_eq!(hex_to_bytes(""), Vec::<u8>::new());
    }

    #[test]
    fn hex_to_bytes_odd_length() {
        // Trailing nibble is ignored
        assert_eq!(hex_to_bytes("abc"), vec![0xab]);
    }
}
