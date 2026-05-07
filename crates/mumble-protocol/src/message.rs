//! Mumble message type enumerations and decoded message wrappers.
//!
//! [`TcpMessageType`] maps numeric wire IDs to their protobuf types.
//! [`ControlMessage`] and [`UdpMessage`] carry fully decoded payloads.
//! [`ServerMessage`] is the unified inbound type used by the work queue.
use crate::proto::{mumble_tcp, mumble_udp};

/// Mumble TCP message type IDs as defined by the protocol.
/// Each variant maps to a protobuf message with a fixed numeric ID
/// used for framing on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum TcpMessageType {
    /// Protocol version negotiation message.
    Version = 0,
    /// UDP audio tunnelled over TCP (fallback).
    UdpTunnel = 1,
    /// Client authentication handshake.
    Authenticate = 2,
    /// Keep-alive ping.
    Ping = 3,
    /// Server rejects the connection.
    Reject = 4,
    /// Server acknowledges successful authentication.
    ServerSync = 5,
    /// Server notifies clients that a channel was removed.
    ChannelRemove = 6,
    /// Channel metadata update.
    ChannelState = 7,
    /// A user disconnected from the server.
    UserRemove = 8,
    /// User state (mute, deafen, channel, etc.) changed.
    UserState = 9,
    /// Ban list from the server.
    BanList = 10,
    /// A text chat message.
    TextMessage = 11,
    /// Server denies an action.
    PermissionDenied = 12,
    /// Access-control list for a channel.
    Acl = 13,
    /// Map of registered users (session -> username).
    QueryUsers = 14,
    /// Encryption key setup for the OCB-encrypted UDP path.
    CryptSetup = 15,
    /// Adds/removes a contextual action button in the Mumble UI.
    ContextActionModify = 16,
    /// A contextual action was triggered by the user.
    ContextAction = 17,
    /// Registered user list.
    UserList = 18,
    /// Configure a voice target for whisper/shout.
    VoiceTarget = 19,
    /// Query or response for channel permissions.
    PermissionQuery = 20,
    /// Negotiated audio codec version.
    CodecVersion = 21,
    /// Detailed statistics for a connected user.
    UserStats = 22,
    /// Request the server to send a large blob (avatar, comment, etc.).
    RequestBlob = 23,
    /// Global server configuration values (max bandwidth, limits, etc.).
    ServerConfig = 24,
    /// Server hints that the client configuration is outdated.
    SuggestConfig = 25,
    /// Plugin data relay between clients (used for polls, pchat, etc.).
    PluginDataTransmission = 26,
    /// Fancy Mumble: encrypted persistent chat message.
    PchatMessage = 100,
    /// Fancy Mumble: fetch stored messages from the server.
    PchatFetch = 101,
    /// Fancy Mumble: server response to a fetch request.
    PchatFetchResponse = 102,
    /// Fancy Mumble: deliver a stored message to the client.
    PchatMessageDeliver = 103,
    /// Fancy Mumble: client announces its E2EE identity keys.
    PchatKeyAnnounce = 104,
    /// Fancy Mumble: peer-to-peer encrypted key exchange.
    PchatKeyExchange = 105,
    /// Fancy Mumble: server requests a key for a new member.
    PchatKeyRequest = 106,
    /// Fancy Mumble: server acknowledgement of a stored message.
    PchatAck = 107,
    /// Fancy Mumble: custodian countersignature for an epoch transition.
    PchatEpochCountersig = 108,
    /// Fancy Mumble: report that a peer holds the channel key.
    PchatKeyHolderReport = 109,
    /// Fancy Mumble: query the server for the list of key holders.
    PchatKeyHoldersQuery = 110,
    /// Fancy Mumble: server response with the key-holder list.
    PchatKeyHoldersList = 111,
    /// Fancy Mumble: server challenge to prove key possession.
    PchatKeyChallenge = 112,
    /// Fancy Mumble: client response to a key-possession challenge.
    PchatKeyChallengeResponse = 113,
    /// Fancy Mumble: server verdict on a key-possession challenge.
    PchatKeyChallengeResult = 114,
    /// Fancy Mumble: delete persisted messages (by ID, time range, or sender).
    PchatDeleteMessages = 115,
    /// Fancy Mumble: server drains offline message queue to a reconnected client.
    PchatOfflineQueueDrain = 116,
    /// Fancy Mumble: client sends a reaction (add/remove) on a persistent message.
    PchatReaction = 117,
    /// Fancy Mumble: server broadcasts a reaction update to channel members.
    PchatReactionDeliver = 118,
    /// Fancy Mumble: server response with reactions for requested messages.
    PchatReactionFetchResponse = 119,
    /// Fancy Mumble: WebRTC screen-sharing signaling relay.
    WebRtcSignal = 120,
    /// Fancy Mumble: Signal sender key distribution (replaces `PluginData` relay).
    PchatSenderKeyDistribution = 121,
    /// Fancy Mumble: FCM push notification registration.
    FancyPushRegister = 122,
    /// Fancy Mumble: push notification mute preferences update.
    FancyPushUpdate = 123,
    /// Fancy Mumble: server broadcasts custom reaction/emoji config.
    FancyCustomReactionsConfig = 124,
    /// Fancy Mumble: live push subscribe for connected clients.
    FancySubscribePush = 125,
    /// Fancy Mumble: client sends a read receipt watermark.
    FancyReadReceipt = 126,
    /// Fancy Mumble: server broadcasts read receipt state.
    FancyReadReceiptDeliver = 127,
    /// Fancy Mumble: client sends a pin/unpin request for a persistent message.
    PchatPin = 128,
    /// Fancy Mumble: server broadcasts a pin state change.
    PchatPinDeliver = 129,
    /// Fancy Mumble: server response with pinned messages for a channel.
    PchatPinFetchResponse = 130,
    /// Fancy Mumble: typing indicator broadcast.
    FancyTypingIndicator = 131,
    /// Fancy Mumble: client requests link previews from the server.
    FancyLinkPreviewRequest = 132,
    /// Fancy Mumble: server responds with link embed metadata.
    FancyLinkPreviewResponse = 133,
    /// Fancy Mumble: synchronised watch-together control event.
    FancyWatchSync = 134,
    /// Fancy Mumble: drawing stroke overlay for screen-share collaboration.
    FancyDrawStroke = 135,
}

