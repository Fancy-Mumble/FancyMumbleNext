use std::sync::{Arc, Mutex};

use mumble_protocol::command;
use mumble_protocol::persistent::{KeyTrustLevel, PchatProtocol};
use mumble_protocol::proto::mumble_tcp;
use tracing::{debug, info};

use super::{HandleMessage, HandlerContext};
use crate::state::{SharedState, types::ChannelEntry};

impl HandleMessage for mumble_tcp::ChannelState {
    #[allow(clippy::too_many_lines, reason = "channel state handler covers sync, description fetch, pchat mode changes, and custodian events")]
    fn handle(&self, ctx: &HandlerContext) {
        let Some(id) = self.channel_id else { return };

        let (is_synced, needs_description, pchat_changed_for_current, custodian_event, _is_new_channel) = {
            let mut state_guard = ctx.shared.lock().ok();
            if let Some(ref mut state) = state_guard {
                let is_new = state.synced && !state.channels.contains_key(&id);
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
                if let Some(parent) = self.parent {
                    ch.parent_id = Some(parent);
                }
                if let Some(ref name) = self.name {
                    ch.name = name.clone();
                }
                if let Some(ref desc) = self.description {
                    ch.description = desc.clone();
                }
                if let Some(ref hash) = self.description_hash {
                    ch.description_hash = Some(hash.clone());
                }
                if let Some(temp) = self.temporary {
                    ch.temporary = temp;
                }
                if let Some(pos) = self.position {
                    ch.position = pos;
                }
                if let Some(max) = self.max_users {
                    ch.max_users = max;
                }
                let mut mode_changed = false;
                if let Some(mode) = self.pchat_protocol {
                    let new_mode = PchatProtocol::from_proto(mode);
                    let old_mode = ch.pchat_protocol;
                    ch.pchat_protocol = Some(new_mode);
                    mode_changed = old_mode != Some(new_mode);
                }
                if let Some(max_hist) = self.pchat_max_history {
                    ch.pchat_max_history = Some(max_hist);
                }
                if let Some(ret) = self.pchat_retention_days {
                    ch.pchat_retention_days = Some(ret);
                }
                // Update key custodian list from proto.
                if !self.pchat_key_custodians.is_empty() || ch.pchat_key_custodians != self.pchat_key_custodians {
                    ch.pchat_key_custodians = self.pchat_key_custodians.clone();
                }
                let new_custodians = ch.pchat_key_custodians.clone();
                let needs_desc =
                    ch.description.is_empty() && ch.description_hash.is_some() && state.synced;
                // ch borrow ends here — safe to borrow state immutably
                if mode_changed {
                    debug!(
                        channel_id = id,
                        is_current = (state.current_channel == Some(id)),
                        has_pchat = state.pchat.is_some(),
                        "pchat: channel mode changed"
                    );
                }
                // Update custodian TOFU pin in key manager.
                let cust_event = state.pchat.as_mut().and_then(|pchat| {
                    let changed = pchat.key_manager.update_custodian_pin(id, new_custodians);
                    changed.then(|| pchat.key_manager.get_custodian_pin(id).cloned()).flatten()
                });
                let is_current = state.current_channel == Some(id);
                (state.synced, needs_desc, mode_changed && is_current, cust_event, is_new)
            } else {
                (false, false, false, None, false)
            }
        };

        // Emit custodian-pin-changed event when the pin state changes.
        if let Some(pin) = custodian_event {
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
            let app = ctx.shared.lock().ok().and_then(|s| s.tauri_app_handle.clone());
            if let Some(app) = app {
                let _ = app.emit("custodian-pin-changed", CustodianPinPayload {
                    channel_id: id,
                    pin: CustodianPinPayloadInner {
                        pinned: pin.pinned,
                        confirmed: pin.confirmed,
                        pending_update: pin.pending_update,
                    },
                });
            }
        }

        // When the pchat mode changes on our current channel to an encrypted mode,
        // trigger key generation and fetch.
        if pchat_changed_for_current {
            debug!(channel_id = id, "pchat: mode changed on current channel, spawning key-gen + fetch");
            let shared = Arc::clone(&ctx.shared);
            let _pchat_key_gen_task = tokio::spawn(pchat_key_gen_and_fetch(shared, id));
        }

        // Request the full description blob if only a hash
        // was provided (large descriptions are deferred).
        if needs_description {
            let shared = Arc::clone(&ctx.shared);
            let _description_fetch_task = tokio::spawn(async move {
                let handle = shared.lock().ok().and_then(|s| s.client_handle.clone());
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

        // When a channel state changes, re-query its permissions
        // so the cached bitmask stays up-to-date (ACL changes, etc.).
        if is_synced {
            let shared = Arc::clone(&ctx.shared);
            let _permissions_query_task = tokio::spawn(async move {
                let handle = {
                    let state = shared.lock().ok();
                    state.and_then(|s| s.client_handle.clone())
                };
                if let Some(handle) = handle {
                    let _ = handle
                        .send(command::PermissionQuery { channel_id: id })
                        .await;
                }
            });
            ctx.emit_empty("state-changed");
        }
    }
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
        .and_then(|s| s.pchat.as_ref().map(|p| !p.key_manager.has_key(id, mode)))
        .unwrap_or(false);

    if needs_key {
        debug!(channel_id = id, ?mode, "pchat: generating key for channel after mode change");
        derive_and_store_archive_key(&shared, id);
    }

    let should_fetch = shared
        .lock()
        .ok()
        .and_then(|s| s.pchat.as_ref().map(|p| !p.fetched_channels.contains(&id)))
        .unwrap_or(false);

    if should_fetch {
        debug!(channel_id = id, "pchat: sending fetch after mode change");
        if let Ok(mut s) = shared.lock() {
            if let Some(ref mut p) = s.pchat {
                let _ = p.fetched_channels.insert(id);
            }
        }
        let fetch = mumble_tcp::PchatFetch {
            channel_id: Some(id),
            before_id: None,
            limit: Some(50),
            after_id: None,
        };
        let handle = shared.lock().ok().and_then(|s| s.client_handle.clone());
        if let Some(handle) = handle {
            let _ = handle.send(command::SendPchatFetch { fetch }).await;
            debug!(channel_id = id, "sent pchat-fetch after mode change");
        }
    }
}

fn derive_and_store_archive_key(shared: &Arc<Mutex<SharedState>>, id: u32) {
    let Ok(mut s) = shared.lock() else { return };
    let m = s.channels.get(&id).and_then(|c| c.pchat_protocol);
    let Some(ref mut pchat) = s.pchat else { return };
    let cert = pchat.own_cert_hash.clone();
    if let Some(PchatProtocol::FancyV1FullArchive) = m {
        let key = mumble_protocol::persistent::encryption::derive_archive_key(&pchat.seed, id);
        pchat.key_manager.store_archive_key(id, key, KeyTrustLevel::Verified);
        pchat.key_manager.set_channel_originator(id, cert.clone());
        info!(channel_id = id, "derived archive key after mode change");
    }
}
