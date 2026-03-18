use crate::proto::{mumble_tcp, mumble_udp};

/// Mumble TCP message type IDs as defined by the protocol.
/// Each variant maps to a protobuf message with a fixed numeric ID
/// used for framing on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum TcpMessageType {
    Version = 0,
    UdpTunnel = 1,
    Authenticate = 2,
    Ping = 3,
    Reject = 4,
    ServerSync = 5,
    ChannelRemove = 6,
    ChannelState = 7,
    UserRemove = 8,
    UserState = 9,
    BanList = 10,
    TextMessage = 11,
    PermissionDenied = 12,
    Acl = 13,
    QueryUsers = 14,
    CryptSetup = 15,
    ContextActionModify = 16,
    ContextAction = 17,
    UserList = 18,
    VoiceTarget = 19,
    PermissionQuery = 20,
    CodecVersion = 21,
    UserStats = 22,
    RequestBlob = 23,
    ServerConfig = 24,
    SuggestConfig = 25,
    PluginDataTransmission = 26,
    PchatMessage = 100,
    PchatFetch = 101,
    PchatFetchResponse = 102,
    PchatMessageDeliver = 103,
    PchatKeyAnnounce = 104,
    PchatKeyExchange = 105,
    PchatKeyRequest = 106,
    PchatAck = 107,
    PchatEpochCountersig = 108,
}

impl TryFrom<u16> for TcpMessageType {
    type Error = crate::error::Error;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Version),
            1 => Ok(Self::UdpTunnel),
            2 => Ok(Self::Authenticate),
            3 => Ok(Self::Ping),
            4 => Ok(Self::Reject),
            5 => Ok(Self::ServerSync),
            6 => Ok(Self::ChannelRemove),
            7 => Ok(Self::ChannelState),
            8 => Ok(Self::UserRemove),
            9 => Ok(Self::UserState),
            10 => Ok(Self::BanList),
            11 => Ok(Self::TextMessage),
            12 => Ok(Self::PermissionDenied),
            13 => Ok(Self::Acl),
            14 => Ok(Self::QueryUsers),
            15 => Ok(Self::CryptSetup),
            16 => Ok(Self::ContextActionModify),
            17 => Ok(Self::ContextAction),
            18 => Ok(Self::UserList),
            19 => Ok(Self::VoiceTarget),
            20 => Ok(Self::PermissionQuery),
            21 => Ok(Self::CodecVersion),
            22 => Ok(Self::UserStats),
            23 => Ok(Self::RequestBlob),
            24 => Ok(Self::ServerConfig),
            25 => Ok(Self::SuggestConfig),
            26 => Ok(Self::PluginDataTransmission),
            100 => Ok(Self::PchatMessage),
            101 => Ok(Self::PchatFetch),
            102 => Ok(Self::PchatFetchResponse),
            103 => Ok(Self::PchatMessageDeliver),
            104 => Ok(Self::PchatKeyAnnounce),
            105 => Ok(Self::PchatKeyExchange),
            106 => Ok(Self::PchatKeyRequest),
            107 => Ok(Self::PchatAck),
            108 => Ok(Self::PchatEpochCountersig),
            other => Err(crate::error::Error::UnknownMessageType(other)),
        }
    }
}

/// A decoded TCP control message received from (or to be sent to) the server.
#[derive(Debug, Clone)]
pub enum ControlMessage {
    Version(mumble_tcp::Version),
    Authenticate(mumble_tcp::Authenticate),
    Ping(mumble_tcp::Ping),
    Reject(mumble_tcp::Reject),
    ServerSync(mumble_tcp::ServerSync),
    ChannelRemove(mumble_tcp::ChannelRemove),
    ChannelState(mumble_tcp::ChannelState),
    UserRemove(mumble_tcp::UserRemove),
    UserState(mumble_tcp::UserState),
    BanList(mumble_tcp::BanList),
    TextMessage(mumble_tcp::TextMessage),
    PermissionDenied(mumble_tcp::PermissionDenied),
    Acl(mumble_tcp::Acl),
    QueryUsers(mumble_tcp::QueryUsers),
    CryptSetup(mumble_tcp::CryptSetup),
    ContextActionModify(mumble_tcp::ContextActionModify),
    ContextAction(mumble_tcp::ContextAction),
    UserList(mumble_tcp::UserList),
    VoiceTarget(mumble_tcp::VoiceTarget),
    PermissionQuery(mumble_tcp::PermissionQuery),
    CodecVersion(mumble_tcp::CodecVersion),
    UserStats(mumble_tcp::UserStats),
    RequestBlob(mumble_tcp::RequestBlob),
    ServerConfig(mumble_tcp::ServerConfig),
    SuggestConfig(mumble_tcp::SuggestConfig),
    PluginDataTransmission(mumble_tcp::PluginDataTransmission),
    PchatMessage(mumble_tcp::PchatMessage),
    PchatFetch(mumble_tcp::PchatFetch),
    PchatFetchResponse(mumble_tcp::PchatFetchResponse),
    PchatMessageDeliver(mumble_tcp::PchatMessageDeliver),
    PchatKeyAnnounce(mumble_tcp::PchatKeyAnnounce),
    PchatKeyExchange(mumble_tcp::PchatKeyExchange),
    PchatKeyRequest(mumble_tcp::PchatKeyRequest),
    PchatAck(mumble_tcp::PchatAck),
    PchatEpochCountersig(mumble_tcp::PchatEpochCountersig),
    /// UDP audio tunneled through TCP (fallback path).
    UdpTunnel(Vec<u8>),
}

