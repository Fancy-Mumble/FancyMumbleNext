//! Key exchange protocol handlers: announce, request, exchange, and
//! post-exchange retry logic.

use std::sync::{Arc, Mutex};

use tracing::{debug, warn};

use mumble_protocol::command;
use mumble_protocol::persistent::PchatProtocol;
use mumble_protocol::proto::mumble_tcp;

use crate::state::types;
use crate::state::SharedState;

use super::conversion::{proto_to_wire_key_announce, proto_to_wire_key_exchange, proto_to_wire_key_request};
use super::key_sharing::{query_key_holders, send_key_holder_report};
use super::persistence::persist_archive_key;
use super::settings::PLACEHOLDER_BODY;

// -- Key announce -----------------------------------------------------

pub(crate) fn handle_proto_key_announce(
    shared: &Arc<Mutex<SharedState>>,
    msg: &mumble_tcp::PchatKeyAnnounce,
) {
    let wire = proto_to_wire_key_announce(msg);

    debug!(
        cert_hash = %wire.cert_hash,
        algo = wire.algorithm_version,
        "received pchat key-announce"
    );

    let Ok(mut state) = shared.lock() else { return };

    let mut should_push_keys = false;
    let peer_cert_hash = wire.cert_hash.clone();

    if let Some(ref mut pchat) = state.pchat_ctx.pchat {
        match pchat.key_manager.record_peer_key(&wire) {
            Ok(true) => {
                debug!(cert_hash = %wire.cert_hash, "recorded peer key");
                should_push_keys = true;
            }
            Ok(false) => debug!(cert_hash = %wire.cert_hash, "stale key-announce discarded"),
            Err(e) => warn!(cert_hash = %wire.cert_hash, "failed to record peer key: {e}"),
        }
    }

    // After successfully recording a peer's public key, emit a consent
    // request to the frontend so the user can decide whether to share.
    // Also collect channels that need a key-holder refresh.
    let channels_to_query: Vec<u32>;

    if should_push_keys {
        let channels_for_peer = find_shareable_channels(&state, &peer_cert_hash);
        channels_to_query = channels_for_peer.clone();

        if !channels_for_peer.is_empty() {
            let peer_name = resolve_peer_name(&state, &peer_cert_hash);
            for ch_id in channels_for_peer {
                queue_key_share_consent(&mut state, ch_id, &peer_cert_hash, &peer_name, None);
            }
        }
    } else {
        channels_to_query = Vec::new();
    }

    // Drop the lock before sending network queries.
    drop(state);

    for ch_id in channels_to_query {
        query_key_holders(shared, ch_id);
    }
}

// -- Find shareable channels ------------------------------------------

/// Find `FullArchive` channel IDs where `peer_cert_hash` is present and
/// we hold the key.
fn find_shareable_channels(state: &SharedState, peer_cert_hash: &str) -> Vec<u32> {
    let Some(ref pchat) = state.pchat_ctx.pchat else {
        return Vec::new();
    };

    let peer_channel_ids: Vec<u32> = state
        .users
        .values()
        .filter(|u| u.hash.as_deref() == Some(peer_cert_hash))
        .map(|u| u.channel_id)
        .collect();

    peer_channel_ids
        .into_iter()
        .filter(|&ch_id| {
            let is_full_archive = state
                .channels
                .get(&ch_id)
                .and_then(|ch| ch.pchat_protocol)
                == Some(PchatProtocol::FancyV1FullArchive);
            let has_key = pchat
                .key_manager
                .has_key(ch_id, PchatProtocol::FancyV1FullArchive);
            let already_holder = pchat
                .key_manager
                .key_holders(ch_id)
                .contains(peer_cert_hash);
            is_full_archive && has_key && !already_holder
        })
        .collect()
}

// -- Check key share for channel --------------------------------------

