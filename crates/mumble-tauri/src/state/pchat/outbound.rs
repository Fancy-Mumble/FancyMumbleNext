//! Outbound message construction and sending for persistent chat.

use tracing::debug;

use mumble_protocol::client::ClientHandle;
use mumble_protocol::command;
use mumble_protocol::persistent::wire::{MessageEnvelope, WireCodec};
use mumble_protocol::persistent::PchatProtocol;
use mumble_protocol::proto::mumble_tcp;

use super::conversion::protocol_to_proto;
use super::PchatState;

// -- Encrypt and build ------------------------------------------------

/// Parameters for building an encrypted pchat message.
pub(crate) struct OutboundMessage<'a> {
    pub channel_id: u32,
    pub protocol: PchatProtocol,
    pub message_id: &'a str,
    pub body: &'a str,
    pub sender_name: &'a str,
    pub sender_session: u32,
    pub timestamp: u64,
}

impl PchatState {
    /// Encrypt a message and build a `PchatMessage` proto struct ready to send.
    ///
    /// This is a synchronous operation (no network I/O) so it can be called
    /// while holding the state lock.
    pub(crate) fn build_encrypted_message(
        &mut self,
        msg: &OutboundMessage<'_>,
    ) -> Result<mumble_tcp::PchatMessage, String> {
        debug!(
            msg.channel_id,
            ?msg.protocol,
            msg.message_id,
            msg.timestamp,
            has_key = self.key_manager.has_key(msg.channel_id, msg.protocol),
            "pchat: build_encrypted_message"
        );
        let envelope = MessageEnvelope {
            body: msg.body.to_string(),
            sender_name: msg.sender_name.to_string(),
            sender_session: msg.sender_session,
            attachments: vec![],
        };

        let envelope_bytes = self
            .codec
            .encode(&envelope)
            .map_err(|e| format!("encode envelope: {e}"))?;

        let payload = self
            .key_manager
            .encrypt(msg.protocol, msg.channel_id, msg.message_id, msg.timestamp, &envelope_bytes)
            .map_err(|e| format!("encrypt message: {e}"))?;

        Ok(mumble_tcp::PchatMessage {
            message_id: Some(msg.message_id.to_string()),
            channel_id: Some(msg.channel_id),
            timestamp: Some(msg.timestamp),
            sender_hash: Some(self.own_cert_hash.clone()),
            protocol: Some(protocol_to_proto(msg.protocol)),
            envelope: Some(payload.ciphertext),
            epoch: payload.epoch,
            chain_index: payload.chain_index,
            epoch_fingerprint: Some(payload.epoch_fingerprint.to_vec()),
            replaces_id: None,
        })
    }
}

// -- Async send operations --------------------------------------------

/// Send a `PchatFetch` proto to request stored messages.
pub(crate) async fn send_fetch(
    handle: &ClientHandle,
    channel_id: u32,
    before_id: Option<String>,
    limit: u32,
) -> Result<(), String> {
    let fetch = mumble_tcp::PchatFetch {
        channel_id: Some(channel_id),
        before_id,
        limit: Some(limit),
        after_id: None,
    };

    handle
        .send(command::SendPchatFetch { fetch })
        .await
        .map_err(|e| format!("send pchat-fetch: {e}"))?;

    debug!(channel_id, "sent pchat-fetch");
    Ok(())
}
