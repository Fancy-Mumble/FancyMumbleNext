//! Inbound message handlers: decrypt, decode, store, and acknowledge
//! persistent chat messages received from the server.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tracing::{debug, warn};

use mumble_protocol::command;
use mumble_protocol::persistent::PchatProtocol;
use mumble_protocol::proto::mumble_tcp;

use crate::state::local_cache::CachedMessage;
use crate::state::types::ChatMessage;
use crate::state::SharedState;

use super::conversion::proto_to_protocol;
use super::settings::PLACEHOLDER_BODY;
use super::PchatState;
use super::PendingSignalEnvelope;

// -- Message delivery -------------------------------------------------

pub(crate) fn handle_proto_msg_deliver(
    shared: &Arc<Mutex<SharedState>>,
    msg: &mumble_tcp::PchatMessageDeliver,
) {
    let message_id = msg.message_id.clone().unwrap_or_default();
    let channel_id = msg.channel_id.unwrap_or(0);
    let timestamp = msg.timestamp.unwrap_or(0);
    let sender_hash = msg.sender_hash.clone().unwrap_or_default();
    let protocol = proto_to_protocol(msg.protocol);
    let envelope_bytes = msg.envelope.clone().unwrap_or_default();
    let replaces_id = msg.replaces_id.clone();

    debug!(data_len = envelope_bytes.len(), "pchat: handle_proto_msg_deliver entry");
    debug!(
        message_id = %message_id,
        channel_id,
        sender = %sender_hash,
        "received pchat msg-deliver"
    );

    let Ok(mut state) = shared.lock() else { return };

    let Some(pchat) = state.pchat.as_mut() else {
        return;
    };

    let (body, sender_name, decrypted) = decrypt_or_stash(
        pchat, protocol, &sender_hash, channel_id, &message_id, timestamp, envelope_bytes,
    );

    if protocol == PchatProtocol::SignalV1 && decrypted {
        pchat.cache_signal_message(CachedMessage {
            message_id: message_id.clone(),
            channel_id,
            timestamp,
            sender_hash: sender_hash.clone(),
            sender_name: sender_name.clone(),
            body: body.clone(),
            is_own: false,
        });
    }

    let sender_session = state
        .users
        .values()
        .find(|u| u.hash.as_deref() == Some(&sender_hash))
        .map(|u| u.session);

    // The server never echoes PchatMessageDeliver back to the sender.
    let chat_msg = ChatMessage {
        sender_session,
        sender_name,
        sender_hash: Some(sender_hash),
        body,
        channel_id,
        is_own: false,
        dm_session: None,
        group_id: None,
        message_id: Some(message_id.clone()),
        timestamp: Some(timestamp),
        is_legacy: false,
    };

    insert_or_replace_message(&mut state, channel_id, &message_id, replaces_id.as_deref(), chat_msg);
}

/// Decrypt an envelope, stashing it for later retry on `SignalV1` failure.
fn decrypt_or_stash(
    pchat: &mut PchatState,
    protocol: PchatProtocol,
    sender_hash: &str,
    channel_id: u32,
    message_id: &str,
    timestamp: u64,
    envelope_bytes: Vec<u8>,
) -> (String, String, bool) {
    match (super::InboundEnvelope {
        protocol, sender_hash, channel_id, message_id, timestamp,
        envelope_bytes: &envelope_bytes, epoch: None, chain_index: None,
        epoch_fingerprint: [0u8; 8],
    }).decrypt(pchat) {
        Ok(env) => {
            debug!(message_id = %message_id, "pchat msg-deliver: decrypted OK");
            (env.body, env.sender_name, true)
        }
        Err(e) => {
            warn!(
                message_id = %message_id,
                channel_id,
                sender = %sender_hash,
                ciphertext_len = envelope_bytes.len(),
                has_key = pchat.key_manager.has_key(channel_id, protocol),
                "failed to decrypt message: {e}"
            );
            if protocol == PchatProtocol::SignalV1 {
                pchat.stash_signal_envelope(PendingSignalEnvelope {
                    message_id: message_id.to_owned(),
                    channel_id,
                    timestamp,
                    sender_hash: sender_hash.to_owned(),
                    envelope_bytes,
                });
            }
            (PLACEHOLDER_BODY.to_string(), sender_hash.to_owned(), false)
        }
    }
}

