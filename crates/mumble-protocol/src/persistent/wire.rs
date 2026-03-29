//! `MessagePack` wire format for all `fancy-pchat-*` payloads.
//!
//! Each struct corresponds to one `PluginDataTransmission` payload
//! identified by its `dataID`. Serialization uses `MessagePack` (compact
//! binary, schema-flexible).

use serde::{Deserialize, Serialize};

// ---- fancy-pchat-msg (section 6.2) ----------------------------------

/// Encrypted message sent from client to server for storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PchatMsg {
    /// Unique message identifier (UUID v4).
    pub message_id: String,
    /// Target channel.
    pub channel_id: u32,
    /// Unix epoch milliseconds.
    pub timestamp: u64,
    /// Sender's TLS certificate hash.
    pub sender_hash: String,
    /// `"FANCY_V1_POST_JOIN"` or `"FANCY_V1_FULL_ARCHIVE"`.
    #[serde(alias = "mode")]
    pub protocol: String,
    #[serde(with = "serde_bytes")]
    /// Encrypted message envelope.
    pub envelope: Vec<u8>,
    /// Epoch number (`POST_JOIN` only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub epoch: Option<u32>,
    /// Chain ratchet index within the epoch (`POST_JOIN` only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_index: Option<u32>,
    /// `SHA-256(epoch_key)[0..8]` for key cross-verification.
    #[serde(default, with = "serde_bytes")]
    pub epoch_fingerprint: Vec<u8>,
    /// If set, this message replaces a previous message by ID
    /// (epoch fork re-send). See design doc section 6.2.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replaces_id: Option<String>,
}

// ---- MessageEnvelope (section 6.3) ----------------------------------

/// Plaintext message content before encryption.
///
/// Serialized to `MessagePack`, then padded, then encrypted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEnvelope {
    /// Message body (HTML).
    pub body: String,
    /// Display name at send time.
    pub sender_name: String,
    /// Session ID at send time.
    pub sender_session: u32,
    /// Optional file attachments.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
}

/// A file attachment inside a [`MessageEnvelope`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    /// Original file name.
    pub name: String,
    /// MIME type (e.g. `"image/png"`).
    pub mime: String,
    /// Raw file bytes.
    #[serde(with = "serde_bytes")]
    pub data: Vec<u8>,
}

// ---- fancy-pchat-fetch (section 6.4) --------------------------------

/// Request stored messages from the server companion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PchatFetch {
    /// Target channel.
    pub channel_id: u32,
    /// Pagination cursor: fetch messages before this UUID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_id: Option<String>,
    /// Maximum messages to return (default 50).
    pub limit: u32,
    /// Pagination cursor: fetch messages after this UUID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_id: Option<String>,
}

// ---- fancy-pchat-fetch-resp (section 6.5) ---------------------------

/// Server response to a fetch request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PchatFetchResp {
    /// Target channel.
    pub channel_id: u32,
    /// Array of [`PchatMsg`] payloads.
    pub messages: Vec<PchatMsg>,
    /// Whether more messages are available (pagination).
    pub has_more: bool,
    /// Total messages stored for this channel.
    pub total_stored: u32,
}

// ---- fancy-pchat-key-exchange (section 6.6) -------------------------

/// Peer-to-peer key exchange, relayed through the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PchatKeyExchange {
    /// Target channel.
    pub channel_id: u32,
    /// `"FANCY_V1_POST_JOIN"` or `"FANCY_V1_FULL_ARCHIVE"`.
    #[serde(alias = "mode")]
    pub protocol: String,
    /// Key epoch number.
    pub epoch: u32,
    /// Epoch/channel key encrypted to the recipient's X25519 public key.
    #[serde(with = "serde_bytes")]
    pub encrypted_key: Vec<u8>,
    /// Cert hash of the key distributor.
    pub sender_hash: String,
    /// Cert hash of the intended recipient.
    pub recipient_hash: String,
    /// References a `fancy-pchat-key-request` (for dedup/relay tracking).
    /// `None` for epoch broadcasts to existing members.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// Unix epoch millis when this exchange was created.
    pub timestamp: u64,
    /// Algorithm version (must match sender's key-announce).
    pub algorithm_version: u8,
    /// Ed25519 signature over the canonical signed data.
    #[serde(with = "serde_bytes")]
    pub signature: Vec<u8>,
    /// `SHA-256(previous_epoch_key)[0..8]`, `POST_JOIN` only.
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        with = "option_serde_bytes"
    )]
    pub parent_fingerprint: Option<Vec<u8>>,
    /// `SHA-256(distributed_key)[0..8]`.
    #[serde(with = "serde_bytes")]
    pub epoch_fingerprint: Vec<u8>,
    /// Key custodian countersignature (see design doc section 5.6.4).
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        with = "option_serde_bytes"
    )]
    pub countersignature: Option<Vec<u8>>,
    /// Cert hash of the countersigning key custodian.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub countersigner_hash: Option<String>,
}

