//! Codec trait for Fancy Mumble extension messages (types 100+).
//!
//! Abstracts the difference between Fancy Mumble servers (which understand
//! native extension message types) and legacy Mumble servers (which only
//! support the standard protocol). On a legacy server, client-to-client
//! Fancy messages are wrapped inside `PluginDataTransmission` for relay.

use std::fmt::Debug;

use tracing::{debug, warn};

use crate::fancy_message_support::{message_support, FallbackPolicy, MessageSupport};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;
use crate::transport::codec;

/// Minimum server fancy version that supports native Fancy message types.
///
/// Servers at or above this version receive extension messages directly.
/// Servers below this version (or without `fancy_version` at all) require
/// the [`LegacyCodec`] `PluginData` wrapper.
///
/// 0.2.12 = `(0 << 48) | (2 << 32) | (12 << 16)`.
pub const FANCY_NATIVE_MIN_VERSION: u64 =
    fancy_utils::version::fancy_version_encode(0, 2, 12);

/// Prefix for `PluginDataTransmission.data_id` identifying a wrapped
/// Fancy extension message. Followed by the decimal `TcpMessageType` ID.
const WRAPPED_DATA_ID_PREFIX: &str = "fancy-native:";

// ---- Trait ---------------------------------------------------------

/// Codec for encoding/decoding Fancy Mumble extension messages.
///
/// Two implementations exist:
/// - [`NativeCodec`]: version-aware codec for Fancy servers (>= 0.2.12).
///   Messages the server is too old for are automatically wrapped in
///   `PluginData` when their [`FallbackPolicy`] allows it.
/// - [`LegacyCodec`]: wraps *all* extension types in `PluginData` for
///   legacy (non-Fancy) Mumble servers.
pub trait FancyCodec: Send + Sync + Debug {
    /// Encode an outbound [`ControlMessage`] for the wire.
    ///
    /// Returns `Some(msg)` with the (possibly transformed) message, or
    /// `None` if the message cannot be sent on this server type (e.g. a
    /// server-processed Fancy message on a legacy server).
    fn encode(&self, msg: ControlMessage, state: &ServerState) -> Option<ControlMessage>;

    /// Decode an inbound [`ControlMessage`], potentially unwrapping a
    /// Fancy extension message that was tunnelled inside `PluginData`.
    fn decode(&self, msg: ControlMessage) -> ControlMessage;
}

/// Select the appropriate codec based on the server's announced Fancy
/// version.
pub fn select_codec(server_fancy_version: Option<u64>) -> Box<dyn FancyCodec> {
    debug!(
        raw_version = ?server_fancy_version,
        decoded = ?server_fancy_version.map(fancy_utils::version::fancy_version_decode),
        "select_codec called"
    );
    match server_fancy_version {
        Some(v) if v >= FANCY_NATIVE_MIN_VERSION => {
            Box::new(NativeCodec { server_version: v })
        }
        _ => Box::new(LegacyCodec),
    }
}

// ---- NativeCodec ---------------------------------------------------

/// Version-aware codec for Fancy Mumble servers (>= 0.2.12).
///
/// Messages that the connected server natively understands are sent
/// as-is.  Messages added in a *newer* server version than the one we
/// are connected to are either wrapped in `PluginData` (when the
/// message's [`FallbackPolicy`] is [`FallbackPolicy::PluginData`]) or
/// dropped with a warning.
///
/// The decode path always attempts to unwrap `fancy-native:*`
/// `PluginData` envelopes so that fallback messages from peers are
/// handled correctly.
#[derive(Debug)]
pub struct NativeCodec {
    server_version: u64,
}