/// Insert a message into the channel history, replacing an existing one
/// if `replaces_id` matches, or deduplicating by `message_id`.
fn insert_or_replace_message(
    state: &mut SharedState,
    channel_id: u32,
    message_id: &str,
    replaces_id: Option<&str>,
    chat_msg: ChatMessage,
) {
    if let Some(replaces_id) = replaces_id {
        if let Some(msgs) = state.messages.get_mut(&channel_id) {
            if let Some(pos) = msgs
                .iter()
                .position(|m| m.message_id.as_deref() == Some(replaces_id))
            {
                msgs[pos] = chat_msg;
                return;
            }
        }
    }

    if let Some(msgs) = state.messages.get(&channel_id) {
        if msgs
            .iter()
            .any(|m| m.message_id.as_deref() == Some(message_id))
        {
            return;
        }
    }

    state
        .messages
        .entry(channel_id)
        .or_default()
        .push(chat_msg);
}

// -- Fetch response ---------------------------------------------------

pub(crate) fn handle_proto_fetch_resp(
    shared: &Arc<Mutex<SharedState>>,
    msg: &mumble_tcp::PchatFetchResponse,
) {
    let channel_id = msg.channel_id.unwrap_or(0);
    let has_more = msg.has_more.unwrap_or(false);
    let total_stored = msg.total_stored.unwrap_or(0);

    debug!(data_len = msg.messages.len(), "pchat: handle_proto_fetch_resp entry");
    debug!(
        channel_id,
        count = msg.messages.len(),
        has_more,
        total = total_stored,
        "received pchat fetch-resp"
    );

    // Phase 1: take PchatState out so we can decrypt without holding
    //   the lock.
    let (mut pchat, own_cert_hash, user_hashes) = {
        let mut state = shared.lock().ok().unwrap_or_else(|| unreachable!());
        let Some(pchat) = state.pchat.take() else {
            return;
        };
        let own_cert_hash = pchat.own_cert_hash.clone();
        let user_hashes: HashMap<String, u32> = state
            .users
            .values()
            .filter_map(|u| u.hash.as_ref().map(|h| (h.clone(), u.session)))
            .collect();
        (pchat, own_cert_hash, user_hashes)
    };

    // Phase 2: decrypt and decode all messages without a lock.
    let decrypted_msgs =
        decrypt_fetched_messages(&mut pchat, &own_cert_hash, &user_hashes, &msg.messages);

    // Phase 3: re-lock briefly to put pchat back and insert messages.
    let Ok(mut state) = shared.lock() else {
        return;
    };
    state.pchat = Some(pchat);

    merge_decrypted_messages(&mut state, channel_id, decrypted_msgs);
}

