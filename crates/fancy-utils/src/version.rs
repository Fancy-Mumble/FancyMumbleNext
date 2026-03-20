//! Fancy Mumble version encoding using the Mumble v2 scheme.

/// Encode a Fancy Mumble version using the Mumble v2 scheme:
/// `(major << 48) | (minor << 32) | (patch << 16)`.
pub const fn fancy_version_encode(major: u16, minor: u16, patch: u16) -> u64 {
    ((major as u64) << 48) | ((minor as u64) << 32) | ((patch as u64) << 16)
}

/// Decode a Fancy Mumble v2-encoded version into (major, minor, patch).
pub const fn fancy_version_decode(v: u64) -> (u16, u16, u16) {
    let major = ((v >> 48) & 0xFFFF) as u16;
    let minor = ((v >> 32) & 0xFFFF) as u16;
    let patch = ((v >> 16) & 0xFFFF) as u16;
    (major, minor, patch)
}

/// Format a v2-encoded Fancy Mumble version as `"major.minor.patch"`.
pub fn fancy_version_string(v: u64) -> String {
    let (major, minor, patch) = fancy_version_decode(v);
    format!("{major}.{minor}.{patch}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        let v = fancy_version_encode(1, 2, 3);
        assert_eq!(fancy_version_decode(v), (1, 2, 3));
    }

    #[test]
    fn version_string() {
        let v = fancy_version_encode(0, 2, 0);
        assert_eq!(fancy_version_string(v), "0.2.0");
    }

    #[test]
    fn zero_version() {
        let v = fancy_version_encode(0, 0, 0);
        assert_eq!(fancy_version_decode(v), (0, 0, 0));
        assert_eq!(fancy_version_string(v), "0.0.0");
    }
}
