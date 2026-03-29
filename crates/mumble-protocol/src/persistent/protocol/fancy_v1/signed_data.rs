//! Signed data builder trait and standard byte-layout implementation.

// ---- Trait: SignedDataBuilder ----------------------------------------

/// Trait abstracting the byte-level layout of data that gets signed
/// in protocol messages.
pub trait SignedDataBuilder: Send + Sync {
    /// Build data for epoch countersignatures.
    fn build_countersig_data(
        &self,
        channel_id: u32,
        epoch: u32,
        epoch_fp: &[u8],
        parent_fp: &[u8],
        timestamp: u64,
        distributor_hash: &str,
    ) -> Vec<u8>;

    /// Build data signed in a key-exchange message.
    #[allow(clippy::too_many_arguments, reason = "protocol message construction requires all fields to be present")]
    fn build_key_exchange_signed_data(
        &self,
        algorithm_version: u8,
        channel_id: u32,
        mode: &crate::persistent::PchatProtocol,
        epoch: u32,
        encrypted_key: &[u8],
        recipient_hash: &str,
        request_id: Option<&str>,
        timestamp: u64,
    ) -> Vec<u8>;

    /// Build data signed in a key-announce message.
    fn build_key_announce_signed_data(
        &self,
        algorithm_version: u8,
        cert_hash: &str,
        timestamp: u64,
        identity_public: &[u8],
        signing_public: &[u8],
    ) -> Vec<u8>;
}

// ---- StandardSignedDataBuilder --------------------------------------

/// Standard byte-layout for signed protocol messages.
#[derive(Debug, Clone, Default)]
pub struct StandardSignedDataBuilder;

impl SignedDataBuilder for StandardSignedDataBuilder {
    fn build_countersig_data(
        &self,
        channel_id: u32,
        epoch: u32,
        epoch_fp: &[u8],
        parent_fp: &[u8],
        timestamp: u64,
        distributor_hash: &str,
    ) -> Vec<u8> {
        let mut data = Vec::with_capacity(4 + 4 + 8 + 8 + 8 + distributor_hash.len());
        data.extend_from_slice(&channel_id.to_be_bytes());
        data.extend_from_slice(&epoch.to_be_bytes());
        data.extend_from_slice(epoch_fp);
        data.extend_from_slice(parent_fp);
        data.extend_from_slice(&timestamp.to_be_bytes());
        data.extend_from_slice(distributor_hash.as_bytes());
        data
    }

    #[allow(clippy::too_many_arguments, reason = "protocol message construction requires all fields to be present")]
    fn build_key_exchange_signed_data(
        &self,
        algorithm_version: u8,
        channel_id: u32,
        mode: &crate::persistent::PchatProtocol,
        epoch: u32,
        encrypted_key: &[u8],
        recipient_hash: &str,
        request_id: Option<&str>,
        timestamp: u64,
    ) -> Vec<u8> {
        let mode_byte: u8 = match mode {
            crate::persistent::PchatProtocol::FancyV1PostJoin => 1,
            crate::persistent::PchatProtocol::FancyV1FullArchive => 2,
            _ => 0,
        };

        let req_id_bytes = request_id.unwrap_or("").as_bytes();
        let capacity =
            1 + 4 + 1 + 4 + encrypted_key.len() + recipient_hash.len() + req_id_bytes.len() + 8;
        let mut data = Vec::with_capacity(capacity);
        data.push(algorithm_version);
        data.extend_from_slice(&channel_id.to_be_bytes());
        data.push(mode_byte);
        data.extend_from_slice(&epoch.to_be_bytes());
        data.extend_from_slice(encrypted_key);
        data.extend_from_slice(recipient_hash.as_bytes());
        data.extend_from_slice(req_id_bytes);
        data.extend_from_slice(&timestamp.to_be_bytes());
        data
    }