/// Re-evaluate key sharing after a user moves into a channel.
///
/// Checks whether we hold the archive key for the given `FullArchive`
/// channel and whether any peers have known public keys. For each
/// qualifying peer, a consent request is queued (if not already pending).
pub(crate) fn check_key_share_for_channel(
    shared: &Arc<Mutex<SharedState>>,
    channel_id: u32,
) {
    let Ok(mut state) = shared.lock() else { return };

    let is_full_archive = state
        .channels
        .get(&channel_id)
        .and_then(|c| c.pchat_protocol)
        == Some(PchatProtocol::FancyV1FullArchive);
    if !is_full_archive {
        return;
    }

    let Some(ref pchat) = state.pchat_ctx.pchat else { return };

    if !pchat
        .key_manager
        .has_key(channel_id, PchatProtocol::FancyV1FullArchive)
    {
        return;
    }

    let own_hash = pchat.own_cert_hash.clone();
    let holders = pchat.key_manager.key_holders(channel_id);

    // Collect peers in this channel for which we hold a peer key.
    let peers: Vec<(String, String)> = state
        .users
        .values()
        .filter(|u| u.channel_id == channel_id)
        .filter_map(|u| {
            let hash = u.hash.as_deref()?;
            if hash == own_hash {
                return None;
            }
            if holders.contains(hash) {
                return None;
            }
            let _ = pchat.key_manager.get_peer(hash)?;
            Some((hash.to_owned(), u.name.clone()))
        })
        .collect();

    if peers.is_empty() {
        return;
    }

    for (peer_cert_hash, peer_name) in peers {
        queue_key_share_consent(&mut state, channel_id, &peer_cert_hash, &peer_name, None);
    }
}

// -- Key request ------------------------------------------------------

pub(crate) fn handle_proto_key_request(
    shared: &Arc<Mutex<SharedState>>,
    msg: &mumble_tcp::PchatKeyRequest,
) {
    let wire_request = proto_to_wire_key_request(msg);

    debug!(
        channel_id = wire_request.channel_id,
        requester = %wire_request.requester_hash,
        request_id = %wire_request.request_id,
        "received pchat key-request"
    );

    let Ok(mut state) = shared.lock() else { return };

    let Some(ref pchat) = state.pchat_ctx.pchat else { return };

    if !pchat
        .key_manager
        .has_key(wire_request.channel_id, PchatProtocol::FancyV1FullArchive)
    {
        debug!(
            channel_id = wire_request.channel_id,
            "no key to share for this channel"
        );
        return;
    }

    let peer_cert_hash = wire_request.requester_hash.clone();
    let ch_id = wire_request.channel_id;
    let request_id = wire_request.request_id.clone();

    // Skip requests from users who are not currently online.
    let requester_online = state
        .users
        .values()
        .any(|u| u.hash.as_deref() == Some(peer_cert_hash.as_str()));
    if !requester_online {
        debug!(channel_id = ch_id, peer = %peer_cert_hash, "ignoring key-request from offline user");
        return;
    }

    // Skip requests from users who are already known key holders.
    if pchat.key_manager.key_holders(ch_id).contains(&peer_cert_hash) {
        debug!(channel_id = ch_id, peer = %peer_cert_hash, "ignoring key-request from existing holder");
        return;
    }

    let peer_name = resolve_peer_name(&state, &peer_cert_hash);
    queue_key_share_consent(&mut state, ch_id, &peer_cert_hash, &peer_name, Some(request_id));
}

// -- Key exchange -----------------------------------------------------

pub(crate) fn handle_proto_key_exchange(
    shared: &Arc<Mutex<SharedState>>,
    msg: &mumble_tcp::PchatKeyExchange,
) {
    let wire_exchange = proto_to_wire_key_exchange(msg);

    debug!(
        channel_id = wire_exchange.channel_id,
        sender = %wire_exchange.sender_hash,
        epoch = wire_exchange.epoch,
        "received pchat key-exchange"
    );

    let channel_id = wire_exchange.channel_id;
    let protocol = PchatProtocol::from_wire_str(&wire_exchange.protocol);
    let request_id = wire_exchange.request_id.clone();

    let Ok(mut state) = shared.lock() else { return };

    let key_accepted = try_accept_key_exchange(&mut state, &wire_exchange, protocol, &request_id);

    if key_accepted {
        finalize_key_acceptance(
            state,
            shared,
            channel_id,
            protocol,
            &wire_exchange.sender_hash,
        );
    }
}