/// A decoded UDP message - either audio or a UDP ping.
#[derive(Debug, Clone)]
pub enum UdpMessage {
    Audio(mumble_udp::Audio),
    Ping(mumble_udp::Ping),
}

/// Unified inbound message from either transport.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum ServerMessage {
    /// Control-plane message received over TCP.
    Control(ControlMessage),
    /// Real-time audio/ping received over UDP (or UDP-over-TCP tunnel).
    Udp(UdpMessage),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tcp_message_type_valid_conversions() {
        let expected = [
            (0u16, TcpMessageType::Version),
            (1, TcpMessageType::UdpTunnel),
            (2, TcpMessageType::Authenticate),
            (3, TcpMessageType::Ping),
            (4, TcpMessageType::Reject),
            (5, TcpMessageType::ServerSync),
            (6, TcpMessageType::ChannelRemove),
            (7, TcpMessageType::ChannelState),
            (8, TcpMessageType::UserRemove),
            (9, TcpMessageType::UserState),
            (10, TcpMessageType::BanList),
            (11, TcpMessageType::TextMessage),
            (12, TcpMessageType::PermissionDenied),
            (13, TcpMessageType::Acl),
            (14, TcpMessageType::QueryUsers),
            (15, TcpMessageType::CryptSetup),
            (16, TcpMessageType::ContextActionModify),
            (17, TcpMessageType::ContextAction),
            (18, TcpMessageType::UserList),
            (19, TcpMessageType::VoiceTarget),
            (20, TcpMessageType::PermissionQuery),
            (21, TcpMessageType::CodecVersion),
            (22, TcpMessageType::UserStats),
            (23, TcpMessageType::RequestBlob),
            (24, TcpMessageType::ServerConfig),
            (25, TcpMessageType::SuggestConfig),
            (26, TcpMessageType::PluginDataTransmission),
            (100, TcpMessageType::PchatMessage),
            (101, TcpMessageType::PchatFetch),
            (102, TcpMessageType::PchatFetchResponse),
            (103, TcpMessageType::PchatMessageDeliver),
            (104, TcpMessageType::PchatKeyAnnounce),
            (105, TcpMessageType::PchatKeyExchange),
            (106, TcpMessageType::PchatKeyRequest),
            (107, TcpMessageType::PchatAck),
            (108, TcpMessageType::PchatEpochCountersig),
        ];

        for (id, expected_type) in &expected {
            let result = TcpMessageType::try_from(*id).unwrap();
            assert_eq!(result, *expected_type, "mismatch for type id {id}");
        }
    }

    #[test]
    fn tcp_message_type_roundtrip() {
        // Core protocol IDs (contiguous 0..=26)
        for id in 0..=26u16 {
            let msg_type = TcpMessageType::try_from(id).unwrap();
            assert_eq!(msg_type as u16, id);
        }
        // Pchat IDs (100..=108)
        for id in 100..=108u16 {
            let msg_type = TcpMessageType::try_from(id).unwrap();
            assert_eq!(msg_type as u16, id);
        }
    }

    #[test]
    fn tcp_message_type_invalid_returns_error() {
        assert!(TcpMessageType::try_from(27u16).is_err());
        assert!(TcpMessageType::try_from(99u16).is_err());
        assert!(TcpMessageType::try_from(109u16).is_err());
        assert!(TcpMessageType::try_from(u16::MAX).is_err());
    }

    #[test]
    fn control_message_variants_are_constructable() {
        // Verify each variant can be constructed via Default
        let _ = ControlMessage::Version(mumble_tcp::Version::default());
        let _ = ControlMessage::Ping(mumble_tcp::Ping::default());
        let _ = ControlMessage::ServerSync(mumble_tcp::ServerSync::default());
        let _ = ControlMessage::UserState(mumble_tcp::UserState::default());
        let _ = ControlMessage::ChannelState(mumble_tcp::ChannelState::default());
        let _ = ControlMessage::TextMessage(mumble_tcp::TextMessage {
            message: "test".into(),
            ..Default::default()
        });
        let _ = ControlMessage::UdpTunnel(vec![1, 2, 3]);
    }

    #[test]
    fn udp_message_audio_variant() {
        let audio = mumble_udp::Audio {
            sender_session: 1,
            frame_number: 42,
            opus_data: vec![0xDE, 0xAD],
            ..Default::default()
        };
        let msg = UdpMessage::Audio(audio);
        match msg {
            UdpMessage::Audio(a) => {
                assert_eq!(a.sender_session, 1);
                assert_eq!(a.frame_number, 42);
            }
            _ => panic!("expected Audio variant"),
        }
    }

    #[test]
    fn udp_message_ping_variant() {
        let ping = mumble_udp::Ping {
            timestamp: 99,
            ..Default::default()
        };
        let msg = UdpMessage::Ping(ping);
        match msg {
            UdpMessage::Ping(p) => assert_eq!(p.timestamp, 99),
            _ => panic!("expected Ping variant"),
        }
    }

    #[test]
    fn server_message_wraps_control() {
        let ping = ControlMessage::Ping(mumble_tcp::Ping::default());
        let msg = ServerMessage::Control(ping);
        match msg {
            ServerMessage::Control(ControlMessage::Ping(_)) => {}
            _ => panic!("expected Control(Ping)"),
        }
    }

    #[test]
    fn server_message_wraps_udp() {
        let udp_ping = UdpMessage::Ping(mumble_udp::Ping::default());
        let msg = ServerMessage::Udp(udp_ping);
        match msg {
            ServerMessage::Udp(UdpMessage::Ping(_)) => {}
            _ => panic!("expected Udp(Ping)"),
        }
    }
}