/// Generates both `TryFrom<u16> for TcpMessageType` and
/// `ControlMessage::type_id()` / `is_fancy_extension()` from a single
/// list of variant names.  The variant names must match between both
/// enums (except `UdpTunnel` which is handled separately).
macro_rules! message_type_mapping {
    ($($variant:ident),* $(,)?) => {
        impl TryFrom<u16> for TcpMessageType {
            type Error = crate::error::Error;

            fn try_from(value: u16) -> Result<Self, Self::Error> {
                match value {
                    $(v if v == Self::$variant as u16 => Ok(Self::$variant),)*
                    other => Err(crate::error::Error::UnknownMessageType(other)),
                }
            }
        }

        impl ControlMessage {
            /// The `TcpMessageType` wire ID for this message.
            pub fn type_id(&self) -> u16 {
                match self {
                    $(Self::$variant(_) => TcpMessageType::$variant as u16,)*
                }
            }

            /// Whether this is a Fancy Mumble extension type (ID >= 100),
            /// unknown to legacy Mumble servers.
            pub fn is_fancy_extension(&self) -> bool {
                self.type_id() >= FANCY_EXTENSION_TYPE_THRESHOLD
            }
        }
    };
}

/// A decoded TCP control message received from (or to be sent to) the server.
#[derive(Debug, Clone)]
pub enum ControlMessage {
    /// Protocol version negotiation.
    Version(mumble_tcp::Version),
    /// Client authentication.
    Authenticate(mumble_tcp::Authenticate),
    /// Keep-alive ping.
    Ping(mumble_tcp::Ping),
    /// Server rejected the connection.
    Reject(mumble_tcp::Reject),
    /// Successful authentication acknowledgement.
    ServerSync(mumble_tcp::ServerSync),
    /// A channel was removed.
    ChannelRemove(mumble_tcp::ChannelRemove),
    /// Channel metadata update.
    ChannelState(mumble_tcp::ChannelState),
    /// A user disconnected.
    UserRemove(mumble_tcp::UserRemove),
    /// User state change (mute, channel, etc.).
    UserState(mumble_tcp::UserState),
    /// Ban list from the server.
    BanList(mumble_tcp::BanList),
    /// A text chat message.
    TextMessage(mumble_tcp::TextMessage),
    /// Server denied an action.
    PermissionDenied(mumble_tcp::PermissionDenied),
    /// Access-control list for a channel.
    Acl(mumble_tcp::Acl),
    /// Registered user name map.
    QueryUsers(mumble_tcp::QueryUsers),
    /// OCB encryption key setup.
    CryptSetup(mumble_tcp::CryptSetup),
    /// Add/remove a contextual action.
    ContextActionModify(mumble_tcp::ContextActionModify),
    /// A contextual action was triggered.
    ContextAction(mumble_tcp::ContextAction),
    /// Registered user list.
    UserList(mumble_tcp::UserList),
    /// Voice target (whisper/shout) configuration.
    VoiceTarget(mumble_tcp::VoiceTarget),
    /// Channel permission query or response.
    PermissionQuery(mumble_tcp::PermissionQuery),
    /// Negotiated audio codec version.
    CodecVersion(mumble_tcp::CodecVersion),
    /// Detailed user statistics.
    UserStats(mumble_tcp::UserStats),
    /// Request to send a large blob.
    RequestBlob(mumble_tcp::RequestBlob),
    /// Global server configuration values.
    ServerConfig(mumble_tcp::ServerConfig),
    /// Server hints at an outdated client configuration.
    SuggestConfig(mumble_tcp::SuggestConfig),
    /// Plugin data relay message.
    PluginDataTransmission(mumble_tcp::PluginDataTransmission),
    /// Fancy: encrypted persistent chat message.
    PchatMessage(mumble_tcp::PchatMessage),
    /// Fancy: request to fetch stored messages.
    PchatFetch(mumble_tcp::PchatFetch),
    /// Fancy: server response to a fetch request.
    PchatFetchResponse(mumble_tcp::PchatFetchResponse),
    /// Fancy: server delivers a stored message to the client.
    PchatMessageDeliver(mumble_tcp::PchatMessageDeliver),
    /// Fancy: client announces its E2EE identity keys.
    PchatKeyAnnounce(mumble_tcp::PchatKeyAnnounce),
    /// Fancy: peer-to-peer encrypted key exchange.
    PchatKeyExchange(mumble_tcp::PchatKeyExchange),
    /// Fancy: server requests a key for a new member.
    PchatKeyRequest(mumble_tcp::PchatKeyRequest),
    /// Fancy: server acknowledgement of a stored message.
    PchatAck(mumble_tcp::PchatAck),
    /// Fancy: custodian countersignature for an epoch transition.
    PchatEpochCountersig(mumble_tcp::PchatEpochCountersig),
    /// Fancy: report that a peer holds the channel key.
    PchatKeyHolderReport(mumble_tcp::PchatKeyHolderReport),
    /// Fancy: query for list of key holders.
    PchatKeyHoldersQuery(mumble_tcp::PchatKeyHoldersQuery),
    /// Fancy: server response with the key-holder list.
    PchatKeyHoldersList(mumble_tcp::PchatKeyHoldersList),
    /// Fancy: server challenge to prove key possession.
    PchatKeyChallenge(mumble_tcp::PchatKeyChallenge),
    /// Fancy: client response to a key-possession challenge.
    PchatKeyChallengeResponse(mumble_tcp::PchatKeyChallengeResponse),
    /// Fancy: server verdict on a key-possession challenge.
    PchatKeyChallengeResult(mumble_tcp::PchatKeyChallengeResult),
    /// Fancy: delete persisted messages.
    PchatDeleteMessages(mumble_tcp::PchatDeleteMessages),
    /// Fancy: server drains offline message queue.
    PchatOfflineQueueDrain(mumble_tcp::PchatOfflineQueueDrain),
    /// Fancy: client sends a reaction on a persistent message.
    PchatReaction(mumble_tcp::PchatReaction),
    /// Fancy: server broadcasts a reaction update.
    PchatReactionDeliver(mumble_tcp::PchatReactionDeliver),
    /// Fancy: server response with reactions for requested messages.
    PchatReactionFetchResponse(mumble_tcp::PchatReactionFetchResponse),
    /// Fancy: WebRTC screen-sharing signaling relay.
    WebRtcSignal(mumble_tcp::WebRtcSignal),
    /// Fancy: Signal sender key distribution.
    PchatSenderKeyDistribution(mumble_tcp::PchatSenderKeyDistribution),
    /// Fancy: FCM push notification registration.
    FancyPushRegister(mumble_tcp::FancyPushRegister),
    /// Fancy: push notification mute preferences update.
    FancyPushUpdate(mumble_tcp::FancyPushUpdate),
    /// Fancy: server broadcasts custom reaction/emoji config.
    FancyCustomReactionsConfig(mumble_tcp::FancyCustomReactionsConfig),
    /// Fancy: live push subscribe for connected clients.
    FancySubscribePush(mumble_tcp::FancySubscribePush),
    /// Fancy: client sends a read receipt watermark.
    FancyReadReceipt(mumble_tcp::FancyReadReceipt),
    /// Fancy: server broadcasts read receipt state.
    FancyReadReceiptDeliver(mumble_tcp::FancyReadReceiptDeliver),
    /// Fancy: client sends a pin/unpin request.
    PchatPin(mumble_tcp::PchatPin),
    /// Fancy: server broadcasts a pin state change.
    PchatPinDeliver(mumble_tcp::PchatPinDeliver),
    /// Fancy: server response with pinned messages for a channel.
    PchatPinFetchResponse(mumble_tcp::PchatPinFetchResponse),
    /// Fancy: typing indicator broadcast.
    FancyTypingIndicator(mumble_tcp::FancyTypingIndicator),
    /// Fancy: client requests link previews from the server.
    FancyLinkPreviewRequest(mumble_tcp::FancyLinkPreviewRequest),
    /// Fancy: server responds with link embed metadata.
    FancyLinkPreviewResponse(mumble_tcp::FancyLinkPreviewResponse),
    /// Fancy: synchronised watch-together control event.
    FancyWatchSync(mumble_tcp::FancyWatchSync),
    /// Fancy: drawing stroke overlay for screen-share collaboration.
    FancyDrawStroke(mumble_tcp::FancyDrawStroke),
    /// UDP audio tunneled through TCP (fallback path).
    UdpTunnel(Vec<u8>),
}