impl FancyCodec for NativeCodec {
    fn encode(&self, msg: ControlMessage, state: &ServerState) -> Option<ControlMessage> {
        if !msg.is_fancy_extension() {
            return Some(msg);
        }

        let support = message_support(&msg);
        debug!(
            type_id = msg.type_id(),
            server_version = self.server_version,
            min_version = support.map(|s| s.min_version),
            version_ok = support.map(|s| self.server_version >= s.min_version),
            fallback = ?support.map(|s| s.fallback),
            "NativeCodec: encode decision"
        );
        match support {
            Some(s) if self.server_version >= s.min_version => {
                debug!(type_id = msg.type_id(), "NativeCodec: sending natively");
                Some(msg)
            }
            Some(MessageSupport { fallback: FallbackPolicy::PluginData, .. }) => {
                debug!(
                    type_id = msg.type_id(),
                    "NativeCodec: server too old, falling back to PluginData"
                );
                LegacyCodec.encode(msg, state)
            }
            Some(_) => {
                debug!(
                    type_id = msg.type_id(),
                    "NativeCodec: server too old, no fallback, dropping"
                );
                None
            }
            None => Some(msg),
        }
    }

    fn decode(&self, msg: ControlMessage) -> ControlMessage {
        LegacyCodec.decode(msg)
    }
}

// ---- LegacyCodec ---------------------------------------------------

/// Legacy codec that wraps Fancy extension messages in `PluginData`.
///
/// Standard Mumble types (0-26) pass through unchanged on both paths.
/// Fancy extension types (100+) are serialized into a
/// `PluginDataTransmission` envelope on send and deserialized back on
/// receive.
#[derive(Debug)]
pub struct LegacyCodec;

impl FancyCodec for LegacyCodec {
    fn encode(&self, msg: ControlMessage, state: &ServerState) -> Option<ControlMessage> {
        if !msg.is_fancy_extension() {
            return Some(msg);
        }

        let receiver_sessions = extract_receiver_sessions(&msg, state);
        if receiver_sessions.is_empty() {
            warn!(
                "cannot relay Fancy message on legacy server: \
                 no receiver sessions could be determined"
            );
            return None;
        }

        let (type_id, payload) = match codec::serialize_control_message(&msg) {
            Ok(pair) => pair,
            Err(e) => {
                warn!("failed to serialize Fancy message for PluginData wrapping: {e}");
                return None;
            }
        };

        let data_id = format!("{WRAPPED_DATA_ID_PREFIX}{type_id}");

        Some(ControlMessage::PluginDataTransmission(
            mumble_tcp::PluginDataTransmission {
                sender_session: None,
                receiver_sessions,
                data: Some(payload),
                data_id: Some(data_id),
            },
        ))
    }

    fn decode(&self, msg: ControlMessage) -> ControlMessage {
        let ControlMessage::PluginDataTransmission(ref pd) = msg else {
            return msg;
        };

        let Some(ref data_id) = pd.data_id else {
            return msg;
        };

        let Some(type_id_str) = data_id.strip_prefix(WRAPPED_DATA_ID_PREFIX) else {
            return msg;
        };

        let Ok(type_id) = type_id_str.parse::<u16>() else {
            warn!("invalid Fancy type ID in PluginData data_id: {data_id}");
            return msg;
        };

        let Some(ref payload) = pd.data else {
            warn!("Fancy PluginData wrapper has no data payload");
            return msg;
        };

        match codec::deserialize_control_message(type_id, payload) {
            Ok(decoded) => {
                let sender = pd.sender_session;
                patch_sender_session(decoded, sender)
            }
            Err(e) => {
                warn!(
                    "failed to decode wrapped Fancy message (type {type_id}): {e}"
                );
                msg
            }
        }
    }
}

// ---- Helpers -------------------------------------------------------

/// Transfer the `sender_session` from the `PluginData` envelope into
/// the decoded message's actor/sender field.
///
/// When a Fancy extension message is relayed via `PluginData`, the
/// Mumble server fills `PluginDataTransmission.sender_session` but
/// does not parse the inner payload. Fields like
/// `FancyTypingIndicator.actor` (normally set by the server on native
/// messages) will be `None`. This function patches them.
fn patch_sender_session(mut msg: ControlMessage, sender: Option<u32>) -> ControlMessage {
    match &mut msg {
        ControlMessage::FancyTypingIndicator(ti) if ti.actor.is_none() => {
            ti.actor = sender;
        }
        _ => {}
    }
    msg
}