// ---- fancy-pchat-key-request (section 6.7) --------------------------

/// Server-broadcast key request when a new member joins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PchatKeyRequest {
    /// Target channel.
    pub channel_id: u32,
    /// `"FANCY_V1_POST_JOIN"` or `"FANCY_V1_FULL_ARCHIVE"`.
    #[serde(alias = "mode")]
    pub protocol: String,
    /// Cert hash of the user who needs a key.
    pub requester_hash: String,
    /// X25519 public key of the requester (32 bytes).
    #[serde(with = "serde_bytes")]
    pub requester_public: Vec<u8>,
    /// Server-generated UUID for dedup.
    pub request_id: String,
    /// Unix epoch millis when the server created this request.
    pub timestamp: u64,
    /// Max responses the server will relay (bandwidth cap).
    pub relay_cap: u32,
}

// ---- fancy-pchat-key-announce (section 6.8) -------------------------

/// Client announcement of E2EE identity public keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PchatKeyAnnounce {
    /// Algorithm version. 1 = X25519 + Ed25519.
    pub algorithm_version: u8,
    /// Key-agreement public key (X25519 for v1, 32 bytes).
    #[serde(with = "serde_bytes")]
    pub identity_public: Vec<u8>,
    /// Signature public key (Ed25519 for v1, 32 bytes).
    #[serde(with = "serde_bytes")]
    pub signing_public: Vec<u8>,
    /// TLS certificate hash.
    pub cert_hash: String,
    /// Announcement time (Unix epoch millis).
    pub timestamp: u64,
    /// Ed25519 signature proving control of `signing_public`.
    #[serde(with = "serde_bytes")]
    pub signature: Vec<u8>,
    /// TLS signature proving control of the TLS certificate.
    #[serde(with = "serde_bytes")]
    pub tls_signature: Vec<u8>,
}

// ---- fancy-pchat-epoch-countersig (section 5.6.4) -------------------

/// Key custodian countersignature on an epoch transition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PchatEpochCountersig {
    /// Target channel.
    pub channel_id: u32,
    /// Epoch number being countersigned.
    pub epoch: u32,
    #[serde(with = "serde_bytes")]
    /// `SHA-256(epoch_key)[0..8]` fingerprint of the endorsed key.
    pub epoch_fingerprint: Vec<u8>,
    #[serde(with = "serde_bytes")]
    /// `SHA-256(previous_epoch_key)[0..8]` fingerprint of the parent epoch.
    pub parent_fingerprint: Vec<u8>,
    /// Cert hash of the signer (key custodian).
    pub signer_hash: String,
    /// Cert hash of the key distributor this endorses.
    pub distributor_hash: String,
    /// Unix epoch millis.
    pub timestamp: u64,
    /// Ed25519 countersignature.
    #[serde(with = "serde_bytes")]
    pub countersignature: Vec<u8>,
}

// ---- fancy-pchat-ack (section 6.9) ----------------------------------

/// Server acknowledgement of message storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PchatAck {
    /// The message ID being acknowledged.
    pub message_id: String,
    /// `"stored"`, `"rejected"`, or `"quota_exceeded"`.
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Optional human-readable reason for rejection.
    pub reason: Option<String>,
}

/// Status values for [`PchatAck`].
pub mod ack_status {
    /// Message was successfully stored.
    pub const STORED: &str = "stored";
    /// Message was rejected by the server.
    pub const REJECTED: &str = "rejected";
    /// Server storage quota was exceeded.
    pub const QUOTA_EXCEEDED: &str = "quota_exceeded";
}

// ---- Codec trait for wire serialization/deserialization --------------

/// Trait for serializing and deserializing wire format payloads.
///
/// Provides a default `MessagePack` implementation. Consumers can
/// override for testing or alternative encodings.
pub trait WireCodec: Send + Sync {
    /// Serialize a payload to bytes.
    fn encode<T: Serialize>(&self, value: &T) -> crate::error::Result<Vec<u8>>;