/// Decrypt a batch of fetched messages outside the state lock.
fn decrypt_fetched_messages(
    pchat: &mut PchatState,
    own_cert_hash: &str,
    user_hashes: &HashMap<String, u32>,
    messages: &[mumble_tcp::PchatMessage],
) -> Vec<ChatMessage> {
    let mut decrypted_msgs: Vec<ChatMessage> = Vec::with_capacity(messages.len());

    for proto_msg in messages {
        let msg_id = proto_msg.message_id.clone().unwrap_or_default();
        let msg_channel_id = proto_msg.channel_id.unwrap_or(0);
        let msg_timestamp = proto_msg.timestamp.unwrap_or(0);
        let msg_sender_hash = proto_msg.sender_hash.clone().unwrap_or_default();
        let protocol = proto_to_protocol(proto_msg.protocol);
        let has_key = pchat.key_manager.has_key(msg_channel_id, protocol);

        debug!(
            message_id = %msg_id,
            channel_id = msg_channel_id,
            timestamp = msg_timestamp,
            sender = %msg_sender_hash,
            envelope_len = proto_msg.envelope.as_ref().map(Vec::len).unwrap_or(0),
            has_key,
            "pchat fetch-resp: processing message"
        );

        let epoch_fp: [u8; 8] = proto_msg
            .epoch_fingerprint
            .clone()
            .unwrap_or_default()
            .try_into()
            .unwrap_or([0u8; 8]);

        let (body, sender_name, decrypted) = match (super::InboundEnvelope {
            protocol,
            sender_hash: &msg_sender_hash,
            channel_id: msg_channel_id,
            message_id: &msg_id,
            timestamp: msg_timestamp,
            envelope_bytes: &proto_msg.envelope.clone().unwrap_or_default(),
            epoch: proto_msg.epoch,
            chain_index: proto_msg.chain_index,
            epoch_fingerprint: epoch_fp,
        }).decrypt(pchat) {
            Ok(env) => {
                debug!(message_id = %msg_id, "pchat fetch-resp: decrypted OK");
                (env.body, env.sender_name, true)
            }
            Err(e) => {
                warn!(message_id = %msg_id, channel_id = msg_channel_id, has_key, "fetch-resp: decrypt failed: {e}");
                (PLACEHOLDER_BODY.to_string(), msg_sender_hash.clone(), false)
            }
        };

        let is_own =
            !msg_sender_hash.is_empty() && !own_cert_hash.is_empty() && msg_sender_hash == own_cert_hash;

        if protocol == PchatProtocol::SignalV1 && decrypted {
            pchat.cache_signal_message(CachedMessage {
                message_id: msg_id.clone(),
                channel_id: msg_channel_id,
                timestamp: msg_timestamp,
                sender_hash: msg_sender_hash.clone(),
                sender_name: sender_name.clone(),
                body: body.clone(),
                is_own,
            });
        }

        debug!(
            message_id = %msg_id,
            msg_sender_hash = %msg_sender_hash,
            own_cert_hash = %own_cert_hash,
            is_own,
            sender_name = %sender_name,
            "pchat fetch-resp: is_own check"
        );

        let sender_session = user_hashes.get(&msg_sender_hash).copied();

        decrypted_msgs.push(ChatMessage {
            sender_session,
            sender_name,
            sender_hash: Some(msg_sender_hash),
            body,
            channel_id: msg_channel_id,
            is_own,
            dm_session: None,
            group_id: None,
            message_id: Some(msg_id),
            timestamp: Some(msg_timestamp),
            is_legacy: false,
        });
    }

    decrypted_msgs
}

/// Merge decrypted messages into the channel history, deduplicating
/// by `message_id` and sorting by timestamp.
fn merge_decrypted_messages(
    state: &mut SharedState,
    channel_id: u32,
    decrypted_msgs: Vec<ChatMessage>,
) {
    if decrypted_msgs.is_empty() {
        debug!(channel_id, "pchat fetch-resp: no messages to insert (all filtered/empty)");
        return;
    }

    debug!(
        channel_id,
        new_count = decrypted_msgs.len(),
        "pchat fetch-resp: inserting decrypted messages"
    );
    let existing = state.messages.entry(channel_id).or_default();

    let existing_ids: std::collections::HashSet<&str> = existing
        .iter()
        .filter_map(|m| m.message_id.as_deref())
        .collect();

    let mut new_msgs: Vec<ChatMessage> = decrypted_msgs
        .into_iter()
        .filter(|m| match m.message_id.as_deref() {
            Some(id) => !existing_ids.contains(id),
            None => true,
        })
        .collect();

    new_msgs.append(existing);
    *existing = new_msgs;
    existing.sort_by_key(|m| m.timestamp.unwrap_or(0));

    debug!(
        channel_id,
        total_messages = existing.len(),
        "pchat fetch-resp: messages after merge+sort"
    );
}

// -- Ack --------------------------------------------------------------

