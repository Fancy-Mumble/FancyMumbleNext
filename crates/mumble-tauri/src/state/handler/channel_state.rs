use std::sync::{Arc, Mutex};

use mumble_protocol::command;
use mumble_protocol::persistent::{KeyTrustLevel, PchatProtocol};
use mumble_protocol::proto::mumble_tcp;
use tracing::{debug, info};

use super::{HandleMessage, HandlerContext};
use crate::state::{SharedState, types::ChannelEntry};

impl HandleMessage for mumble_tcp::ChannelState {
    fn handle(&self, ctx: &HandlerContext) {
        let Some(id) = self.channel_id else { return };

        let (is_synced, needs_description, pchat_changed_for_current, custodian_event) = {
            let mut state_guard = ctx.shared.lock().ok();
            if let Some(ref mut state) = state_guard {
                let ch = state.channels.entry(id).or_insert_with(|| ChannelEntry {
                    id,
                    parent_id: None,
                    name: String::new(),
                    description: String::new(),
                    description_hash: None,
                    user_count: 0,
                    permissions: None,
                    temporary: false,
                    position: 0,
                    max_users: 0,
                    pchat_protocol: None,
                    pchat_max_history: None,
                    pchat_retention_days: None,
                    pchat_key_custodians: Vec::new(),
                });
                let mode_changed = apply_channel_state_fields(ch, self);
                let new_custodians = ch.pchat_key_custodians.clone();
                let needs_desc =
                    ch.description.is_empty() && ch.description_hash.is_some() && state.conn.synced;
                if mode_changed {
                    debug!(
                        channel_id = id,
                        is_current = (state.current_channel == Some(id)),
                        has_pchat = state.pchat_ctx.pchat.is_some(),
                        "pchat: channel mode changed"
                    );
                }
                let cust_event = state.pchat_ctx.pchat.as_mut().and_then(|pchat| {
                    let changed = pchat.key_manager.update_custodian_pin(id, new_custodians);
                    changed.then(|| pchat.key_manager.get_custodian_pin(id).cloned()).flatten()
                });
                let is_current = state.current_channel == Some(id);
                (state.conn.synced, needs_desc, mode_changed && is_current, cust_event)
            } else {
                (false, false, false, None)
            }
        };

        if let Some(pin) = custodian_event {
            emit_custodian_pin_changed(ctx, id, pin);
        }

        if pchat_changed_for_current {
            debug!(channel_id = id, "pchat: mode changed on current channel, spawning key-gen + fetch");
            let shared = Arc::clone(&ctx.shared);
            let _pchat_key_gen_task = tokio::spawn(pchat_key_gen_and_fetch(shared, id));
        }

        if needs_description {
            spawn_description_fetch(Arc::clone(&ctx.shared), id);
        }

        if is_synced {
            spawn_permissions_refresh(Arc::clone(&ctx.shared), id);
            ctx.emit_empty("state-changed");
        }
    }
}

fn apply_channel_state_fields(ch: &mut ChannelEntry, proto: &mumble_tcp::ChannelState) -> bool {
    if let Some(parent) = proto.parent {
        ch.parent_id = Some(parent);
    }
    if let Some(ref name) = proto.name {
        ch.name = name.clone();
    }
    if let Some(ref desc) = proto.description {
        ch.description = desc.clone();
    }
    if let Some(ref hash) = proto.description_hash {
        ch.description_hash = Some(hash.clone());
    }
    if let Some(temp) = proto.temporary {
        ch.temporary = temp;
    }
    if let Some(pos) = proto.position {
        ch.position = pos;
    }
    if let Some(max) = proto.max_users {
        ch.max_users = max;
    }
    let mut mode_changed = false;
    if let Some(mode) = proto.pchat_protocol {
        let new_mode = PchatProtocol::from_proto(mode);
        let old_mode = ch.pchat_protocol;
        ch.pchat_protocol = Some(new_mode);
        mode_changed = old_mode != Some(new_mode);
    }
    if let Some(max_hist) = proto.pchat_max_history {
        ch.pchat_max_history = Some(max_hist);
    }
    if let Some(ret) = proto.pchat_retention_days {
        ch.pchat_retention_days = Some(ret);
    }
    if !proto.pchat_key_custodians.is_empty() || ch.pchat_key_custodians != proto.pchat_key_custodians {
        ch.pchat_key_custodians = proto.pchat_key_custodians.clone();
    }
    mode_changed
}

