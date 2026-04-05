//! Proto <-> Wire type conversions for persistent chat messages.

use mumble_protocol::persistent::wire::{
    PchatKeyAnnounce as WireKeyAnnounce,
    PchatKeyExchange as WireKeyExchange,
    PchatKeyRequest as WireKeyRequest,
};
use mumble_protocol::persistent::PchatProtocol;
use mumble_protocol::proto::mumble_tcp;

// -- Protocol enum conversion -----------------------------------------

pub(crate) fn protocol_to_proto(protocol: PchatProtocol) -> i32 {
    match protocol {
        PchatProtocol::FancyV1PostJoin => mumble_tcp::PchatProtocol::FancyV1PostJoin as i32,
        PchatProtocol::FancyV1FullArchive => mumble_tcp::PchatProtocol::FancyV1FullArchive as i32,
        PchatProtocol::SignalV1 => mumble_tcp::PchatProtocol::SignalV1 as i32,
        PchatProtocol::ServerManaged => mumble_tcp::PchatProtocol::ServerManaged as i32,
        PchatProtocol::None => mumble_tcp::PchatProtocol::None as i32,
    }
}

pub(crate) fn proto_to_protocol(proto: Option<i32>) -> PchatProtocol {
    match proto.and_then(|v| mumble_tcp::PchatProtocol::try_from(v).ok()) {
        Some(mumble_tcp::PchatProtocol::FancyV1PostJoin) => PchatProtocol::FancyV1PostJoin,
        Some(mumble_tcp::PchatProtocol::FancyV1FullArchive) => PchatProtocol::FancyV1FullArchive,
        Some(mumble_tcp::PchatProtocol::SignalV1) => PchatProtocol::SignalV1,
        Some(mumble_tcp::PchatProtocol::ServerManaged) => PchatProtocol::ServerManaged,
        _ => PchatProtocol::FancyV1PostJoin,
    }
}

pub(super) fn proto_protocol_to_wire_str(proto: Option<i32>) -> String {
    proto_to_protocol(proto).as_wire_str().to_string()
}

pub(super) fn wire_protocol_str_to_proto(s: &str) -> i32 {
    protocol_to_proto(PchatProtocol::from_wire_str(s))
}

// -- Key announce conversion ------------------------------------------

pub(crate) fn wire_key_announce_to_proto(w: &WireKeyAnnounce) -> mumble_tcp::PchatKeyAnnounce {
    mumble_tcp::PchatKeyAnnounce {
        algorithm_version: Some(w.algorithm_version as u32),
        identity_public: Some(w.identity_public.clone()),
        signing_public: Some(w.signing_public.clone()),
        cert_hash: Some(w.cert_hash.clone()),
        timestamp: Some(w.timestamp),
        signature: Some(w.signature.clone()),
        tls_signature: Some(w.tls_signature.clone()),
    }
}

pub(super) fn proto_to_wire_key_announce(p: &mumble_tcp::PchatKeyAnnounce) -> WireKeyAnnounce {
    WireKeyAnnounce {
        algorithm_version: p.algorithm_version.unwrap_or(1) as u8,
        identity_public: p.identity_public.clone().unwrap_or_default(),
        signing_public: p.signing_public.clone().unwrap_or_default(),
        cert_hash: p.cert_hash.clone().unwrap_or_default(),
        timestamp: p.timestamp.unwrap_or(0),
        signature: p.signature.clone().unwrap_or_default(),
        tls_signature: p.tls_signature.clone().unwrap_or_default(),
    }
}

// -- Key request conversion -------------------------------------------

pub(super) fn proto_to_wire_key_request(p: &mumble_tcp::PchatKeyRequest) -> WireKeyRequest {
    WireKeyRequest {
        channel_id: p.channel_id.unwrap_or(0),
        protocol: proto_protocol_to_wire_str(p.protocol),
        requester_hash: p.requester_hash.clone().unwrap_or_default(),
        requester_public: p.requester_public.clone().unwrap_or_default(),
        request_id: p.request_id.clone().unwrap_or_default(),
        timestamp: p.timestamp.unwrap_or(0),
        relay_cap: p.relay_cap.unwrap_or(3),
    }
}

// -- Key exchange conversion ------------------------------------------

pub(crate) fn wire_key_exchange_to_proto(w: &WireKeyExchange) -> mumble_tcp::PchatKeyExchange {
    mumble_tcp::PchatKeyExchange {
        channel_id: Some(w.channel_id),
        protocol: Some(wire_protocol_str_to_proto(&w.protocol)),
        epoch: Some(w.epoch),
        encrypted_key: Some(w.encrypted_key.clone()),
        sender_hash: Some(w.sender_hash.clone()),
        recipient_hash: Some(w.recipient_hash.clone()),
        request_id: w.request_id.clone(),
        timestamp: Some(w.timestamp),
        algorithm_version: Some(w.algorithm_version as u32),
        signature: Some(w.signature.clone()),
        parent_fingerprint: w.parent_fingerprint.clone(),
        epoch_fingerprint: if w.epoch_fingerprint.is_empty() {
            None
        } else {
            Some(w.epoch_fingerprint.clone())
        },
        countersignature: w.countersignature.clone(),
        countersigner_hash: w.countersigner_hash.clone(),
    }
}

pub(super) fn proto_to_wire_key_exchange(p: &mumble_tcp::PchatKeyExchange) -> WireKeyExchange {
    WireKeyExchange {
        channel_id: p.channel_id.unwrap_or(0),
        protocol: proto_protocol_to_wire_str(p.protocol),
        epoch: p.epoch.unwrap_or(0),
        encrypted_key: p.encrypted_key.clone().unwrap_or_default(),
        sender_hash: p.sender_hash.clone().unwrap_or_default(),
        recipient_hash: p.recipient_hash.clone().unwrap_or_default(),
        request_id: p.request_id.clone(),
        timestamp: p.timestamp.unwrap_or(0),
        algorithm_version: p.algorithm_version.unwrap_or(1) as u8,
        signature: p.signature.clone().unwrap_or_default(),
        parent_fingerprint: p.parent_fingerprint.clone(),
        epoch_fingerprint: p.epoch_fingerprint.clone().unwrap_or_default(),
        countersignature: p.countersignature.clone(),
        countersigner_hash: p.countersigner_hash.clone(),
    }
}
