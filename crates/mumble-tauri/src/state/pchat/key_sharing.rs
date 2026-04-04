//! Key sharing consent, key-possession challenges, and key holder
//! management (report, query, takeover).

use std::sync::{Arc, Mutex};

use tracing::{debug, warn};

use mumble_protocol::client::ClientHandle;
use mumble_protocol::command;
use mumble_protocol::proto::mumble_tcp;

use crate::state::types;
use crate::state::SharedState;

use super::persistence::delete_persisted_archive_key;

// -- Key challenge ----------------------------------------------------

pub(crate) fn handle_proto_key_challenge(
    shared: &Arc<Mutex<SharedState>>,
    msg: &mumble_tcp::PchatKeyChallenge,
) {
    let channel_id = msg.channel_id.unwrap_or(0);
    let challenge = msg.challenge.as_deref().unwrap_or_default();

    if challenge.is_empty() {
        warn!(channel_id, "received empty challenge from server, ignoring");
        return;
    }

    let (handle, proof) = {
        let s = shared.lock().ok();
        let h = s.as_ref().and_then(|s| s.client_handle.clone());
        let proof = s
            .as_ref()
            .and_then(|s| s.pchat.as_ref())
            .and_then(|p| p.key_manager.compute_challenge_proof(channel_id, challenge));
        (h, proof)
    };

    match (handle, proof) {
        (Some(handle), Some(proof)) => {
            debug!(channel_id, "responding to key-possession challenge");
            let _challenge_response_task = tokio::spawn(async move {
                let response = mumble_tcp::PchatKeyChallengeResponse {
                    channel_id: Some(channel_id),
                    proof: Some(proof.to_vec()),
                };
                if let Err(e) = handle
                    .send(command::SendPchatKeyChallengeResponse { response })
                    .await
                {
                    warn!(channel_id, "failed to send challenge response: {e}");
                }
            });
        }
        (_, None) => {
            warn!(
                channel_id,
                "no archive key for channel, cannot respond to challenge"
            );
        }
        (None, _) => {
            warn!("no client handle, cannot respond to challenge");
        }
    }
}

// -- Key challenge result ---------------------------------------------

/// Handle a `PchatKeyChallengeResult` from the server.
///
/// If `passed == true`, our key is verified and we are accepted as a holder.
/// If `passed == false`, we hold a wrong key: remove it from memory and disk.
pub(crate) fn handle_proto_key_challenge_result(
    shared: &Arc<Mutex<SharedState>>,
    msg: &mumble_tcp::PchatKeyChallengeResult,
) {
    let channel_id = msg.channel_id.unwrap_or(0);
    let passed = msg.passed.unwrap_or(false);

    if passed {
        debug!(channel_id, "key-possession challenge passed");
        return;
    }

    warn!(
        channel_id,
        "key-possession challenge FAILED - discarding archive key"
    );

    let (identity_dir, app) = {
        let mut s = shared.lock().ok();
        let dir = s
            .as_ref()
            .and_then(|s| s.pchat.as_ref())
            .and_then(|p| p.identity_dir.clone());
        let app_handle = s
            .as_ref()
            .and_then(|s| s.tauri_app_handle.clone());
        // Remove all keying material for the channel from memory.
        if let Some(ref mut s) = s {
            if let Some(ref mut pchat) = s.pchat {
                pchat.key_manager.remove_channel(channel_id);
            }
            let before_len = s.pending_key_shares.len();
            s.pending_key_shares
                .retain(|p| p.channel_id != channel_id);
            if s.pending_key_shares.len() != before_len {
                if let Some(ref app) = app_handle {
                    use tauri::Emitter;
                    let _ = app.emit(
                        "pchat-key-share-requests-changed",
                        types::KeyShareRequestsChangedPayload {
                            channel_id,
                            pending: vec![],
                        },
                    );
                }
            }
        }
        (dir, app_handle)
    };

    if let Some(dir) = identity_dir {
        delete_persisted_archive_key(&dir, channel_id);
    }

    if let Some(app) = app {
        use tauri::Emitter;
        let _ = app.emit(
            "pchat-key-revoked",
            types::PchatKeyRevokedPayload { channel_id },
        );
    }
}

// -- Key holder report ------------------------------------------------

