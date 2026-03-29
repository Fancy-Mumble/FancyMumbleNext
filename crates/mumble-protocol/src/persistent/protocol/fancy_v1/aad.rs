//! AEAD Associated Authenticated Data builder trait and implementation.

use crate::error::{Error, Result};

// ---- Trait: AadBuilder ----------------------------------------------

/// Trait abstracting the construction of AEAD Associated Authenticated Data.
pub trait AadBuilder: Send + Sync {
    /// Build AAD bytes from channel metadata.
    ///
    /// `AAD = channel_id(4B BE) || message_id(16B UUID) || timestamp(8B BE)`
    fn build_aad(&self, channel_id: u32, message_id: &[u8; 16], timestamp: u64) -> Vec<u8>;

    /// Parse a UUID string into 16 raw bytes.
    fn uuid_to_bytes(&self, uuid_str: &str) -> Result<[u8; 16]>;
}

// ---- StandardAadBuilder ---------------------------------------------

/// Standard AAD builder: `channel_id(4B) || message_id(16B) || timestamp(8B)`.
#[derive(Debug, Clone, Default)]
pub struct StandardAadBuilder;

impl AadBuilder for StandardAadBuilder {
    fn build_aad(&self, channel_id: u32, message_id: &[u8; 16], timestamp: u64) -> Vec<u8> {
        let mut aad = Vec::with_capacity(4 + 16 + 8);
        aad.extend_from_slice(&channel_id.to_be_bytes());
        aad.extend_from_slice(message_id);
        aad.extend_from_slice(&timestamp.to_be_bytes());
        aad
    }

    fn uuid_to_bytes(&self, uuid_str: &str) -> Result<[u8; 16]> {
        let id = uuid::Uuid::parse_str(uuid_str)
            .map_err(|e| Error::InvalidState(format!("invalid UUID: {e}")))?;
        Ok(*id.as_bytes())
    }
}

// ---- Convenience free functions -------------------------------------

/// Build the Associated Authenticated Data for AEAD.
///
/// Convenience wrapper around [`StandardAadBuilder`].
pub fn build_aad(channel_id: u32, message_id: &[u8; 16], timestamp: u64) -> Vec<u8> {
    StandardAadBuilder.build_aad(channel_id, message_id, timestamp)
}

/// Parse a UUID string into 16 raw bytes.
///
/// Convenience wrapper around [`StandardAadBuilder`].
pub fn uuid_to_bytes(uuid_str: &str) -> Result<[u8; 16]> {
    StandardAadBuilder.uuid_to_bytes(uuid_str)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
    use super::*;

    #[test]
    fn build_aad_format() {
        let aad = build_aad(1, &[0u8; 16], 100);
        assert_eq!(aad.len(), 4 + 16 + 8);
        // channel_id = 1 big-endian
        assert_eq!(&aad[..4], &[0, 0, 0, 1]);
        // timestamp = 100 big-endian
        assert_eq!(&aad[20..], &100u64.to_be_bytes());
    }
}
