//! Persistent-chat custodian and key-share/key-takeover commands.

use crate::state::{self, AppState};

/// Confirm the initial custodian list for a channel (TOFU, Section 5.7).
#[tauri::command]
pub(crate) fn confirm_custodians(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
) -> Result<(), String> {
    let mut shared = state.inner.lock().map_err(|e| e.to_string())?;
    if let Some(ref mut pchat) = shared.pchat_ctx.pchat {
        pchat.key_manager.confirm_custodian_list(channel_id);
    }
    Ok(())
}

/// Accept a pending custodian list change for a channel (Section 5.7).
#[tauri::command]
pub(crate) fn accept_custodian_changes(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
) -> Result<(), String> {
    let mut shared = state.inner.lock().map_err(|e| e.to_string())?;
    if let Some(ref mut pchat) = shared.pchat_ctx.pchat {
        pchat.key_manager.accept_custodian_update(channel_id);
    }
    Ok(())
}

/// Approve a pending key-share request: actually send the encrypted
/// channel key to the peer that triggered the consent banner.
#[tauri::command]
pub(crate) async fn approve_key_share(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
    peer_cert_hash: String,
) -> Result<(), String> {
    use mumble_protocol::persistent::PchatProtocol;
    use std::time::{SystemTime, UNIX_EPOCH};

    // Extract everything we need while holding the lock, then release it.
    let (handle, exchange, share_requests_emit) = {
        let mut shared = state.inner.lock().map_err(|e| e.to_string())?;

        // Remove the pending entry and capture its request_id.
        let idx = shared
            .pchat_ctx.pending_key_shares
            .iter()
            .position(|p| p.channel_id == channel_id && p.peer_cert_hash == peer_cert_hash)
            .ok_or("no pending key share for this channel/peer")?;
        let removed = shared.pchat_ctx.pending_key_shares.remove(idx);
        let request_id = removed.request_id;

        // Collect payload for deferred emit outside the lock.
        let share_requests_emit = shared.conn.tauri_app_handle.as_ref().map(|app| {
            let remaining: Vec<_> = shared
                .pchat_ctx.pending_key_shares
                .iter()
                .filter(|p| p.channel_id == channel_id)
                .cloned()
                .collect();
            (
                app.clone(),
                state::types::KeyShareRequestsChangedPayload {
                    channel_id,
                    pending: remaining,
                },
            )
        });

        let pchat = shared.pchat_ctx.pchat.as_ref().ok_or("pchat not initialised")?;

        let peer_record = pchat
            .key_manager
            .get_peer(&peer_cert_hash)
            .ok_or("peer public key not known")?;
        let peer_x25519 = peer_record.dh_public;

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let mut wire_exchange = pchat
            .key_manager
            .distribute_key(
                channel_id,
                PchatProtocol::FancyV1FullArchive,
                0,
                &peer_cert_hash,
                &peer_x25519,
                request_id.as_deref(),
                now_ms,
            )
            .map_err(|e| format!("failed to build key exchange: {e}"))?;

        wire_exchange.sender_hash = pchat.own_cert_hash.clone();

        let proto =
            state::pchat::wire_key_exchange_to_proto(&wire_exchange);

        let handle = shared
            .conn.client_handle
            .clone()
            .ok_or("not connected")?;

        (handle, proto, share_requests_emit)
    };

    // Emit outside the lock to avoid deadlock with Tauri IPC.
    if let Some((app, payload)) = share_requests_emit {
        use tauri::Emitter;
        let _ = app.emit("pchat-key-share-requests-changed", payload);
    }

    // Send the key exchange to the peer.
    handle
        .send(mumble_protocol::command::SendPchatKeyExchange { exchange })
        .await
        .map_err(|e| format!("send failed: {e}"))?;

    // Record the peer as a key holder locally so we don't prompt consent
    // for them again on subsequent channel moves.
    if let Ok(mut shared) = state.inner.lock() {
        if let Some(ref mut pchat) = shared.pchat_ctx.pchat {
            pchat
                .key_manager
                .record_key_holder(channel_id, peer_cert_hash.clone());
        }
    }

    // Report to the server that the peer now holds the key.
    let report = mumble_protocol::proto::mumble_tcp::PchatKeyHolderReport {
        channel_id: Some(channel_id),
        cert_hash: Some(peer_cert_hash),
        takeover_mode: None,
    };
    let _ = handle
        .send(mumble_protocol::command::SendPchatKeyHolderReport { report })
        .await;

    Ok(())
}

/// Dismiss a pending key-share request without sending the key.
#[tauri::command]
pub(crate) fn dismiss_key_share(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
    peer_cert_hash: String,
) -> Result<(), String> {
    let share_requests_emit = {
        let mut shared = state.inner.lock().map_err(|e| e.to_string())?;

        shared
            .pchat_ctx.pending_key_shares
            .retain(|p| !(p.channel_id == channel_id && p.peer_cert_hash == peer_cert_hash));

        // Collect payload for deferred emit outside the lock.
        shared.conn.tauri_app_handle.as_ref().map(|app| {
            let remaining: Vec<_> = shared
                .pchat_ctx.pending_key_shares
                .iter()
                .filter(|p| p.channel_id == channel_id)
                .cloned()
                .collect();
            (
                app.clone(),
                state::types::KeyShareRequestsChangedPayload {
                    channel_id,
                    pending: remaining,
                },
            )
        })
    };

    // Emit outside the lock to avoid deadlock with Tauri IPC.
    if let Some((app, payload)) = share_requests_emit {
        use tauri::Emitter;
        let _ = app.emit("pchat-key-share-requests-changed", payload);
    }

    Ok(())
}

/// Ask the server for the list of key holders for a channel.
#[tauri::command]
pub(crate) async fn query_key_holders(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
) -> Result<(), String> {
    let handle = {
        let shared = state.inner.lock().map_err(|e| e.to_string())?;
        shared.conn.client_handle.clone().ok_or("not connected")?
    };
    let query = mumble_protocol::proto::mumble_tcp::PchatKeyHoldersQuery {
        channel_id: Some(channel_id),
    };
    handle
        .send(mumble_protocol::command::SendPchatKeyHoldersQuery { query })
        .await
        .map_err(|e| format!("send failed: {e}"))
}

/// Return the cached key holders for a channel (from the last server response).
#[tauri::command]
pub(crate) fn get_key_holders(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
) -> Vec<state::types::KeyHolderEntry> {
    let shared = state.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    shared.pchat_ctx.key_holders.get(&channel_id).cloned().unwrap_or_default()
}

/// Request a key-ownership takeover for a channel (requires `KeyOwner` permission).
///
/// `mode` must be `"full_wipe"` (delete messages + key takeover) or
/// `"key_only"` (key takeover without deleting messages).
///
/// On success the server responds with an updated `PchatKeyHoldersList`.
/// On failure the server sends `PermissionDenied`.
#[tauri::command]
pub(crate) async fn key_takeover(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
    mode: String,
) -> Result<(), String> {
    use mumble_protocol::proto::mumble_tcp::pchat_key_holder_report::KeyTakeoverMode;
    let takeover_mode = match mode.as_str() {
        "full_wipe" => KeyTakeoverMode::FullWipe,
        "key_only" => KeyTakeoverMode::KeyOnly,
        _ => return Err(format!("invalid takeover mode: {mode}")),
    };
    state::pchat::send_key_takeover(&state.inner, channel_id, takeover_mode);
    Ok(())
}