    fn build_key_announce_signed_data(
        &self,
        algorithm_version: u8,
        cert_hash: &str,
        timestamp: u64,
        identity_public: &[u8],
        signing_public: &[u8],
    ) -> Vec<u8> {
        let capacity = 1 + cert_hash.len() + 8 + identity_public.len() + signing_public.len();
        let mut data = Vec::with_capacity(capacity);
        data.push(algorithm_version);
        data.extend_from_slice(cert_hash.as_bytes());
        data.extend_from_slice(&timestamp.to_be_bytes());
        data.extend_from_slice(identity_public);
        data.extend_from_slice(signing_public);
        data
    }
}

// ---- Convenience free functions -------------------------------------

/// Build the data that a key custodian signs for epoch countersignatures.
///
/// Convenience wrapper around [`StandardSignedDataBuilder`].
pub fn build_countersig_data(
    channel_id: u32,
    epoch: u32,
    epoch_fp: &[u8],
    parent_fp: &[u8],
    timestamp: u64,
    distributor_hash: &str,
) -> Vec<u8> {
    StandardSignedDataBuilder.build_countersig_data(
        channel_id,
        epoch,
        epoch_fp,
        parent_fp,
        timestamp,
        distributor_hash,
    )
}

/// Build the data signed in a key-exchange message (section 6.6).
///
/// Convenience wrapper around [`StandardSignedDataBuilder`].
#[allow(clippy::too_many_arguments, reason = "protocol message construction requires all fields to be present")]
pub fn build_key_exchange_signed_data(
    algorithm_version: u8,
    channel_id: u32,
    mode: &crate::persistent::PchatProtocol,
    epoch: u32,
    encrypted_key: &[u8],
    recipient_hash: &str,
    request_id: Option<&str>,
    timestamp: u64,
) -> Vec<u8> {
    StandardSignedDataBuilder.build_key_exchange_signed_data(
        algorithm_version,
        channel_id,
        mode,
        epoch,
        encrypted_key,
        recipient_hash,
        request_id,
        timestamp,
    )
}

/// Build the data signed in a key-announce message (section 6.8).
///
/// Convenience wrapper around [`StandardSignedDataBuilder`].
pub fn build_key_announce_signed_data(
    algorithm_version: u8,
    cert_hash: &str,
    timestamp: u64,
    identity_public: &[u8],
    signing_public: &[u8],
) -> Vec<u8> {
    StandardSignedDataBuilder.build_key_announce_signed_data(
        algorithm_version,
        cert_hash,
        timestamp,
        identity_public,
        signing_public,
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
    use super::*;

    #[test]
    fn key_exchange_signed_data_includes_all_fields() {
        let data = build_key_exchange_signed_data(
            1,
            42,
            &crate::persistent::PchatProtocol::FancyV1PostJoin,
            5,
            &[0xAA; 48],
            "recipient",
            Some("req-1"),
            12345,
        );
        // version(1) + channel(4) + mode(1) + epoch(4) + key(48) + recipient(9) + req_id(5) + ts(8)
        assert_eq!(data.len(), 1 + 4 + 1 + 4 + 48 + 9 + 5 + 8);
        assert_eq!(data[0], 1); // algorithm_version
    }

    #[test]
    fn key_announce_signed_data_format() {
        let data = build_key_announce_signed_data(1, "abc123", 99999, &[0; 32], &[0; 32]);
        // version(1) + cert_hash(6) + ts(8) + id_pub(32) + sign_pub(32) = 79
        assert_eq!(data.len(), 1 + 6 + 8 + 32 + 32);
        assert_eq!(data[0], 1);
    }

    #[test]
    fn countersig_data_format() {
        let data = build_countersig_data(1, 2, &[0; 8], &[0; 8], 5000, "dist");
        // channel(4) + epoch(4) + efp(8) + pfp(8) + ts(8) + dist(4) = 36
        assert_eq!(data.len(), 4 + 4 + 8 + 8 + 8 + 4);
    }
}