/// Attempt to receive a key exchange and determine whether the key was
/// accepted.  Returns `true` when the key manager has a usable key
/// after processing.
fn try_accept_key_exchange(
    state: &mut SharedState,
    wire_exchange: &mumble_protocol::persistent::wire::PchatKeyExchange,
    protocol: PchatProtocol,
    request_id: &Option<String>,
) -> bool {
    let Some(ref mut pchat) = state.pchat_ctx.pchat else {
        return false;
    };
    let channel_id = wire_exchange.channel_id;

    match pchat.key_manager.receive_key_exchange(wire_exchange, None) {
        Ok(()) => {
            debug!(
                channel_id,
                epoch = wire_exchange.epoch,
                "accepted key-exchange"
            );
            pchat
                .key_manager
                .record_key_holder(channel_id, wire_exchange.sender_hash.clone());

            if protocol == PchatProtocol::FancyV1FullArchive {
                if let Some(ref rid) = request_id {
                    match pchat.key_manager.evaluate_consensus(rid, channel_id, &[]) {
                        Ok((trust, Some(_key))) => {
                            debug!(channel_id, ?trust, "accepted archive key via consensus");
                            true
                        }
                        Ok((_, None)) => {
                            warn!(channel_id, "consensus produced no key");
                            false
                        }
                        Err(e) => {
                            warn!(channel_id, "evaluate_consensus failed: {e}");
                            false
                        }
                    }
                } else {
                    pchat.key_manager.has_key(channel_id, protocol)
                }
            } else {
                pchat.key_manager.has_key(channel_id, protocol)
            }
        }
        Err(e) => {
            warn!(channel_id, "key-exchange rejected: {e}");
            false
        }
    }
}

/// Perform all post-acceptance bookkeeping: record self as holder,
/// prune stale consent prompts, emit events, retry decryption, report
/// holder status, and persist the archive key to disk.
fn finalize_key_acceptance(
    mut state: std::sync::MutexGuard<'_, SharedState>,
    shared: &Arc<Mutex<SharedState>>,
    channel_id: u32,
    protocol: PchatProtocol,
    sender_hash: &str,
) {
    // Record ourselves as a holder.
    if let Some(ref mut pchat) = state.pchat_ctx.pchat {
        pchat
            .key_manager
            .record_key_holder(channel_id, pchat.own_cert_hash.clone());
    }

    // Remove stale consent prompts for a sender who already has the key.
    let before_len = state.pchat_ctx.pending_key_shares.len();
    state.pchat_ctx.pending_key_shares.retain(|p| {
        !(p.channel_id == channel_id && p.peer_cert_hash == sender_hash)
    });

    if state.pchat_ctx.pending_key_shares.len() != before_len {
        emit_key_share_requests_changed(&state, channel_id);
    }

    // Extract persistence info before dropping the lock.
    let persist_info = if protocol == PchatProtocol::FancyV1FullArchive {
        state.pchat_ctx.pchat.as_ref().and_then(|p| {
            let (key, _trust) = p.key_manager.get_archive_key(channel_id)?;
            let originator = p
                .key_manager
                .get_channel_originator(channel_id)
                .map(String::from);
            let dir = p.identity_dir.clone()?;
            Some((dir, key, originator))
        })
    } else {
        None
    };

    // Notify the frontend that the revoked key has been replaced.
    if let Some(ref app) = state.conn.tauri_app_handle {
        use tauri::Emitter;
        let _ = app.emit(
            "pchat-key-restored",
            types::PchatKeyRevokedPayload { channel_id },
        );
    }

    retry_decrypt_pending_messages(&mut state, channel_id, protocol);

    drop(state);
    send_key_holder_report(shared, channel_id);

    // Persist the accepted archive key to disk (outside the lock).
    if let Some((dir, key, originator)) = persist_info {
        persist_archive_key(&dir, channel_id, &key, originator.as_deref());
    }
}