/// Extract receiver session IDs from a Fancy extension message so the
/// legacy server knows whom to relay the `PluginData` to.
///
/// Returns an empty `Vec` for server-processed message types that have
/// no meaningful client-to-client relay target.
fn extract_receiver_sessions(msg: &ControlMessage, state: &ServerState) -> Vec<u32> {
    let own_session = state.own_session().unwrap_or(0);

    match msg {
        ControlMessage::WebRtcSignal(signal) => {
            let target = signal.target_session.unwrap_or(0);
            if target != 0 {
                vec![target]
            } else {
                channel_members_except_self(state, own_session)
            }
        }
        ControlMessage::PchatSenderKeyDistribution(skd) => {
            let channel_id = skd.channel_id.unwrap_or(0);
            state
                .users
                .values()
                .filter(|u| u.channel_id == channel_id && u.session != own_session)
                .map(|u| u.session)
                .collect()
        }
        ControlMessage::FancyTypingIndicator(_) => {
            channel_members_except_self(state, own_session)
        }
        _ => Vec::new(),
    }
}

/// All user sessions in our current channel, excluding ourselves.
fn channel_members_except_self(state: &ServerState, own_session: u32) -> Vec<u32> {
    let own_channel = state
        .users
        .get(&own_session)
        .map(|u| u.channel_id)
        .unwrap_or(0);

    state
        .users
        .values()
        .filter(|u| u.channel_id == own_channel && u.session != own_session)
        .map(|u| u.session)
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]

    use super::*;
    use crate::proto::mumble_tcp;

    fn state_with_users() -> ServerState {
        let mut state = ServerState::new();
        state.apply_server_sync(&mumble_tcp::ServerSync {
            session: Some(1),
            ..Default::default()
        });
        // Own user in channel 0.
        state.apply_user_state(&mumble_tcp::UserState {
            session: Some(1),
            name: Some("self".into()),
            channel_id: Some(0),
            ..Default::default()
        });
        // Peer in channel 0.
        state.apply_user_state(&mumble_tcp::UserState {
            session: Some(2),
            name: Some("peer".into()),
            channel_id: Some(0),
            ..Default::default()
        });
        // Peer in channel 1 (different channel).
        state.apply_user_state(&mumble_tcp::UserState {
            session: Some(3),
            name: Some("other".into()),
            channel_id: Some(1),
            ..Default::default()
        });
        state
    }

    // ---- select_codec ------------------------------------------------

    #[test]
    fn select_codec_native_for_new_server() {
        let codec = select_codec(Some(FANCY_NATIVE_MIN_VERSION));
        assert!(format!("{codec:?}").contains("NativeCodec"));
    }

    #[test]
    fn select_codec_legacy_for_old_server() {
        let old_version = fancy_utils::version::fancy_version_encode(0, 2, 11);
        let codec = select_codec(Some(old_version));
        assert!(format!("{codec:?}").contains("LegacyCodec"));
    }

    #[test]
    fn select_codec_legacy_when_no_version() {
        let codec = select_codec(None);
        assert!(format!("{codec:?}").contains("LegacyCodec"));
    }

    // ---- NativeCodec -------------------------------------------------

    const V_0_2_12: u64 = fancy_utils::version::fancy_version_encode(0, 2, 12);
    const V_0_2_14: u64 = fancy_utils::version::fancy_version_encode(0, 2, 14);
    const V_0_2_16: u64 = fancy_utils::version::fancy_version_encode(0, 2, 16);

    #[test]
    fn native_codec_passthrough_standard_message() {
        let codec = NativeCodec { server_version: V_0_2_12 };
        let state = ServerState::new();
        let ping = ControlMessage::Ping(mumble_tcp::Ping {
            timestamp: Some(42),
            ..Default::default()
        });
        let encoded = codec.encode(ping.clone(), &state).unwrap();
        assert!(matches!(encoded, ControlMessage::Ping(_)));
    }

    #[test]
    fn native_codec_passthrough_fancy_message_when_server_supports_it() {
        let codec = NativeCodec { server_version: V_0_2_12 };
        let state = ServerState::new();
        let signal = ControlMessage::WebRtcSignal(mumble_tcp::WebRtcSignal {
            target_session: Some(5),
            signal_type: Some(0),
            payload: Some("test".into()),
            ..Default::default()
        });
        let encoded = codec.encode(signal, &state).unwrap();
        assert!(matches!(encoded, ControlMessage::WebRtcSignal(_)));
    }

    #[test]
    fn native_codec_falls_back_to_plugin_data_when_server_too_old() {
        let codec = NativeCodec { server_version: V_0_2_14 };
        let state = state_with_users();
        let msg = ControlMessage::FancyTypingIndicator(mumble_tcp::FancyTypingIndicator {
            channel_id: Some(0),
            actor: None,
        });

        let encoded = codec.encode(msg, &state).unwrap();
        let ControlMessage::PluginDataTransmission(pd) = &encoded else {
            panic!("expected PluginDataTransmission, got {encoded:?}");
        };
        assert_eq!(pd.data_id.as_deref(), Some("fancy-native:131"));
        assert_eq!(pd.receiver_sessions, vec![2]);
    }

    #[test]
    fn native_codec_passthrough_typing_indicator_when_server_new_enough() {
        let codec = NativeCodec { server_version: V_0_2_16 };
        let state = state_with_users();
        let msg = ControlMessage::FancyTypingIndicator(mumble_tcp::FancyTypingIndicator {
            channel_id: Some(0),
            actor: None,
        });
        let encoded = codec.encode(msg, &state).unwrap();
        assert!(matches!(encoded, ControlMessage::FancyTypingIndicator(_)));
    }

    #[test]
    fn native_codec_drops_server_only_message_when_unsupported() {
        let codec = NativeCodec { server_version: V_0_2_14 };
        let state = state_with_users();
        let msg = ControlMessage::PchatPin(mumble_tcp::PchatPin {
            channel_id: Some(0),
            ..Default::default()
        });
        assert!(codec.encode(msg, &state).is_none());
    }

    #[test]
    fn native_codec_decode_passthrough() {
        let codec = NativeCodec { server_version: V_0_2_12 };
        let msg = ControlMessage::Ping(mumble_tcp::Ping {
            timestamp: Some(99),
            ..Default::default()
        });
        let decoded = codec.decode(msg);
        assert!(matches!(decoded, ControlMessage::Ping(_)));
    }

    #[test]
    fn native_codec_decode_unwraps_and_patches_sender() {
        let codec = NativeCodec { server_version: V_0_2_14 };

        // actor is None in the inner payload (client never sets it).
        let original = mumble_tcp::FancyTypingIndicator {
            actor: None,
            channel_id: Some(0),
        };
        let payload = prost::Message::encode_to_vec(&original);
        let wrapped = ControlMessage::PluginDataTransmission(
            mumble_tcp::PluginDataTransmission {
                sender_session: Some(2),
                receiver_sessions: vec![1],
                data: Some(payload),
                data_id: Some("fancy-native:131".into()),
            },
        );
        let decoded = codec.decode(wrapped);
        let ControlMessage::FancyTypingIndicator(ti) = decoded else {
            panic!("expected FancyTypingIndicator, got {decoded:?}");
        };
        assert_eq!(ti.actor, Some(2), "actor patched from sender_session");
        assert_eq!(ti.channel_id, Some(0));
    }

    // ---- LegacyCodec encode ------------------------------------------

    #[test]
    fn legacy_codec_passthrough_standard_message() {
        let codec = LegacyCodec;
        let state = ServerState::new();
        let ping = ControlMessage::Ping(mumble_tcp::Ping {
            timestamp: Some(42),
            ..Default::default()
        });
        let result = codec.encode(ping, &state).unwrap();
        assert!(matches!(result, ControlMessage::Ping(_)));
    }

    #[test]
    fn legacy_codec_wraps_webrtc_signal_in_plugin_data() {
        let codec = LegacyCodec;
        let state = state_with_users();
        let signal = ControlMessage::WebRtcSignal(mumble_tcp::WebRtcSignal {
            target_session: Some(2),
            signal_type: Some(0),
            payload: Some("sdp-offer".into()),
            ..Default::default()
        });

        let encoded = codec.encode(signal, &state).unwrap();
        let ControlMessage::PluginDataTransmission(pd) = &encoded else {
            panic!("expected PluginDataTransmission, got {encoded:?}");
        };

        assert_eq!(pd.data_id.as_deref(), Some("fancy-native:120"));
        assert_eq!(pd.receiver_sessions, vec![2]);
        assert!(pd.data.is_some());
    }

    #[test]
    fn legacy_codec_drops_server_only_fancy_message() {
        let codec = LegacyCodec;
        let state = state_with_users();
        let receipt = ControlMessage::FancyReadReceipt(mumble_tcp::FancyReadReceipt {
            channel_id: Some(0),
            last_read_message_id: Some("msg-1".into()),
            ..Default::default()
        });

        // No receiver sessions can be determined -> None.
        assert!(codec.encode(receipt, &state).is_none());
    }

    #[test]
    fn legacy_codec_wraps_sender_key_distribution() {
        let codec = LegacyCodec;
        let state = state_with_users();
        let skd = ControlMessage::PchatSenderKeyDistribution(
            mumble_tcp::PchatSenderKeyDistribution {
                channel_id: Some(0),
                sender_hash: None,
                distribution: Some(vec![1, 2, 3]),
            },
        );

        let encoded = codec.encode(skd, &state).unwrap();
        let ControlMessage::PluginDataTransmission(pd) = &encoded else {
            panic!("expected PluginDataTransmission");
        };

        assert_eq!(pd.data_id.as_deref(), Some("fancy-native:121"));
        assert_eq!(pd.receiver_sessions, vec![2]);
    }

    // ---- LegacyCodec decode ------------------------------------------

    #[test]
    fn legacy_codec_decode_passthrough() {
        let codec = LegacyCodec;

        // Standard message passes through.
        let ping = ControlMessage::Ping(mumble_tcp::Ping { timestamp: Some(42), ..Default::default() });
        assert!(matches!(codec.decode(ping), ControlMessage::Ping(_)));

        // Non-fancy PluginData passes through.
        let pd = ControlMessage::PluginDataTransmission(mumble_tcp::PluginDataTransmission {
            sender_session: Some(2),
            receiver_sessions: vec![1],
            data: Some(b"poll-json".to_vec()),
            data_id: Some("fancy-poll".into()),
        });
        assert!(matches!(codec.decode(pd), ControlMessage::PluginDataTransmission(_)));
    }

    #[test]
    fn legacy_codec_unwraps_fancy_native_plugin_data() {
        let codec = LegacyCodec;

        // Manually wrap a WebRtcSignal the way `encode` would.
        let original = mumble_tcp::WebRtcSignal {
            target_session: Some(2),
            signal_type: Some(0),
            payload: Some("offer".into()),
            ..Default::default()
        };
        let payload = prost::Message::encode_to_vec(&original);

        let wrapped = ControlMessage::PluginDataTransmission(
            mumble_tcp::PluginDataTransmission {
                sender_session: Some(5),
                receiver_sessions: vec![1],
                data: Some(payload),
                data_id: Some("fancy-native:120".into()),
            },
        );

        let decoded = codec.decode(wrapped);
        let ControlMessage::WebRtcSignal(signal) = decoded else {
            panic!("expected WebRtcSignal, got {decoded:?}");
        };

        assert_eq!(signal.target_session, Some(2));
        assert_eq!(signal.payload.as_deref(), Some("offer"));
    }

    // ---- Round-trip ---------------------------------------------------

    #[test]
    fn legacy_codec_roundtrip_webrtc_signal() {
        let codec = LegacyCodec;
        let state = state_with_users();

        let original = ControlMessage::WebRtcSignal(mumble_tcp::WebRtcSignal {
            target_session: Some(2),
            signal_type: Some(2),
            payload: Some(r#"{"sdp":"v=0..."}"#.into()),
            ..Default::default()
        });

        let encoded = codec.encode(original, &state).unwrap();
        assert!(matches!(
            encoded,
            ControlMessage::PluginDataTransmission(_)
        ));

        let decoded = codec.decode(encoded);
        let ControlMessage::WebRtcSignal(signal) = decoded else {
            panic!("expected WebRtcSignal after round-trip");
        };

        assert_eq!(signal.target_session, Some(2));
        assert_eq!(signal.signal_type, Some(2));
        assert_eq!(
            signal.payload.as_deref(),
            Some(r#"{"sdp":"v=0..."}"#)
        );
    }

    #[test]
    fn legacy_codec_roundtrip_sender_key_distribution() {
        let codec = LegacyCodec;
        let state = state_with_users();

        let original = ControlMessage::PchatSenderKeyDistribution(
            mumble_tcp::PchatSenderKeyDistribution {
                channel_id: Some(0),
                sender_hash: Some("abc123".into()),
                distribution: Some(vec![10, 20, 30]),
            },
        );

        let encoded = codec.encode(original, &state).unwrap();
        let decoded = codec.decode(encoded);

        let ControlMessage::PchatSenderKeyDistribution(skd) = decoded else {
            panic!("expected PchatSenderKeyDistribution after round-trip");
        };

        assert_eq!(skd.channel_id, Some(0));
        assert_eq!(skd.sender_hash.as_deref(), Some("abc123"));
        assert_eq!(skd.distribution, Some(vec![10, 20, 30]));
    }

    #[test]
    fn legacy_decode_ignores_invalid_or_missing_payload() {
        let codec = LegacyCodec;

        // Invalid type ID string.
        let msg = ControlMessage::PluginDataTransmission(
            mumble_tcp::PluginDataTransmission {
                sender_session: Some(2),
                receiver_sessions: vec![1],
                data: Some(vec![0, 1, 2]),
                data_id: Some("fancy-native:not-a-number".into()),
            },
        );
        assert!(matches!(codec.decode(msg), ControlMessage::PluginDataTransmission(_)));

        // Missing payload.
        let msg = ControlMessage::PluginDataTransmission(
            mumble_tcp::PluginDataTransmission {
                sender_session: Some(2),
                receiver_sessions: vec![1],
                data: None,
                data_id: Some("fancy-native:120".into()),
            },
        );
        assert!(matches!(codec.decode(msg), ControlMessage::PluginDataTransmission(_)));
    }

    // ---- NativeCodec fallback round-trip -----------------------------

    #[test]
    fn native_codec_roundtrip_typing_indicator_via_fallback() {
        let codec = NativeCodec { server_version: V_0_2_14 };
        let state = state_with_users();

        let original = ControlMessage::FancyTypingIndicator(
            mumble_tcp::FancyTypingIndicator {
                channel_id: Some(0),
                actor: None,
            },
        );

        let encoded = codec.encode(original, &state).unwrap();
        let ControlMessage::PluginDataTransmission(mut pd) = encoded else {
            panic!("expected PluginData fallback");
        };

        // Simulate: the server fills sender_session before relaying.
        pd.sender_session = Some(1);
        let relayed = ControlMessage::PluginDataTransmission(pd);

        let decoded = codec.decode(relayed);
        let ControlMessage::FancyTypingIndicator(ti) = decoded else {
            panic!("expected FancyTypingIndicator after round-trip, got {decoded:?}");
        };
        assert_eq!(ti.channel_id, Some(0));
        assert_eq!(ti.actor, Some(1), "actor should be patched from sender_session");
    }

    #[test]
    fn legacy_decode_patches_sender_session_into_typing_indicator() {
        let codec = LegacyCodec;
        let original = mumble_tcp::FancyTypingIndicator {
            actor: None,
            channel_id: Some(5),
        };
        let payload = prost::Message::encode_to_vec(&original);
        let wrapped = ControlMessage::PluginDataTransmission(
            mumble_tcp::PluginDataTransmission {
                sender_session: Some(7),
                receiver_sessions: vec![1],
                data: Some(payload),
                data_id: Some("fancy-native:131".into()),
            },
        );
        let decoded = codec.decode(wrapped);
        let ControlMessage::FancyTypingIndicator(ti) = decoded else {
            panic!("expected FancyTypingIndicator, got {decoded:?}");
        };
        assert_eq!(ti.actor, Some(7));
        assert_eq!(ti.channel_id, Some(5));
    }
}
