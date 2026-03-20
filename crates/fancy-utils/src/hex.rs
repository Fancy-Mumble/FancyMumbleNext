//! Hex encoding and decoding utilities.

/// Decode a hex string into bytes (best-effort, ignores invalid chars).
///
/// Processes the input in pairs of characters. If a pair contains an
/// invalid hex character it is silently skipped; an odd trailing nibble
/// is also ignored.
///
/// Use [`hex_decode`] when you need strict validation instead.
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

/// Strictly decode a hex string into bytes.
///
/// Returns `None` if the input has an odd length or contains any
/// non-hex character.
pub fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

/// Encode a byte slice as a lowercase hex string.
pub fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
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

    // -- hex_to_bytes (best-effort) ---

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

    // -- hex_decode (strict) ---

    #[test]
    fn hex_decode_basic() {
        assert_eq!(hex_decode("deadbeef"), Some(vec![0xde, 0xad, 0xbe, 0xef]));
    }

    #[test]
    fn hex_decode_empty() {
        assert_eq!(hex_decode(""), Some(vec![]));
    }

    #[test]
    fn hex_decode_odd_length_returns_none() {
        assert_eq!(hex_decode("abc"), None);
    }

    #[test]
    fn hex_decode_invalid_chars_returns_none() {
        assert_eq!(hex_decode("gg"), None);
    }

    // -- bytes_to_hex ---

    #[test]
    fn bytes_to_hex_basic() {
        assert_eq!(bytes_to_hex(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
    }

    #[test]
    fn bytes_to_hex_empty() {
        assert_eq!(bytes_to_hex(&[]), "");
    }

    #[test]
    fn roundtrip_strict() {
        let original = vec![0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef];
        let hex = bytes_to_hex(&original);
        assert_eq!(hex_decode(&hex), Some(original));
    }
}
