//! Codec trait for Fancy Mumble extension messages (types 100+).
//!
//! Abstracts the difference between Fancy Mumble servers (which understand
//! native extension message types) and legacy Mumble servers (which only
//! support the standard protocol). On a legacy server, client-to-client
//! Fancy messages are wrapped inside `PluginDataTransmission` for relay.

use std::fmt::Debug;

use tracing::warn;

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
/// - [`NativeCodec`]: passthrough for Fancy servers (>= 0.2.12).
/// - [`LegacyCodec`]: wraps extension types in `PluginData` for legacy
///   servers, and unwraps them on the receive path.
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
    let is_native = server_fancy_version
        .is_some_and(|v| v >= FANCY_NATIVE_MIN_VERSION);

    if is_native {
        Box::new(NativeCodec)
    } else {
        Box::new(LegacyCodec)
    }
}

// ---- NativeCodec ---------------------------------------------------

/// Direct codec for Fancy Mumble servers.
///
/// All messages pass through unchanged because the server understands
/// native Fancy extension types.
#[derive(Debug)]
pub struct NativeCodec;

impl FancyCodec for NativeCodec {
    fn encode(&self, msg: ControlMessage, _state: &ServerState) -> Option<ControlMessage> {
        Some(msg)
    }

    fn decode(&self, msg: ControlMessage) -> ControlMessage {
        msg
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
            Ok(decoded) => decoded,
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

    #[test]
    fn native_codec_passthrough_standard_message() {
        let codec = NativeCodec;
        let state = ServerState::new();
        let ping = ControlMessage::Ping(mumble_tcp::Ping {
            timestamp: Some(42),
            ..Default::default()
        });
        let encoded = codec.encode(ping.clone(), &state).unwrap();
        assert!(matches!(encoded, ControlMessage::Ping(_)));
    }

    #[test]
    fn native_codec_passthrough_fancy_message() {
        let codec = NativeCodec;
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
    fn native_codec_decode_passthrough() {
        let codec = NativeCodec;
        let msg = ControlMessage::Ping(mumble_tcp::Ping {
            timestamp: Some(99),
            ..Default::default()
        });
        let decoded = codec.decode(msg);
        assert!(matches!(decoded, ControlMessage::Ping(_)));
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
    fn legacy_codec_webrtc_broadcast_targets_channel_members() {
        let codec = LegacyCodec;
        let state = state_with_users();
        let signal = ControlMessage::WebRtcSignal(mumble_tcp::WebRtcSignal {
            target_session: Some(0),
            signal_type: Some(0),
            payload: Some("broadcast".into()),
            ..Default::default()
        });

        let encoded = codec.encode(signal, &state).unwrap();
        let ControlMessage::PluginDataTransmission(pd) = &encoded else {
            panic!("expected PluginDataTransmission");
        };

        // Session 2 is in channel 0 (same as us). Session 3 is in channel 1.
        assert_eq!(pd.receiver_sessions, vec![2]);
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
    fn legacy_codec_decode_passthrough_standard_message() {
        let codec = LegacyCodec;
        let msg = ControlMessage::Ping(mumble_tcp::Ping {
            timestamp: Some(42),
            ..Default::default()
        });
        let decoded = codec.decode(msg);
        assert!(matches!(decoded, ControlMessage::Ping(_)));
    }

    #[test]
    fn legacy_codec_decode_passthrough_normal_plugin_data() {
        let codec = LegacyCodec;
        let msg = ControlMessage::PluginDataTransmission(
            mumble_tcp::PluginDataTransmission {
                sender_session: Some(2),
                receiver_sessions: vec![1],
                data: Some(b"poll-json".to_vec()),
                data_id: Some("fancy-poll".into()),
            },
        );
        let decoded = codec.decode(msg);
        assert!(matches!(
            decoded,
            ControlMessage::PluginDataTransmission(_)
        ));
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
    fn legacy_decode_ignores_invalid_type_id() {
        let codec = LegacyCodec;
        let msg = ControlMessage::PluginDataTransmission(
            mumble_tcp::PluginDataTransmission {
                sender_session: Some(2),
                receiver_sessions: vec![1],
                data: Some(vec![0, 1, 2]),
                data_id: Some("fancy-native:not-a-number".into()),
            },
        );
        let decoded = codec.decode(msg);
        assert!(matches!(
            decoded,
            ControlMessage::PluginDataTransmission(_)
        ));
    }

    #[test]
    fn legacy_decode_handles_missing_payload() {
        let codec = LegacyCodec;
        let msg = ControlMessage::PluginDataTransmission(
            mumble_tcp::PluginDataTransmission {
                sender_session: Some(2),
                receiver_sessions: vec![1],
                data: None,
                data_id: Some("fancy-native:120".into()),
            },
        );
        let decoded = codec.decode(msg);
        assert!(matches!(
            decoded,
            ControlMessage::PluginDataTransmission(_)
        ));
    }
}