// -- Retry decrypt after key exchange ---------------------------------

/// Remove placeholder messages and re-fetch so they are decrypted with
/// the newly accepted key.
fn retry_decrypt_pending_messages(
    state: &mut SharedState,
    channel_id: u32,
    _protocol: PchatProtocol,
) {
    let has_placeholders = state
        .msgs.by_channel
        .get(&channel_id)
        .is_some_and(|msgs| msgs.iter().any(|m| m.body == PLACEHOLDER_BODY));

    if !has_placeholders {
        return;
    }

    debug!(
        channel_id,
        "removing placeholder messages and re-fetching after key exchange"
    );

    if let Some(msgs) = state.msgs.by_channel.get_mut(&channel_id) {
        msgs.retain(|m| m.body != PLACEHOLDER_BODY);
    }

    if let Some(ref mut pchat) = state.pchat_ctx.pchat {
        let _ = pchat.fetched_channels.remove(&channel_id);
    }

    let handle = state.conn.client_handle.clone();
    if let Some(handle) = handle {
        let _refetch_task = tokio::spawn(async move {
            let fetch = mumble_tcp::PchatFetch {
                channel_id: Some(channel_id),
                before_id: None,
                limit: Some(50),
                after_id: None,
            };
            if let Err(e) = handle.send(command::SendPchatFetch { fetch }).await {
                warn!(channel_id, "re-fetch after key exchange failed: {e}");
            } else {
                debug!(channel_id, "sent pchat re-fetch after key exchange");
            }
        });
    }
}

// -- Shared helpers ---------------------------------------------------

/// Queue a key-share consent request to the frontend, avoiding duplicates.
///
/// Extracted to eliminate the triple duplication across `handle_proto_key_announce`,
/// `check_key_share_for_channel`, and `handle_proto_key_request`.
fn queue_key_share_consent(
    state: &mut SharedState,
    channel_id: u32,
    peer_cert_hash: &str,
    peer_name: &str,
    request_id: Option<String>,
) {
    // Avoid duplicate pending requests.
    let already_pending = state
        .pchat_ctx.pending_key_shares
        .iter()
        .any(|p| p.channel_id == channel_id && p.peer_cert_hash == peer_cert_hash);
    if already_pending {
        debug!(
            channel_id,
            peer = %peer_cert_hash,
            "key-share consent already pending, skipping"
        );
        return;
    }

    let pending = types::PendingKeyShare {
        channel_id,
        peer_cert_hash: peer_cert_hash.to_owned(),
        peer_name: peer_name.to_owned(),
        request_id,
    };
    state.pchat_ctx.pending_key_shares.push(pending);

    if let Some(ref app) = state.conn.tauri_app_handle {
        use tauri::Emitter;
        let _ = app.emit(
            "pchat-key-share-request",
            types::KeyShareRequestPayload {
                channel_id,
                peer_name: peer_name.to_owned(),
                peer_cert_hash: peer_cert_hash.to_owned(),
            },
        );
    }

    debug!(
        channel_id,
        peer = %peer_cert_hash,
        "queued key-share consent request"
    );
}

/// Resolve a human-readable peer name from their cert hash.
fn resolve_peer_name(state: &SharedState, peer_cert_hash: &str) -> String {
    state
        .users
        .values()
        .find(|u| u.hash.as_deref() == Some(peer_cert_hash))
        .map(|u| u.name.clone())
        .unwrap_or_else(|| peer_cert_hash.chars().take(8).collect())
}

/// Emit `pchat-key-share-requests-changed` for a channel.
fn emit_key_share_requests_changed(state: &SharedState, channel_id: u32) {
    if let Some(ref app) = state.conn.tauri_app_handle {
        use tauri::Emitter;
        let remaining: Vec<_> = state
            .pchat_ctx.pending_key_shares
            .iter()
            .filter(|p| p.channel_id == channel_id)
            .cloned()
            .collect();
        let _ = app.emit(
            "pchat-key-share-requests-changed",
            types::KeyShareRequestsChangedPayload {
                channel_id,
                pending: remaining,
            },
        );
    }
}