/// First Fancy Mumble extension type ID. All IDs at or above this
/// threshold are Fancy-specific and unknown to legacy Mumble servers.
pub const FANCY_EXTENSION_TYPE_THRESHOLD: u16 = TcpMessageType::PchatMessage as u16;

message_type_mapping! {
    Version, UdpTunnel, Authenticate, Ping, Reject, ServerSync,
    ChannelRemove, ChannelState, UserRemove, UserState, BanList,
    TextMessage, PermissionDenied, Acl, QueryUsers, CryptSetup,
    ContextActionModify, ContextAction, UserList, VoiceTarget,
    PermissionQuery, CodecVersion, UserStats, RequestBlob,
    ServerConfig, SuggestConfig, PluginDataTransmission,
    PchatMessage, PchatFetch, PchatFetchResponse, PchatMessageDeliver,
    PchatKeyAnnounce, PchatKeyExchange, PchatKeyRequest, PchatAck,
    PchatEpochCountersig, PchatKeyHolderReport, PchatKeyHoldersQuery,
    PchatKeyHoldersList, PchatKeyChallenge, PchatKeyChallengeResponse,
    PchatKeyChallengeResult, PchatDeleteMessages, PchatOfflineQueueDrain,
    PchatReaction, PchatReactionDeliver, PchatReactionFetchResponse,
    WebRtcSignal, PchatSenderKeyDistribution,
    FancyPushRegister, FancyPushUpdate, FancyCustomReactionsConfig,
    FancySubscribePush, FancyReadReceipt, FancyReadReceiptDeliver,
    PchatPin, PchatPinDeliver, PchatPinFetchResponse,
    FancyTypingIndicator,
    FancyLinkPreviewRequest, FancyLinkPreviewResponse,
    FancyWatchSync, FancyDrawStroke,
}

