//! Per-message server version requirements and fallback policies.
//!
//! Each Fancy extension message type declares the minimum server
//! `fancy_version` required for native handling and whether a
//! `PluginData` fallback is available when the server is too old.
//!
//! The [`fancy_message_support!`] macro generates the
//! [`message_support`] lookup function from a compact declaration table.

use crate::message::ControlMessage;

/// Whether a Fancy extension message can fall back to `PluginData`
/// relay when the server does not natively understand it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackPolicy {
    /// Message can be wrapped in `PluginDataTransmission` and relayed
    /// client-to-client through any Mumble server.
    PluginData,
    /// Message requires server-side processing; sending via `PluginData`
    /// would be meaningless.
    ServerOnly,
}

/// Minimum server version and fallback policy for a Fancy extension
/// message.
#[derive(Debug, Clone, Copy)]
pub struct MessageSupport {
    /// Minimum `fancy_version` the server must report for native
    /// handling.
    pub min_version: u64,
    /// What to do when the server is too old.
    pub fallback: FallbackPolicy,
}

/// Declares the server version requirements and fallback policy for
/// each Fancy extension message type.
///
/// Each entry has the form:
///
/// ```text
/// (major, minor, patch) Variant => Policy
/// ```
///
/// where *version* is the Fancy Mumble server release that first
/// understands the message natively, and *Policy* is either
/// `PluginData` (client-to-client relay is possible) or `ServerOnly`
/// (no sensible fallback).
macro_rules! fancy_message_support {
    ($(($major:literal, $minor:literal, $patch:literal) $variant:ident => $fallback:ident),* $(,)?) => {
        /// Look up the server version requirement and fallback policy
        /// for a Fancy extension [`ControlMessage`].
        ///
        /// Returns `None` for standard Mumble messages (type < 100).
        pub fn message_support(msg: &ControlMessage) -> Option<MessageSupport> {
            match msg {
                $(
                    ControlMessage::$variant(_) => Some(MessageSupport {
                        min_version: fancy_utils::version::fancy_version_encode(
                            $major, $minor, $patch,
                        ),
                        fallback: FallbackPolicy::$fallback,
                    }),
                )*
                _ => None,
            }
        }
    };
}

fancy_message_support! {
    // -- Persistent chat (server-processed) -- 0.2.12 ----------------
    (0, 2, 12) PchatMessage              => ServerOnly,
    (0, 2, 12) PchatFetch                => ServerOnly,
    (0, 2, 12) PchatFetchResponse        => ServerOnly,
    (0, 2, 12) PchatMessageDeliver       => ServerOnly,
    (0, 2, 12) PchatKeyAnnounce          => ServerOnly,
    (0, 2, 12) PchatKeyExchange          => ServerOnly,
    (0, 2, 12) PchatKeyRequest           => ServerOnly,
    (0, 2, 12) PchatAck                  => ServerOnly,
    (0, 2, 12) PchatEpochCountersig      => ServerOnly,
    (0, 2, 12) PchatKeyHolderReport      => ServerOnly,
    (0, 2, 12) PchatKeyHoldersQuery      => ServerOnly,
    (0, 2, 12) PchatKeyHoldersList       => ServerOnly,
    (0, 2, 12) PchatKeyChallenge         => ServerOnly,
    (0, 2, 12) PchatKeyChallengeResponse => ServerOnly,
    (0, 2, 12) PchatKeyChallengeResult   => ServerOnly,
    (0, 2, 12) PchatDeleteMessages       => ServerOnly,
    (0, 2, 12) PchatOfflineQueueDrain    => ServerOnly,
    (0, 2, 12) PchatReaction             => ServerOnly,
    (0, 2, 12) PchatReactionDeliver      => ServerOnly,
    (0, 2, 12) PchatReactionFetchResponse => ServerOnly,

    // -- Client-to-client relay -- 0.2.12 ----------------------------
    (0, 2, 12) WebRtcSignal               => PluginData,
    (0, 2, 12) PchatSenderKeyDistribution => PluginData,

    // -- Push / notification / config (server-processed) -- 0.2.12 ---
    (0, 2, 12) FancyPushRegister          => ServerOnly,
    (0, 2, 12) FancyPushUpdate            => ServerOnly,
    (0, 2, 12) FancyCustomReactionsConfig => ServerOnly,
    (0, 2, 12) FancySubscribePush         => ServerOnly,
    (0, 2, 12) FancyReadReceipt           => ServerOnly,
    (0, 2, 12) FancyReadReceiptDeliver    => ServerOnly,

    // -- Pin messages (server-processed) -- 0.2.16 -------------------
    (0, 2, 16) PchatPin                   => ServerOnly,
    (0, 2, 16) PchatPinDeliver            => ServerOnly,
    (0, 2, 16) PchatPinFetchResponse      => ServerOnly,

    // -- Typing indicator (client-to-client relay) -- 0.2.18 ---------
    (0, 2, 18) FancyTypingIndicator       => PluginData,

    // -- Watch together (client-to-client relay) -- 0.2.20 -----------
    (0, 2, 20) FancyWatchSync             => PluginData,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]

    use super::*;
    use crate::proto::mumble_tcp;

    const V_0_2_12: u64 = fancy_utils::version::fancy_version_encode(0, 2, 12);
    const V_0_2_18: u64 = fancy_utils::version::fancy_version_encode(0, 2, 18);

    #[test]
    fn returns_none_for_standard_messages() {
        let msg = ControlMessage::Ping(mumble_tcp::Ping::default());
        assert!(message_support(&msg).is_none());
    }

    #[test]
    fn typing_indicator_is_plugin_data_fallback() {
        let msg = ControlMessage::FancyTypingIndicator(
            mumble_tcp::FancyTypingIndicator::default(),
        );
        let support = message_support(&msg).unwrap();
        assert_eq!(support.fallback, FallbackPolicy::PluginData);
        assert_eq!(support.min_version, V_0_2_18);
    }

    #[test]
    fn pchat_message_is_server_only() {
        let msg = ControlMessage::PchatMessage(
            mumble_tcp::PchatMessage::default(),
        );
        let support = message_support(&msg).unwrap();
        assert_eq!(support.fallback, FallbackPolicy::ServerOnly);
        assert_eq!(support.min_version, V_0_2_12);
    }

    #[test]
    fn webrtc_signal_is_plugin_data_fallback() {
        let msg = ControlMessage::WebRtcSignal(mumble_tcp::WebRtcSignal {
            target_session: Some(5),
            ..Default::default()
        });
        let support = message_support(&msg).unwrap();
        assert_eq!(support.fallback, FallbackPolicy::PluginData);
        assert_eq!(support.min_version, V_0_2_12);
    }
}