fn emit_custodian_pin_changed(
    ctx: &HandlerContext,
    channel_id: u32,
    pin: mumble_protocol::persistent::keys::CustodianPinState,
) {
    use serde::Serialize;
    use tauri::Emitter;
    #[derive(Serialize, Clone)]
    struct CustodianPinPayload {
        channel_id: u32,
        pin: CustodianPinPayloadInner,
    }
    #[derive(Serialize, Clone)]
    #[serde(rename_all = "camelCase")]
    struct CustodianPinPayloadInner {
        pinned: Vec<String>,
        confirmed: bool,
        pending_update: Option<Vec<String>>,
    }
    let app = ctx.shared.lock().ok().and_then(|s| s.conn.tauri_app_handle.clone());
    if let Some(app) = app {
        let _ = app.emit("custodian-pin-changed", CustodianPinPayload {
            channel_id,
            pin: CustodianPinPayloadInner {
                pinned: pin.pinned,
                confirmed: pin.confirmed,
                pending_update: pin.pending_update,
            },
        });
    }
}

fn spawn_description_fetch(shared: Arc<Mutex<SharedState>>, id: u32) {
    let _task = tokio::spawn(async move {
        let handle = shared.lock().ok().and_then(|s| s.conn.client_handle.clone());
        if let Some(handle) = handle {
            let _ = handle
                .send(command::RequestBlob {
                    session_texture: Vec::new(),
                    session_comment: Vec::new(),
                    channel_description: vec![id],
                })
                .await;
        }
    });
}

fn spawn_permissions_refresh(shared: Arc<Mutex<SharedState>>, id: u32) {
    let _task = tokio::spawn(async move {
        let handle = shared.lock().ok().and_then(|s| s.conn.client_handle.clone());
        if let Some(handle) = handle {
            let _ = handle
                .send(command::PermissionQuery { channel_id: id })
                .await;
        }
    });
}

async fn pchat_key_gen_and_fetch(shared: Arc<Mutex<SharedState>>, id: u32) {
    let mode = shared
        .lock()
        .ok()
        .and_then(|s| s.channels.get(&id).and_then(|c| c.pchat_protocol));
    let Some(mode) = mode else { return };
    if !mode.is_encrypted() {
        return;
    }

    let needs_key = shared
        .lock()
        .ok()
        .and_then(|s| s.pchat_ctx.pchat.as_ref().map(|p| !p.key_manager.has_key(id, mode)))
        .unwrap_or(false);

    if needs_key {
        debug!(channel_id = id, ?mode, "pchat: generating key for channel after mode change");
        derive_and_store_archive_key(&shared, id);
    }

    let should_fetch = shared
        .lock()
        .ok()
        .and_then(|s| s.pchat_ctx.pchat.as_ref().map(|p| !p.fetched_channels.contains(&id)))
        .unwrap_or(false);

    if should_fetch {
        debug!(channel_id = id, "pchat: sending fetch after mode change");
        if let Ok(mut s) = shared.lock() {
            if let Some(ref mut p) = s.pchat_ctx.pchat {
                let _ = p.fetched_channels.insert(id);
            }
        }
        let fetch = mumble_tcp::PchatFetch {
            channel_id: Some(id),
            before_id: None,
            limit: Some(50),
            after_id: None,
        };
        let handle = shared.lock().ok().and_then(|s| s.conn.client_handle.clone());
        if let Some(handle) = handle {
            let _ = handle.send(command::SendPchatFetch { fetch }).await;
            debug!(channel_id = id, "sent pchat-fetch after mode change");
        }
    }
}

fn derive_and_store_archive_key(shared: &Arc<Mutex<SharedState>>, id: u32) {
    let Ok(mut s) = shared.lock() else { return };
    let m = s.channels.get(&id).and_then(|c| c.pchat_protocol);
    let Some(ref mut pchat) = s.pchat_ctx.pchat else { return };
    let cert = pchat.own_cert_hash.clone();
    if let Some(PchatProtocol::FancyV1FullArchive) = m {
        let key = mumble_protocol::persistent::encryption::derive_archive_key(&pchat.seed, id);
        pchat.key_manager.store_archive_key(id, key, KeyTrustLevel::Verified);
        pchat.key_manager.set_channel_originator(id, cert.clone());
        info!(channel_id = id, "derived archive key after mode change");
    }
}