/// A decoded UDP message - either audio or a UDP ping.
#[derive(Debug, Clone)]
pub enum UdpMessage {
    /// An audio packet (encoded speech or music).
    Audio(mumble_udp::Audio),
    /// A UDP-level ping for latency measurement.
    Ping(mumble_udp::Ping),
}

/// Unified inbound message from either transport.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant, reason = "Control variant must hold a full ControlMessage; boxing would add heap allocation on the hot audio path")]
pub enum ServerMessage {
    /// Control-plane message received over TCP.
    Control(ControlMessage),
    /// Real-time audio/ping received over UDP (or UDP-over-TCP tunnel).
    Udp(UdpMessage),
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
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
            (109, TcpMessageType::PchatKeyHolderReport),
            (110, TcpMessageType::PchatKeyHoldersQuery),
            (111, TcpMessageType::PchatKeyHoldersList),
            (112, TcpMessageType::PchatKeyChallenge),
            (113, TcpMessageType::PchatKeyChallengeResponse),
            (114, TcpMessageType::PchatKeyChallengeResult),
            (115, TcpMessageType::PchatDeleteMessages),
            (116, TcpMessageType::PchatOfflineQueueDrain),
            (117, TcpMessageType::PchatReaction),
            (118, TcpMessageType::PchatReactionDeliver),
            (119, TcpMessageType::PchatReactionFetchResponse),
            (120, TcpMessageType::WebRtcSignal),
            (121, TcpMessageType::PchatSenderKeyDistribution),
            (128, TcpMessageType::PchatPin),
            (129, TcpMessageType::PchatPinDeliver),
            (130, TcpMessageType::PchatPinFetchResponse),
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
        // Key-holder IDs (109..=111)
        for id in 109..=111u16 {
            let msg_type = TcpMessageType::try_from(id).unwrap();
            assert_eq!(msg_type as u16, id);
        }
        // Key-challenge IDs (112..=115)
        for id in 112..=115u16 {
            let msg_type = TcpMessageType::try_from(id).unwrap();
            assert_eq!(msg_type as u16, id);
        }
        // Offline queue ID (116)
        {
            let msg_type = TcpMessageType::try_from(116u16).unwrap();
            assert_eq!(msg_type as u16, 116);
        }
        // Reaction IDs (117..=119)
        for id in 117..=119u16 {
            let msg_type = TcpMessageType::try_from(id).unwrap();
            assert_eq!(msg_type as u16, id);
        }
        // WebRtcSignal ID (120)
        {
            let msg_type = TcpMessageType::try_from(120u16).unwrap();
            assert_eq!(msg_type as u16, 120);
        }
        // PchatSenderKeyDistribution ID (121)
        {
            let msg_type = TcpMessageType::try_from(121u16).unwrap();
            assert_eq!(msg_type as u16, 121);
        }
        // FancyPushRegister (122) .. FancyReadReceiptDeliver (127)
        for id in 122..=127u16 {
            let msg_type = TcpMessageType::try_from(id).unwrap();
            assert_eq!(msg_type as u16, id);
        }
        // PchatPin (128) .. PchatPinFetchResponse (130)
        for id in 128..=130u16 {
            let msg_type = TcpMessageType::try_from(id).unwrap();
            assert_eq!(msg_type as u16, id);
        }
        // FancyTypingIndicator (131) .. FancyDrawStroke (135)
        for id in 131..=135u16 {
            let msg_type = TcpMessageType::try_from(id).unwrap();
            assert_eq!(msg_type as u16, id);
        }
    }

    #[test]
    fn tcp_message_type_invalid_returns_error() {
        assert!(TcpMessageType::try_from(27u16).is_err());
        assert!(TcpMessageType::try_from(99u16).is_err());
        assert!(TcpMessageType::try_from(136u16).is_err());
        assert!(TcpMessageType::try_from(199u16).is_err());
        assert!(TcpMessageType::try_from(203u16).is_err());
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
