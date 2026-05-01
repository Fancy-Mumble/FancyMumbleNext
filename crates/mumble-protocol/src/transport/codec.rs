//! Mumble TCP packet framing: encode and decode the `[type:u16][length:u32][payload]` wire format.

use bytes::{Buf, BufMut, BytesMut};
use prost::Message;

use crate::error::{Error, Result};
use crate::message::{ControlMessage, TcpMessageType};
use crate::proto::mumble_tcp;

/// Maximum allowed payload size (8 MiB, generous upper bound).
const MAX_PAYLOAD_SIZE: u32 = 8 * 1024 * 1024;

/// Header size: 2 bytes type + 4 bytes length.
pub const HEADER_SIZE: usize = 6;

/// Encode a [`ControlMessage`] into a framed byte buffer ready for the wire.
pub fn encode(msg: &ControlMessage) -> Result<Vec<u8>> {
    let (type_id, payload) = serialize_control_message(msg)?;
    let len = payload.len() as u32;

    let mut buf = Vec::with_capacity(HEADER_SIZE + payload.len());
    buf.put_u16(type_id);
    buf.put_u32(len);
    buf.extend_from_slice(&payload);
    Ok(buf)
}

/// Try to decode one complete frame from `buf`.
///
/// Returns `Ok(Some(msg))` if a full frame was available (consumed from `buf`),
/// `Ok(None)` if more data is needed, or `Err` on protocol errors.
pub fn decode(buf: &mut BytesMut) -> Result<Option<ControlMessage>> {
    if buf.len() < HEADER_SIZE {
        return Ok(None);
    }

    let msg_type = u16::from_be_bytes([buf[0], buf[1]]);
    let payload_len = u32::from_be_bytes([buf[2], buf[3], buf[4], buf[5]]);

    if payload_len > MAX_PAYLOAD_SIZE {
        return Err(Error::InvalidState(format!(
            "payload too large: {payload_len} bytes"
        )));
    }

    let total = HEADER_SIZE + payload_len as usize;
    if buf.len() < total {
        return Ok(None);
    }

    buf.advance(HEADER_SIZE);
    let payload = buf.split_to(payload_len as usize);

    let msg = deserialize_control_message(msg_type, &payload)?;
    Ok(Some(msg))
}

// -- Serialization helpers ------------------------------------------

pub(crate) fn serialize_control_message(msg: &ControlMessage) -> Result<(u16, Vec<u8>)> {
    use ControlMessage::*;

    let type_id = msg.type_id();
    let payload = match msg {
        Version(m) => m.encode_to_vec(),
        Authenticate(m) => m.encode_to_vec(),
        Ping(m) => m.encode_to_vec(),
        Reject(m) => m.encode_to_vec(),
        ServerSync(m) => m.encode_to_vec(),
        ChannelRemove(m) => m.encode_to_vec(),
        ChannelState(m) => m.encode_to_vec(),
        UserRemove(m) => m.encode_to_vec(),
        UserState(m) => m.encode_to_vec(),
        BanList(m) => m.encode_to_vec(),
        TextMessage(m) => m.encode_to_vec(),
        PermissionDenied(m) => m.encode_to_vec(),
        Acl(m) => m.encode_to_vec(),
        QueryUsers(m) => m.encode_to_vec(),
        CryptSetup(m) => m.encode_to_vec(),
        ContextActionModify(m) => m.encode_to_vec(),
        ContextAction(m) => m.encode_to_vec(),
        UserList(m) => m.encode_to_vec(),
        VoiceTarget(m) => m.encode_to_vec(),
        PermissionQuery(m) => m.encode_to_vec(),
        CodecVersion(m) => m.encode_to_vec(),
        UserStats(m) => m.encode_to_vec(),
        RequestBlob(m) => m.encode_to_vec(),
        ServerConfig(m) => m.encode_to_vec(),
        SuggestConfig(m) => m.encode_to_vec(),
        PluginDataTransmission(m) => m.encode_to_vec(),
        PchatMessage(m) => m.encode_to_vec(),
        PchatFetch(m) => m.encode_to_vec(),
        PchatFetchResponse(m) => m.encode_to_vec(),
        PchatMessageDeliver(m) => m.encode_to_vec(),
        PchatKeyAnnounce(m) => m.encode_to_vec(),
        PchatKeyExchange(m) => m.encode_to_vec(),
        PchatKeyRequest(m) => m.encode_to_vec(),
        PchatAck(m) => m.encode_to_vec(),
        PchatEpochCountersig(m) => m.encode_to_vec(),
        PchatKeyHolderReport(m) => m.encode_to_vec(),
        PchatKeyHoldersQuery(m) => m.encode_to_vec(),
        PchatKeyHoldersList(m) => m.encode_to_vec(),
        PchatKeyChallenge(m) => m.encode_to_vec(),
        PchatKeyChallengeResponse(m) => m.encode_to_vec(),
        PchatKeyChallengeResult(m) => m.encode_to_vec(),
        PchatDeleteMessages(m) => m.encode_to_vec(),
        PchatOfflineQueueDrain(m) => m.encode_to_vec(),
        PchatReaction(m) => m.encode_to_vec(),
        PchatReactionDeliver(m) => m.encode_to_vec(),
        PchatReactionFetchResponse(m) => m.encode_to_vec(),
        WebRtcSignal(m) => m.encode_to_vec(),
        PchatSenderKeyDistribution(m) => m.encode_to_vec(),
        FancyPushRegister(m) => m.encode_to_vec(),
        FancyPushUpdate(m) => m.encode_to_vec(),
        FancyCustomReactionsConfig(m) => m.encode_to_vec(),
        FancySubscribePush(m) => m.encode_to_vec(),
        FancyReadReceipt(m) => m.encode_to_vec(),
        FancyReadReceiptDeliver(m) => m.encode_to_vec(),
        PchatPin(m) => m.encode_to_vec(),
        PchatPinDeliver(m) => m.encode_to_vec(),
        PchatPinFetchResponse(m) => m.encode_to_vec(),
        FancyTypingIndicator(m) => m.encode_to_vec(),
        FancyLinkPreviewRequest(m) => m.encode_to_vec(),
        FancyLinkPreviewResponse(m) => m.encode_to_vec(),
        FancyWatchSync(m) => m.encode_to_vec(),
        UdpTunnel(data) => data.clone(),
    };

    Ok((type_id, payload))
}