pub(crate) fn handle_proto_ack(msg: &mumble_tcp::PchatAck) {
    let message_ids = &msg.message_ids;
    let status = msg.status.unwrap_or(0);
    let reason = msg.reason.as_deref();

    if status == mumble_tcp::PchatAckStatus::PchatAckRejected as i32
        || status == mumble_tcp::PchatAckStatus::PchatAckQuotaExceeded as i32
    {
        warn!(?message_ids, status, reason = ?reason, "pchat message rejected by server");
    } else {
        debug!(?message_ids, status, "received pchat ack");
    }
}

// -- Delete -----------------------------------------------------------

pub(crate) fn handle_proto_delete_messages(
    shared: &Arc<Mutex<SharedState>>,
    msg: &mumble_tcp::PchatDeleteMessages,
) {
    let channel_id = msg.channel_id.unwrap_or(0);
    let Ok(mut state) = shared.lock() else {
        return;
    };

    let Some(messages) = state.messages.get_mut(&channel_id) else {
        debug!(channel_id, "pchat delete: no local messages for channel");
        return;
    };

    let before = messages.len();
    let ids = &msg.message_ids;
    let time_range = msg.time_range.as_ref();
    let sender_hash = msg.sender_hash.as_deref();

    messages.retain(|m| {
        if !ids.is_empty() {
            if let Some(ref mid) = m.message_id {
                if ids.iter().any(|id| id == mid) {
                    return false;
                }
            }
        }
        if let Some(range) = time_range {
            if let Some(ts) = m.timestamp {
                let after_from = range.from.is_none_or(|f| ts >= f);
                let before_to = range.to.is_none_or(|t| ts <= t);
                if after_from && before_to {
                    return false;
                }
            }
        }
        if let Some(hash) = sender_hash {
            if m.sender_name == hash {
                return false;
            }
        }
        true
    });

    let removed = before - messages.len();
    debug!(channel_id, removed, "pchat delete: evicted messages from local store");
}

// -- Offline queue drain ----------------------------------------------

pub(crate) fn handle_proto_offline_queue_drain(
    shared: &Arc<Mutex<SharedState>>,
    msg: &mumble_tcp::PchatOfflineQueueDrain,
) {
    let channel_id = msg.channel_id.unwrap_or(0);

    debug!(
        channel_id,
        distribution_count = msg.distributions.len(),
        message_count = msg.messages.len(),
        "received offline queue drain"
    );

    // Process sender key distributions BEFORE attempting to decrypt
    // messages. The server bundles all relevant SKDMs so we can
    // establish the sender key state necessary for decryption.
    for dist in &msg.distributions {
        let sender_hash = dist.sender_hash.clone().unwrap_or_default();
        let dist_channel = dist.channel_id.unwrap_or(channel_id);
        let data = dist.distribution.clone().unwrap_or_default();
        if !data.is_empty() {
            let _ = super::handle_signal_sender_key_by_hash(shared, &sender_hash, dist_channel, &data);
        }
    }

    if msg.messages.is_empty() {
        return;
    }

    let Ok(mut state) = shared.lock() else {
        return;
    };
    let Some(pchat) = state.pchat.as_mut() else {
        return;
    };

    let decrypted_msgs = decrypt_offline_batch(pchat, channel_id, &msg.messages);
    let acked_ids = insert_offline_messages(&mut state, channel_id, &decrypted_msgs);

    if let Some(msgs) = state.messages.get_mut(&channel_id) {
        msgs.sort_by_key(|m| m.timestamp.unwrap_or(0));
    }

    if !acked_ids.is_empty() {
        send_offline_queue_ack(&state, channel_id, acked_ids);
    }
}

/// Intermediate result after decrypting a single offline-queued message.
struct DecryptedOfflineMsg {
    message_id: String,
    timestamp: u64,
    sender_hash: String,
    body: String,
    sender_name: String,
}