    /// Deserialize a payload from bytes.
    fn decode<'a, T: Deserialize<'a>>(&self, data: &'a [u8]) -> crate::error::Result<T>;
}

/// Default `MessagePack` codec.
#[derive(Debug, Clone, Default)]
pub struct MsgPackCodec;

impl WireCodec for MsgPackCodec {
    fn encode<T: Serialize>(&self, value: &T) -> crate::error::Result<Vec<u8>> {
        rmp_serde::to_vec_named(value).map_err(|e| crate::error::Error::Other(e.to_string()))
    }

    fn decode<'a, T: Deserialize<'a>>(&self, data: &'a [u8]) -> crate::error::Result<T> {
        rmp_serde::from_slice(data).map_err(|e| crate::error::Error::Other(e.to_string()))
    }
}

// ---- Helper: Option<Vec<u8>> serde_bytes ----------------------------

/// Serde helper for `Option<Vec<u8>>` using byte-efficient encoding.
mod option_serde_bytes {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(super) fn serialize<S: Serializer>(val: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
        match val {
            Some(bytes) => serde_bytes::Bytes::new(bytes).serialize(s),
            None => s.serialize_none(),
        }
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
        let opt: Option<serde_bytes::ByteBuf> = Option::deserialize(d)?;
        Ok(opt.map(serde_bytes::ByteBuf::into_vec))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
    use super::*;

    #[test]
    fn pchat_msg_roundtrip() {
        let codec = MsgPackCodec;
        let msg = PchatMsg {
            message_id: "test-id".into(),
            channel_id: 1,
            timestamp: 1234567890,
            sender_hash: "abc".into(),
            protocol: "FANCY_V1_POST_JOIN".into(),
            envelope: vec![1, 2, 3],
            epoch: Some(1),
            chain_index: Some(0),
            epoch_fingerprint: vec![4, 5, 6, 7, 8, 9, 10, 11],
            replaces_id: None,
        };
        let bytes = codec.encode(&msg).unwrap();
        let decoded: PchatMsg = codec.decode(&bytes).unwrap();
        assert_eq!(decoded.message_id, msg.message_id);
        assert_eq!(decoded.channel_id, msg.channel_id);
        assert_eq!(decoded.epoch, Some(1));
    }

    #[test]
    fn message_envelope_roundtrip() {
        let codec = MsgPackCodec;
        let env = MessageEnvelope {
            body: "<b>Hello</b>".into(),
            sender_name: "Alice".into(),
            sender_session: 5,
            attachments: vec![Attachment {
                name: "test.txt".into(),
                mime: "text/plain".into(),
                data: b"hello".to_vec(),
            }],
        };
        let bytes = codec.encode(&env).unwrap();
        let decoded: MessageEnvelope = codec.decode(&bytes).unwrap();
        assert_eq!(decoded.body, env.body);
        assert_eq!(decoded.attachments.len(), 1);
    }

    #[test]
    fn pchat_fetch_roundtrip() {
        let codec = MsgPackCodec;
        let fetch = PchatFetch {
            channel_id: 42,
            before_id: Some("cursor-id".into()),
            limit: 50,
            after_id: None,
        };
        let bytes = codec.encode(&fetch).unwrap();
        let decoded: PchatFetch = codec.decode(&bytes).unwrap();
        assert_eq!(decoded.channel_id, 42);
        assert_eq!(decoded.limit, 50);
    }

    #[test]
    fn pchat_ack_roundtrip() {
        let codec = MsgPackCodec;
        let ack = PchatAck {
            message_id: "msg-1".into(),
            status: ack_status::STORED.into(),
            reason: None,
        };
        let bytes = codec.encode(&ack).unwrap();
        let decoded: PchatAck = codec.decode(&bytes).unwrap();
        assert_eq!(decoded.status, ack_status::STORED);
    }

    #[test]
    fn pchat_key_exchange_roundtrip() {
        let codec = MsgPackCodec;
        let kex = PchatKeyExchange {
            channel_id: 10,
            protocol: "FANCY_V1_FULL_ARCHIVE".into(),
            epoch: 0,
            encrypted_key: vec![0u8; 48],
            sender_hash: "sender".into(),
            recipient_hash: "recipient".into(),
            request_id: Some("req-1".into()),
            timestamp: 9999,
            algorithm_version: 1,
            signature: vec![0u8; 64],
            parent_fingerprint: None,
            epoch_fingerprint: vec![0u8; 8],
            countersignature: None,
            countersigner_hash: None,
        };
        let bytes = codec.encode(&kex).unwrap();
        let decoded: PchatKeyExchange = codec.decode(&bytes).unwrap();
        assert_eq!(decoded.channel_id, 10);
        assert_eq!(decoded.algorithm_version, 1);
        assert!(decoded.parent_fingerprint.is_none());
    }
}