pub(crate) fn deserialize_control_message(type_id: u16, payload: &[u8]) -> Result<ControlMessage> {
    let msg_type = TcpMessageType::try_from(type_id)?;
    use TcpMessageType::*;

    let msg = match msg_type {
        Version => ControlMessage::Version(mumble_tcp::Version::decode(payload)?),
        UdpTunnel => ControlMessage::UdpTunnel(payload.to_vec()),
        Authenticate => ControlMessage::Authenticate(mumble_tcp::Authenticate::decode(payload)?),
        Ping => ControlMessage::Ping(mumble_tcp::Ping::decode(payload)?),
        Reject => ControlMessage::Reject(mumble_tcp::Reject::decode(payload)?),
        ServerSync => ControlMessage::ServerSync(mumble_tcp::ServerSync::decode(payload)?),
        ChannelRemove => ControlMessage::ChannelRemove(mumble_tcp::ChannelRemove::decode(payload)?),
        ChannelState => ControlMessage::ChannelState(mumble_tcp::ChannelState::decode(payload)?),
        UserRemove => ControlMessage::UserRemove(mumble_tcp::UserRemove::decode(payload)?),
        UserState => ControlMessage::UserState(mumble_tcp::UserState::decode(payload)?),
        BanList => ControlMessage::BanList(mumble_tcp::BanList::decode(payload)?),
        TextMessage => ControlMessage::TextMessage(mumble_tcp::TextMessage::decode(payload)?),
        PermissionDenied => ControlMessage::PermissionDenied(mumble_tcp::PermissionDenied::decode(payload)?),
        Acl => ControlMessage::Acl(mumble_tcp::Acl::decode(payload)?),
        QueryUsers => ControlMessage::QueryUsers(mumble_tcp::QueryUsers::decode(payload)?),
        CryptSetup => ControlMessage::CryptSetup(mumble_tcp::CryptSetup::decode(payload)?),
        ContextActionModify => ControlMessage::ContextActionModify(mumble_tcp::ContextActionModify::decode(payload)?),
        ContextAction => ControlMessage::ContextAction(mumble_tcp::ContextAction::decode(payload)?),
        UserList => ControlMessage::UserList(mumble_tcp::UserList::decode(payload)?),
        VoiceTarget => ControlMessage::VoiceTarget(mumble_tcp::VoiceTarget::decode(payload)?),
        PermissionQuery => ControlMessage::PermissionQuery(mumble_tcp::PermissionQuery::decode(payload)?),
        CodecVersion => ControlMessage::CodecVersion(mumble_tcp::CodecVersion::decode(payload)?),
        UserStats => ControlMessage::UserStats(mumble_tcp::UserStats::decode(payload)?),
        RequestBlob => ControlMessage::RequestBlob(mumble_tcp::RequestBlob::decode(payload)?),
        ServerConfig => ControlMessage::ServerConfig(mumble_tcp::ServerConfig::decode(payload)?),
        SuggestConfig => ControlMessage::SuggestConfig(mumble_tcp::SuggestConfig::decode(payload)?),
        PluginDataTransmission => ControlMessage::PluginDataTransmission(mumble_tcp::PluginDataTransmission::decode(payload)?),
        PchatMessage => ControlMessage::PchatMessage(mumble_tcp::PchatMessage::decode(payload)?),
        PchatFetch => ControlMessage::PchatFetch(mumble_tcp::PchatFetch::decode(payload)?),
        PchatFetchResponse => ControlMessage::PchatFetchResponse(mumble_tcp::PchatFetchResponse::decode(payload)?),
        PchatMessageDeliver => ControlMessage::PchatMessageDeliver(mumble_tcp::PchatMessageDeliver::decode(payload)?),
        PchatKeyAnnounce => ControlMessage::PchatKeyAnnounce(mumble_tcp::PchatKeyAnnounce::decode(payload)?),
        PchatKeyExchange => ControlMessage::PchatKeyExchange(mumble_tcp::PchatKeyExchange::decode(payload)?),
        PchatKeyRequest => ControlMessage::PchatKeyRequest(mumble_tcp::PchatKeyRequest::decode(payload)?),
        PchatAck => ControlMessage::PchatAck(mumble_tcp::PchatAck::decode(payload)?),
        PchatEpochCountersig => ControlMessage::PchatEpochCountersig(mumble_tcp::PchatEpochCountersig::decode(payload)?),
        PchatKeyHolderReport => ControlMessage::PchatKeyHolderReport(mumble_tcp::PchatKeyHolderReport::decode(payload)?),
        PchatKeyHoldersQuery => ControlMessage::PchatKeyHoldersQuery(mumble_tcp::PchatKeyHoldersQuery::decode(payload)?),
        PchatKeyHoldersList => ControlMessage::PchatKeyHoldersList(mumble_tcp::PchatKeyHoldersList::decode(payload)?),
        PchatKeyChallenge => ControlMessage::PchatKeyChallenge(mumble_tcp::PchatKeyChallenge::decode(payload)?),
        PchatKeyChallengeResponse => ControlMessage::PchatKeyChallengeResponse(mumble_tcp::PchatKeyChallengeResponse::decode(payload)?),
        PchatKeyChallengeResult => ControlMessage::PchatKeyChallengeResult(mumble_tcp::PchatKeyChallengeResult::decode(payload)?),
        PchatDeleteMessages => ControlMessage::PchatDeleteMessages(mumble_tcp::PchatDeleteMessages::decode(payload)?),
        PchatOfflineQueueDrain => ControlMessage::PchatOfflineQueueDrain(mumble_tcp::PchatOfflineQueueDrain::decode(payload)?),
        PchatReaction => ControlMessage::PchatReaction(mumble_tcp::PchatReaction::decode(payload)?),
        PchatReactionDeliver => ControlMessage::PchatReactionDeliver(mumble_tcp::PchatReactionDeliver::decode(payload)?),
        PchatReactionFetchResponse => ControlMessage::PchatReactionFetchResponse(mumble_tcp::PchatReactionFetchResponse::decode(payload)?),
        WebRtcSignal => ControlMessage::WebRtcSignal(mumble_tcp::WebRtcSignal::decode(payload)?),
        PchatSenderKeyDistribution => ControlMessage::PchatSenderKeyDistribution(mumble_tcp::PchatSenderKeyDistribution::decode(payload)?),
        FancyPushRegister => ControlMessage::FancyPushRegister(mumble_tcp::FancyPushRegister::decode(payload)?),
        FancyPushUpdate => ControlMessage::FancyPushUpdate(mumble_tcp::FancyPushUpdate::decode(payload)?),
        FancyCustomReactionsConfig => ControlMessage::FancyCustomReactionsConfig(mumble_tcp::FancyCustomReactionsConfig::decode(payload)?),
        FancySubscribePush => ControlMessage::FancySubscribePush(mumble_tcp::FancySubscribePush::decode(payload)?),
        FancyReadReceipt => ControlMessage::FancyReadReceipt(mumble_tcp::FancyReadReceipt::decode(payload)?),
        FancyReadReceiptDeliver => ControlMessage::FancyReadReceiptDeliver(mumble_tcp::FancyReadReceiptDeliver::decode(payload)?),
        PchatPin => ControlMessage::PchatPin(mumble_tcp::PchatPin::decode(payload)?),
        PchatPinDeliver => ControlMessage::PchatPinDeliver(mumble_tcp::PchatPinDeliver::decode(payload)?),
        PchatPinFetchResponse => ControlMessage::PchatPinFetchResponse(mumble_tcp::PchatPinFetchResponse::decode(payload)?),
        FancyTypingIndicator => ControlMessage::FancyTypingIndicator(mumble_tcp::FancyTypingIndicator::decode(payload)?),
        FancyLinkPreviewRequest => ControlMessage::FancyLinkPreviewRequest(mumble_tcp::FancyLinkPreviewRequest::decode(payload)?),
        FancyLinkPreviewResponse => ControlMessage::FancyLinkPreviewResponse(mumble_tcp::FancyLinkPreviewResponse::decode(payload)?),
        FancyWatchSync => ControlMessage::FancyWatchSync(mumble_tcp::FancyWatchSync::decode(payload)?),
    };
    Ok(msg)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
    use super::*;

    #[test]
    fn roundtrip_ping() -> Result<()> {
        let ping = mumble_tcp::Ping {
            timestamp: Some(42),
            ..Default::default()
        };
        let msg = ControlMessage::Ping(ping);
        let encoded = encode(&msg)?;
        let mut buf = BytesMut::from(&encoded[..]);
        let decoded = decode(&mut buf)?
            .ok_or(Error::InvalidState(
                "expected complete frame".into(),
            ))?;

        match decoded {
            ControlMessage::Ping(p) => assert_eq!(p.timestamp, Some(42)),
            other => panic!("unexpected message: {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn partial_frame_returns_none() -> Result<()> {
        let mut buf = BytesMut::from(&[0u8; 4][..]);
        assert!(decode(&mut buf)?.is_none());
        Ok(())
    }

    #[test]
    fn roundtrip_version() -> Result<()> {
        let version = mumble_tcp::Version {
            version_v2: Some(0x0001_0005_0000_0000),
            release: Some("Test 1.5.0".into()),
            os: Some("Windows".into()),
            os_version: Some("10".into()),
            ..Default::default()
        };
        let msg = ControlMessage::Version(version);
        let encoded = encode(&msg)?;
        let mut buf = BytesMut::from(&encoded[..]);
        let decoded = decode(&mut buf)?.unwrap();

        match decoded {
            ControlMessage::Version(v) => {
                assert_eq!(v.release.as_deref(), Some("Test 1.5.0"));
                assert_eq!(v.os.as_deref(), Some("Windows"));
            }
            other => panic!("expected Version, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn roundtrip_text_message() -> Result<()> {
        let msg = ControlMessage::TextMessage(mumble_tcp::TextMessage {
            message: "Hello, world!".into(),
            channel_id: vec![0],
            ..Default::default()
        });
        let encoded = encode(&msg)?;
        let mut buf = BytesMut::from(&encoded[..]);
        let decoded = decode(&mut buf)?.unwrap();

        match decoded {
            ControlMessage::TextMessage(tm) => {
                assert_eq!(tm.message, "Hello, world!");
                assert_eq!(tm.channel_id, vec![0]);
            }
            other => panic!("expected TextMessage, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn roundtrip_user_state() -> Result<()> {
        let msg = ControlMessage::UserState(mumble_tcp::UserState {
            session: Some(42),
            name: Some("TestUser".into()),
            channel_id: Some(0),
            self_mute: Some(true),
            ..Default::default()
        });
        let encoded = encode(&msg)?;
        let mut buf = BytesMut::from(&encoded[..]);
        let decoded = decode(&mut buf)?.unwrap();

        match decoded {
            ControlMessage::UserState(us) => {
                assert_eq!(us.session, Some(42));
                assert_eq!(us.name.as_deref(), Some("TestUser"));
                assert_eq!(us.self_mute, Some(true));
            }
            other => panic!("expected UserState, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn roundtrip_server_sync() -> Result<()> {
        let msg = ControlMessage::ServerSync(mumble_tcp::ServerSync {
            session: Some(7),
            max_bandwidth: Some(72000),
            welcome_text: Some("Welcome!".into()),
            ..Default::default()
        });
        let encoded = encode(&msg)?;
        let mut buf = BytesMut::from(&encoded[..]);
        let decoded = decode(&mut buf)?.unwrap();

        match decoded {
            ControlMessage::ServerSync(ss) => {
                assert_eq!(ss.session, Some(7));
                assert_eq!(ss.max_bandwidth, Some(72000));
                assert_eq!(ss.welcome_text.as_deref(), Some("Welcome!"));
            }
            other => panic!("expected ServerSync, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn roundtrip_channel_state() -> Result<()> {
        let msg = ControlMessage::ChannelState(mumble_tcp::ChannelState {
            channel_id: Some(1),
            name: Some("Lobby".into()),
            parent: Some(0),
            temporary: Some(true),
            ..Default::default()
        });
        let encoded = encode(&msg)?;
        let mut buf = BytesMut::from(&encoded[..]);
        let decoded = decode(&mut buf)?.unwrap();

        match decoded {
            ControlMessage::ChannelState(cs) => {
                assert_eq!(cs.channel_id, Some(1));
                assert_eq!(cs.name.as_deref(), Some("Lobby"));
                assert_eq!(cs.parent, Some(0));
                assert!(cs.temporary.unwrap());
            }
            other => panic!("expected ChannelState, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn roundtrip_udp_tunnel() -> Result<()> {
        let data = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let msg = ControlMessage::UdpTunnel(data.clone());
        let encoded = encode(&msg)?;
        let mut buf = BytesMut::from(&encoded[..]);
        let decoded = decode(&mut buf)?.unwrap();

        match decoded {
            ControlMessage::UdpTunnel(d) => assert_eq!(d, data),
            other => panic!("expected UdpTunnel, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn roundtrip_reject() -> Result<()> {
        let msg = ControlMessage::Reject(mumble_tcp::Reject {
            r#type: Some(mumble_tcp::reject::RejectType::WrongUserPw as i32),
            reason: Some("Bad password".into()),
        });
        let encoded = encode(&msg)?;
        let mut buf = BytesMut::from(&encoded[..]);
        let decoded = decode(&mut buf)?.unwrap();

        match decoded {
            ControlMessage::Reject(r) => {
                assert_eq!(
                    r.r#type,
                    Some(mumble_tcp::reject::RejectType::WrongUserPw as i32)
                );
                assert_eq!(r.reason.as_deref(), Some("Bad password"));
            }
            other => panic!("expected Reject, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn empty_buffer_returns_none() -> Result<()> {
        let mut buf = BytesMut::new();
        assert!(decode(&mut buf)?.is_none());
        Ok(())
    }

    #[test]
    fn header_only_no_payload_returns_none() -> Result<()> {
        // Header says payload is 100 bytes but buffer only has the header
        let mut buf = BytesMut::new();
        buf.put_u16(3); // Ping type
        buf.put_u32(100); // payload_len = 100
        // No payload bytes
        assert!(decode(&mut buf)?.is_none());
        Ok(())
    }

    #[test]
    fn payload_too_large_returns_error() {
        let mut buf = BytesMut::new();
        buf.put_u16(3); // Ping type
        buf.put_u32(MAX_PAYLOAD_SIZE + 1); // exceeds limit
        let result = decode(&mut buf);
        assert!(result.is_err());
    }

    #[test]
    fn multiple_frames_in_buffer() -> Result<()> {
        let msg1 = ControlMessage::Ping(mumble_tcp::Ping {
            timestamp: Some(1),
            ..Default::default()
        });
        let msg2 = ControlMessage::Ping(mumble_tcp::Ping {
            timestamp: Some(2),
            ..Default::default()
        });

        let enc1 = encode(&msg1)?;
        let enc2 = encode(&msg2)?;

        let mut buf = BytesMut::new();
        buf.extend_from_slice(&enc1);
        buf.extend_from_slice(&enc2);

        let decoded1 = decode(&mut buf)?.unwrap();
        let decoded2 = decode(&mut buf)?.unwrap();
        assert!(decode(&mut buf)?.is_none()); // no more

        match decoded1 {
            ControlMessage::Ping(p) => assert_eq!(p.timestamp, Some(1)),
            _ => panic!("expected Ping"),
        }
        match decoded2 {
            ControlMessage::Ping(p) => assert_eq!(p.timestamp, Some(2)),
            _ => panic!("expected Ping"),
        }
        Ok(())
    }

    #[test]
    fn encode_header_format() -> Result<()> {
        let msg = ControlMessage::Ping(mumble_tcp::Ping::default());
        let encoded = encode(&msg)?;

        // First 2 bytes = type ID (Ping = 3)
        assert_eq!(encoded[0], 0);
        assert_eq!(encoded[1], 3);

        // Next 4 bytes = payload length
        let payload_len =
            u32::from_be_bytes([encoded[2], encoded[3], encoded[4], encoded[5]]);
        assert_eq!(payload_len as usize, encoded.len() - HEADER_SIZE);
        Ok(())
    }

    #[test]
    fn roundtrip_server_config() -> Result<()> {
        let msg = ControlMessage::ServerConfig(mumble_tcp::ServerConfig {
            max_bandwidth: Some(128000),
            message_length: Some(5000),
            image_message_length: Some(131072),
            allow_html: Some(true),
            ..Default::default()
        });
        let encoded = encode(&msg)?;
        let mut buf = BytesMut::from(&encoded[..]);
        let decoded = decode(&mut buf)?.unwrap();
        match decoded {
            ControlMessage::ServerConfig(sc) => {
                assert_eq!(sc.max_bandwidth, Some(128000));
                assert_eq!(sc.image_message_length, Some(131072));
            }
            other => panic!("expected ServerConfig, got {other:?}"),
        }
        Ok(())
    }

    // -- PluginDataTransmission codec tests ------------------------

    #[test]
    fn roundtrip_plugin_data_transmission() -> Result<()> {
        let msg = ControlMessage::PluginDataTransmission(mumble_tcp::PluginDataTransmission {
            sender_session: Some(42),
            receiver_sessions: vec![10, 20, 30],
            data: Some(b"hello plugin".to_vec()),
            data_id: Some("fancy-poll".into()),
        });
        let encoded = encode(&msg)?;
        let mut buf = BytesMut::from(&encoded[..]);
        let decoded = decode(&mut buf)?.unwrap();

        match decoded {
            ControlMessage::PluginDataTransmission(pd) => {
                assert_eq!(pd.sender_session, Some(42));
                assert_eq!(pd.receiver_sessions, vec![10, 20, 30]);
                assert_eq!(pd.data.as_deref(), Some(b"hello plugin".as_slice()));
                assert_eq!(pd.data_id.as_deref(), Some("fancy-poll"));
            }
            other => panic!("expected PluginDataTransmission, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn roundtrip_plugin_data_empty_receivers() -> Result<()> {
        let msg = ControlMessage::PluginDataTransmission(mumble_tcp::PluginDataTransmission {
            sender_session: None,
            receiver_sessions: vec![],
            data: Some(b"{}".to_vec()),
            data_id: Some("fancy-poll".into()),
        });
        let encoded = encode(&msg)?;
        let mut buf = BytesMut::from(&encoded[..]);
        let decoded = decode(&mut buf)?.unwrap();

        match decoded {
            ControlMessage::PluginDataTransmission(pd) => {
                assert!(pd.sender_session.is_none());
                assert!(pd.receiver_sessions.is_empty());
            }
            other => panic!("expected PluginDataTransmission, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn roundtrip_plugin_data_large_json_payload() -> Result<()> {
        let json = r#"{"type":"poll","id":"550e8400-e29b-41d4-a716-446655440000","question":"What is your favourite language?","options":["Rust","TypeScript","Python","Go"],"multiple":false,"creator":42,"creatorName":"Alice","createdAt":"2025-01-01T00:00:00Z"}"#;
        let msg = ControlMessage::PluginDataTransmission(mumble_tcp::PluginDataTransmission {
            sender_session: Some(42),
            receiver_sessions: vec![10],
            data: Some(json.as_bytes().to_vec()),
            data_id: Some("fancy-poll".into()),
        });
        let encoded = encode(&msg)?;
        let mut buf = BytesMut::from(&encoded[..]);
        let decoded = decode(&mut buf)?.unwrap();

        match decoded {
            ControlMessage::PluginDataTransmission(pd) => {
                let payload = std::str::from_utf8(pd.data.as_deref().unwrap()).unwrap();
                assert_eq!(payload, json);
            }
            other => panic!("expected PluginDataTransmission, got {other:?}"),
        }
        Ok(())
    }

    // -- PchatKeyHolder* codec tests ----------------------------------

    #[test]
    fn roundtrip_pchat_key_holder_report() -> Result<()> {
        let msg = ControlMessage::PchatKeyHolderReport(mumble_tcp::PchatKeyHolderReport {
            channel_id: Some(42),
            cert_hash: Some("abcdef0123456789".into()),
            takeover_mode: None,
        });
        let encoded = encode(&msg)?;
        let mut buf = BytesMut::from(&encoded[..]);
        let decoded = decode(&mut buf)?.unwrap();

        match decoded {
            ControlMessage::PchatKeyHolderReport(r) => {
                assert_eq!(r.channel_id, Some(42));
                assert_eq!(r.cert_hash.as_deref(), Some("abcdef0123456789"));
            }
            other => panic!("expected PchatKeyHolderReport, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn roundtrip_pchat_key_holders_query() -> Result<()> {
        let msg = ControlMessage::PchatKeyHoldersQuery(mumble_tcp::PchatKeyHoldersQuery {
            channel_id: Some(7),
        });
        let encoded = encode(&msg)?;
        let mut buf = BytesMut::from(&encoded[..]);
        let decoded = decode(&mut buf)?.unwrap();

        match decoded {
            ControlMessage::PchatKeyHoldersQuery(q) => {
                assert_eq!(q.channel_id, Some(7));
            }
            other => panic!("expected PchatKeyHoldersQuery, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn roundtrip_pchat_key_holders_list() -> Result<()> {
        use mumble_tcp::pchat_key_holders_list::Entry;

        let msg = ControlMessage::PchatKeyHoldersList(mumble_tcp::PchatKeyHoldersList {
            channel_id: Some(3),
            holders: vec![
                Entry {
                    cert_hash: Some("hash_alice".into()),
                    name: Some("Alice".into()),
                },
                Entry {
                    cert_hash: Some("hash_bob".into()),
                    name: Some("Bob".into()),
                },
            ],
        });
        let encoded = encode(&msg)?;
        let mut buf = BytesMut::from(&encoded[..]);
        let decoded = decode(&mut buf)?.unwrap();

        match decoded {
            ControlMessage::PchatKeyHoldersList(l) => {
                assert_eq!(l.channel_id, Some(3));
                assert_eq!(l.holders.len(), 2);
                assert_eq!(l.holders[0].cert_hash.as_deref(), Some("hash_alice"));
                assert_eq!(l.holders[0].name.as_deref(), Some("Alice"));
                assert_eq!(l.holders[1].cert_hash.as_deref(), Some("hash_bob"));
                assert_eq!(l.holders[1].name.as_deref(), Some("Bob"));
            }
            other => panic!("expected PchatKeyHoldersList, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn roundtrip_pchat_key_holders_list_empty() -> Result<()> {
        let msg = ControlMessage::PchatKeyHoldersList(mumble_tcp::PchatKeyHoldersList {
            channel_id: Some(0),
            holders: vec![],
        });
        let encoded = encode(&msg)?;
        let mut buf = BytesMut::from(&encoded[..]);
        let decoded = decode(&mut buf)?.unwrap();

        match decoded {
            ControlMessage::PchatKeyHoldersList(l) => {
                assert_eq!(l.channel_id, Some(0));
                assert!(l.holders.is_empty());
            }
            other => panic!("expected PchatKeyHoldersList, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn pchat_key_holder_report_wire_type_id() -> Result<()> {
        let msg = ControlMessage::PchatKeyHolderReport(mumble_tcp::PchatKeyHolderReport {
            channel_id: Some(1),
            cert_hash: Some("abc".into()),
            takeover_mode: None,
        });
        let encoded = encode(&msg)?;
        // First 2 bytes are the type ID (big-endian u16).
        let type_id = u16::from_be_bytes([encoded[0], encoded[1]]);
        assert_eq!(type_id, 109, "PchatKeyHolderReport must be wire type 109");
        Ok(())
    }

    #[test]
    fn pchat_key_holders_query_wire_type_id() -> Result<()> {
        let msg = ControlMessage::PchatKeyHoldersQuery(mumble_tcp::PchatKeyHoldersQuery {
            channel_id: Some(1),
        });
        let encoded = encode(&msg)?;
        let type_id = u16::from_be_bytes([encoded[0], encoded[1]]);
        assert_eq!(type_id, 110, "PchatKeyHoldersQuery must be wire type 110");
        Ok(())
    }

    #[test]
    fn pchat_key_holders_list_wire_type_id() -> Result<()> {
        let msg = ControlMessage::PchatKeyHoldersList(mumble_tcp::PchatKeyHoldersList {
            channel_id: Some(1),
            holders: vec![],
        });
        let encoded = encode(&msg)?;
        let type_id = u16::from_be_bytes([encoded[0], encoded[1]]);
        assert_eq!(type_id, 111, "PchatKeyHoldersList must be wire type 111");
        Ok(())
    }

    // -- PchatKeyChallenge* codec tests -------------------------------

    #[test]
    fn roundtrip_pchat_key_challenge() -> Result<()> {
        let challenge = vec![0xAA; 32];
        let msg = ControlMessage::PchatKeyChallenge(mumble_tcp::PchatKeyChallenge {
            channel_id: Some(5),
            challenge: Some(challenge.clone()),
        });
        let encoded = encode(&msg)?;
        let mut buf = BytesMut::from(&encoded[..]);
        let decoded = decode(&mut buf)?.unwrap();

        match decoded {
            ControlMessage::PchatKeyChallenge(c) => {
                assert_eq!(c.channel_id, Some(5));
                assert_eq!(c.challenge.as_deref(), Some(challenge.as_slice()));
            }
            other => panic!("expected PchatKeyChallenge, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn roundtrip_pchat_key_challenge_response() -> Result<()> {
        let proof = vec![0xBB; 32];
        let msg = ControlMessage::PchatKeyChallengeResponse(mumble_tcp::PchatKeyChallengeResponse {
            channel_id: Some(5),
            proof: Some(proof.clone()),
        });
        let encoded = encode(&msg)?;
        let mut buf = BytesMut::from(&encoded[..]);
        let decoded = decode(&mut buf)?.unwrap();

        match decoded {
            ControlMessage::PchatKeyChallengeResponse(r) => {
                assert_eq!(r.channel_id, Some(5));
                assert_eq!(r.proof.as_deref(), Some(proof.as_slice()));
            }
            other => panic!("expected PchatKeyChallengeResponse, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn roundtrip_pchat_key_challenge_result() -> Result<()> {
        let msg = ControlMessage::PchatKeyChallengeResult(mumble_tcp::PchatKeyChallengeResult {
            channel_id: Some(5),
            passed: Some(true),
        });
        let encoded = encode(&msg)?;
        let mut buf = BytesMut::from(&encoded[..]);
        let decoded = decode(&mut buf)?.unwrap();

        match decoded {
            ControlMessage::PchatKeyChallengeResult(r) => {
                assert_eq!(r.channel_id, Some(5));
                assert_eq!(r.passed, Some(true));
            }
            other => panic!("expected PchatKeyChallengeResult, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn pchat_key_challenge_wire_type_id() -> Result<()> {
        let msg = ControlMessage::PchatKeyChallenge(mumble_tcp::PchatKeyChallenge {
            channel_id: Some(1),
            challenge: Some(vec![0; 32]),
        });
        let encoded = encode(&msg)?;
        let type_id = u16::from_be_bytes([encoded[0], encoded[1]]);
        assert_eq!(type_id, 112, "PchatKeyChallenge must be wire type 112");
        Ok(())
    }

    #[test]
    fn pchat_key_challenge_response_wire_type_id() -> Result<()> {
        let msg = ControlMessage::PchatKeyChallengeResponse(mumble_tcp::PchatKeyChallengeResponse {
            channel_id: Some(1),
            proof: Some(vec![0; 32]),
        });
        let encoded = encode(&msg)?;
        let type_id = u16::from_be_bytes([encoded[0], encoded[1]]);
        assert_eq!(type_id, 113, "PchatKeyChallengeResponse must be wire type 113");
        Ok(())
    }

    #[test]
    fn pchat_key_challenge_result_wire_type_id() -> Result<()> {
        let msg = ControlMessage::PchatKeyChallengeResult(mumble_tcp::PchatKeyChallengeResult {
            channel_id: Some(1),
            passed: Some(false),
        });
        let encoded = encode(&msg)?;
        let type_id = u16::from_be_bytes([encoded[0], encoded[1]]);
        assert_eq!(type_id, 114, "PchatKeyChallengeResult must be wire type 114");
        Ok(())
    }

    #[test]
    fn roundtrip_pchat_ack_with_channel_id() -> Result<()> {
        let ack = mumble_tcp::PchatAck {
            message_ids: vec!["msg-1".into(), "msg-2".into()],
            status: Some(mumble_tcp::PchatAckStatus::PchatAckStored as i32),
            reason: None,
            channel_id: Some(42),
        };
        let msg = ControlMessage::PchatAck(ack);
        let encoded = encode(&msg)?;
        let mut buf = BytesMut::from(&encoded[..]);
        let decoded = decode(&mut buf)?.unwrap();

        match decoded {
            ControlMessage::PchatAck(a) => {
                assert_eq!(a.message_ids, vec!["msg-1", "msg-2"]);
                assert_eq!(a.status, Some(mumble_tcp::PchatAckStatus::PchatAckStored as i32));
                assert_eq!(a.channel_id, Some(42));
            }
            other => panic!("expected PchatAck, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn roundtrip_pchat_offline_queue_drain() -> Result<()> {
        let drain = mumble_tcp::PchatOfflineQueueDrain {
            channel_id: Some(7),
            messages: vec![
                mumble_tcp::PchatMessageDeliver {
                    message_id: Some("offline-1".into()),
                    channel_id: Some(7),
                    sender_hash: Some("abc123".into()),
                    timestamp: Some(1_700_000_000),
                    envelope: Some(b"encrypted-payload".to_vec()),
                    protocol: Some(mumble_tcp::PchatProtocol::SignalV1 as i32),
                    replaces_id: None,
                },
            ],
            distributions: vec![],
        };
        let msg = ControlMessage::PchatOfflineQueueDrain(drain);
        let encoded = encode(&msg)?;

        // Wire type ID must be 116.
        let type_id = u16::from_be_bytes([encoded[0], encoded[1]]);
        assert_eq!(type_id, 116, "PchatOfflineQueueDrain must be wire type 116");

        let mut buf = BytesMut::from(&encoded[..]);
        let decoded = decode(&mut buf)?.unwrap();

        match decoded {
            ControlMessage::PchatOfflineQueueDrain(d) => {
                assert_eq!(d.channel_id, Some(7));
                assert_eq!(d.messages.len(), 1);
                assert_eq!(d.messages[0].message_id.as_deref(), Some("offline-1"));
                assert_eq!(d.messages[0].sender_hash.as_deref(), Some("abc123"));
                assert_eq!(d.messages[0].timestamp, Some(1_700_000_000));
                assert_eq!(d.messages[0].envelope.as_deref(), Some(b"encrypted-payload".as_ref()));
            }
            other => panic!("expected PchatOfflineQueueDrain, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn roundtrip_pchat_sender_key_distribution() -> Result<()> {
        let skd = mumble_tcp::PchatSenderKeyDistribution {
            channel_id: Some(10),
            sender_hash: Some("sender_abc".into()),
            distribution: Some(b"skdm-bytes-here".to_vec()),
        };
        let msg = ControlMessage::PchatSenderKeyDistribution(skd);
        let encoded = encode(&msg)?;

        let type_id = u16::from_be_bytes([encoded[0], encoded[1]]);
        assert_eq!(type_id, 121, "PchatSenderKeyDistribution must be wire type 121");

        let mut buf = BytesMut::from(&encoded[..]);
        let decoded = decode(&mut buf)?.unwrap();

        match decoded {
            ControlMessage::PchatSenderKeyDistribution(d) => {
                assert_eq!(d.channel_id, Some(10));
                assert_eq!(d.sender_hash.as_deref(), Some("sender_abc"));
                assert_eq!(d.distribution.as_deref(), Some(b"skdm-bytes-here".as_ref()));
            }
            other => panic!("expected PchatSenderKeyDistribution, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn roundtrip_fancy_typing_indicator() -> Result<()> {
        let msg = ControlMessage::FancyTypingIndicator(mumble_tcp::FancyTypingIndicator {
            actor: Some(7),
            channel_id: Some(42),
        });
        let encoded = encode(&msg)?;

        let type_id = u16::from_be_bytes([encoded[0], encoded[1]]);
        assert_eq!(type_id, 131, "FancyTypingIndicator must be wire type 131");

        let mut buf = BytesMut::from(&encoded[..]);
        let decoded = decode(&mut buf)?.unwrap();

        match decoded {
            ControlMessage::FancyTypingIndicator(m) => {
                assert_eq!(m.actor, Some(7));
                assert_eq!(m.channel_id, Some(42));
            }
            other => panic!("expected FancyTypingIndicator, got {other:?}"),
        }
        Ok(())
    }
}