fn decrypt_offline_batch(
    pchat: &mut PchatState,
    channel_id: u32,
    messages: &[mumble_tcp::PchatMessageDeliver],
) -> Vec<DecryptedOfflineMsg> {
    let mut results = Vec::with_capacity(messages.len());

    for deliver in messages {
        let message_id = deliver.message_id.clone().unwrap_or_default();
        let timestamp = deliver.timestamp.unwrap_or(0);
        let sender_hash = deliver.sender_hash.clone().unwrap_or_default();
        let protocol = proto_to_protocol(deliver.protocol);
        let envelope_bytes = deliver.envelope.clone().unwrap_or_default();

        let (body, sender_name, decrypted) = match (super::InboundEnvelope {
            protocol,
            sender_hash: &sender_hash,
            channel_id,
            message_id: &message_id,
            timestamp,
            envelope_bytes: &envelope_bytes,
            epoch: None,
            chain_index: None,
            epoch_fingerprint: [0u8; 8],
        }).decrypt(pchat) {
            Ok(env) => {
                debug!(message_id = %message_id, "offline drain: decrypted OK");
                (env.body, env.sender_name, true)
            }
            Err(e) => {
                warn!(message_id = %message_id, channel_id, "offline drain: failed to decrypt: {e}");
                if protocol == PchatProtocol::SignalV1 {
                    pchat.stash_signal_envelope(PendingSignalEnvelope {
                        message_id: message_id.clone(),
                        channel_id,
                        timestamp,
                        sender_hash: sender_hash.clone(),
                        envelope_bytes,
                    });
                }
                (PLACEHOLDER_BODY.to_string(), sender_hash.clone(), false)
            }
        };

        if protocol == PchatProtocol::SignalV1 && decrypted {
            pchat.cache_signal_message(CachedMessage {
                message_id: message_id.clone(),
                channel_id,
                timestamp,
                sender_hash: sender_hash.clone(),
                sender_name: sender_name.clone(),
                body: body.clone(),
                is_own: false,
            });
        }

        results.push(DecryptedOfflineMsg {
            message_id,
            timestamp,
            sender_hash,
            body,
            sender_name,
        });
    }

    results
}

fn insert_offline_messages(
    state: &mut SharedState,
    channel_id: u32,
    decrypted: &[DecryptedOfflineMsg],
) -> Vec<String> {
    let mut acked_ids: Vec<String> = Vec::with_capacity(decrypted.len());

    for dm in decrypted {
        if let Some(msgs) = state.messages.get(&channel_id) {
            if msgs
                .iter()
                .any(|m| m.message_id.as_deref() == Some(&dm.message_id))
            {
                acked_ids.push(dm.message_id.clone());
                continue;
            }
        }

        let sender_session = state
            .users
            .values()
            .find(|u| u.hash.as_deref() == Some(&dm.sender_hash))
            .map(|u| u.session);

        let chat_msg = ChatMessage {
            sender_session,
            sender_name: dm.sender_name.clone(),
            sender_hash: Some(dm.sender_hash.clone()),
            body: dm.body.clone(),
            channel_id,
            is_own: false,
            dm_session: None,
            group_id: None,
            message_id: Some(dm.message_id.clone()),
            timestamp: Some(dm.timestamp),
            is_legacy: false,
        };

        state
            .messages
            .entry(channel_id)
            .or_default()
            .push(chat_msg);

        acked_ids.push(dm.message_id.clone());
    }

    acked_ids
}

fn send_offline_queue_ack(state: &SharedState, channel_id: u32, acked_ids: Vec<String>) {
    let Some(handle) = state.client_handle.clone() else {
        return;
    };
    let ack = mumble_tcp::PchatAck {
        message_ids: acked_ids.clone(),
        status: Some(mumble_tcp::PchatAckStatus::PchatAckStored as i32),
        reason: None,
        channel_id: Some(channel_id),
    };
    let _ack_task = tokio::spawn(async move {
        if let Err(e) = handle.send(command::SendPchatAck { ack }).await {
            warn!(channel_id, "failed to send offline queue ack: {e}");
        } else {
            debug!(channel_id, count = acked_ids.len(), "sent offline queue ack");
        }
    });
}