/// Extract state needed for a key holder report and verify we hold a
/// usable key before reporting.
fn prepare_key_holder_report(
    shared: &Arc<Mutex<SharedState>>,
    channel_id: u32,
) -> Option<(ClientHandle, mumble_tcp::PchatKeyHolderReport)> {
    let (handle, hash) = {
        let mut s = shared.lock().ok();
        let h = s.as_ref().and_then(|s| s.client_handle.clone());
        let hash = s
            .as_ref()
            .and_then(|s| s.pchat.as_ref().map(|p| p.own_cert_hash.clone()));

        let mode = s.as_ref().and_then(|s| {
            s.channels
                .get(&channel_id)
                .and_then(|c| c.pchat_protocol)
        });
        if let (Some(ref s), Some(mode)) = (&s, mode) {
            if let Some(ref pchat) = s.pchat {
                if !pchat.key_manager.has_key(channel_id, mode) {
                    warn!(channel_id, ?mode, "not reporting as key holder: no usable key");
                    return None;
                }
            }
        }

        if let (Some(ref mut s), Some(ref hash)) = (&mut s, &hash) {
            if let Some(ref mut pchat) = s.pchat {
                pchat
                    .key_manager
                    .record_key_holder(channel_id, hash.clone());
            }
        }
        (h, hash)
    };
    match (handle, hash) {
        (Some(handle), Some(hash)) => {
            let report = mumble_tcp::PchatKeyHolderReport {
                channel_id: Some(channel_id),
                cert_hash: Some(hash),
                takeover_mode: None,
            };
            Some((handle, report))
        }
        _ => None,
    }
}

/// Report that we hold the E2EE key for a channel (async variant).
pub(crate) async fn send_key_holder_report_async(
    shared: &Arc<Mutex<SharedState>>,
    channel_id: u32,
) {
    if let Some((handle, report)) = prepare_key_holder_report(shared, channel_id) {
        if let Err(e) = handle
            .send(command::SendPchatKeyHolderReport { report })
            .await
        {
            warn!(channel_id, "failed to report key holder: {e}");
        } else {
            debug!(channel_id, "reported self as key holder");
        }
    }
}

/// Report that we hold the E2EE key for a channel (fire-and-forget).
pub(crate) fn send_key_holder_report(
    shared: &Arc<Mutex<SharedState>>,
    channel_id: u32,
) {
    if let Some((handle, report)) = prepare_key_holder_report(shared, channel_id) {
        let _key_holder_report_task = tokio::spawn(async move {
            if let Err(e) = handle
                .send(command::SendPchatKeyHolderReport { report })
                .await
            {
                warn!(channel_id, "failed to report key holder: {e}");
            } else {
                debug!(channel_id, "reported self as key holder");
            }
        });
    }
}

// -- Key takeover -----------------------------------------------------

/// Request a key-ownership takeover for a channel (requires `KeyOwner`
/// permission).
pub(crate) fn send_key_takeover(
    shared: &Arc<Mutex<SharedState>>,
    channel_id: u32,
    mode: mumble_tcp::pchat_key_holder_report::KeyTakeoverMode,
) {
    let (handle, hash) = {
        let s = shared.lock().ok();
        let h = s.as_ref().and_then(|s| s.client_handle.clone());
        let hash = s
            .as_ref()
            .and_then(|s| s.pchat.as_ref().map(|p| p.own_cert_hash.clone()));
        (h, hash)
    };
    let Some(handle) = handle else { return };
    let Some(hash) = hash else { return };

    let report = mumble_tcp::PchatKeyHolderReport {
        channel_id: Some(channel_id),
        cert_hash: Some(hash),
        takeover_mode: Some(mode as i32),
    };

    let _task = tokio::spawn(async move {
        if let Err(e) = handle
            .send(command::SendPchatKeyHolderReport { report })
            .await
        {
            warn!(channel_id, "failed to send key takeover: {e}");
        } else {
            debug!(channel_id, "sent key takeover");
        }
    });
}

// -- Key holders query ------------------------------------------------

/// Ask the server for the latest key holders of a channel.
pub(crate) fn query_key_holders(
    shared: &Arc<Mutex<SharedState>>,
    channel_id: u32,
) {
    let handle = {
        let Ok(state) = shared.lock() else { return };
        state.client_handle.clone()
    };
    let Some(handle) = handle else { return };
    let query = mumble_tcp::PchatKeyHoldersQuery {
        channel_id: Some(channel_id),
    };
    let _query_task = tokio::spawn(async move {
        if let Err(e) = handle
            .send(command::SendPchatKeyHoldersQuery { query })
            .await
        {
            warn!(channel_id, "failed to query key holders: {e}");
        }
    });
}
