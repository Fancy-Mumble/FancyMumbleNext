use std::sync::{Arc, Mutex};

use mumble_protocol::command;
use mumble_protocol::persistent::PchatProtocol;
use mumble_protocol::persistent::KeyTrustLevel;
use mumble_protocol::proto::mumble_tcp;
use tracing::info;

use super::{HandleMessage, HandlerContext};
use crate::state::{pchat, SharedState};
use crate::state::types::{CurrentChannelPayload, UserEntry};

impl HandleMessage for mumble_tcp::UserState {
    #[allow(clippy::too_many_lines, reason = "user state handler covers channel moves, profile updates, pchat key exchange, and history fetch")]
    fn handle(&self, ctx: &HandlerContext) {
        let Some(session) = self.session else { return };

        let (is_synced, own_channel_changed, remote_channel_move) = {
            let mut state_guard = ctx.shared.lock().ok();
            if let Some(ref mut state) = state_guard {
                let resolver = state.hash_name_resolver.clone();
                let is_new_user = !state.users.contains_key(&session);
                let user = state.users.entry(session).or_insert_with(|| UserEntry {
                    session,
                    name: String::new(),
                    channel_id: 0,
                    user_id: None,
                    texture: None,
                    comment: None,
                    mute: false,
                    deaf: false,
                    suppress: false,
                    self_mute: false,
                    self_deaf: false,
                    priority_speaker: false,
                    hash: None,
                    client_features: Vec::new(),
                });
                if let Some(ref name) = self.name {
                    user.name = name.clone();
                }
                if let Some(ref texture) = self.texture {
                    user.texture = (!texture.is_empty()).then(|| texture.clone());
                }
                if let Some(ref comment) = self.comment {
                    user.comment = (!comment.is_empty()).then(|| comment.clone());
                }
                if let Some(mute) = self.mute {
                    user.mute = mute;
                }
                if let Some(deaf) = self.deaf {
                    user.deaf = deaf;
                }
                if let Some(suppress) = self.suppress {
                    user.suppress = suppress;
                }
                if let Some(self_mute) = self.self_mute {
                    user.self_mute = self_mute;
                }
                if let Some(self_deaf) = self.self_deaf {
                    user.self_deaf = self_deaf;
                }
                if let Some(priority) = self.priority_speaker {
                    user.priority_speaker = priority;
                }
                if let Some(ref hash) = self.hash {
                    user.hash = Some(hash.clone());
                }
                if !self.client_features.is_empty() {
                    user.client_features = self.client_features.clone();
                }
                if let Some(uid) = self.user_id {
                    user.user_id = Some(uid);
                }

                // Persist cert_hash -> username mapping for offline display.
                if let (Some(ref hash), name) = (&user.hash, &user.name) {
                    maybe_record_name(&resolver, hash, name);
                }

                let mut own_ch = false;
                let mut remote_ch: Option<u32> = None;
                if let Some(ch) = self.channel_id {
                    let prev_channel = user.channel_id;
                    user.channel_id = ch;
                    let (o, r) = set_channel_outcome(
                        state.own_session,
                        session,
                        ch,
                        prev_channel,
                        is_new_user,
                        &mut state.current_channel,
                    );
                    own_ch = o;
                    remote_ch = r;
                }
                (state.synced, own_ch, remote_ch)
            } else {
                (false, false, None)
            }
        };

        // When a remote peer moves into a channel, re-evaluate whether
        // we should offer to share our channel key with them, then ask the
        // server for the latest key-holder list so stale prompts are dismissed
        // if the peer already has the key.
        if is_synced {
            if let Some(ch) = remote_channel_move {
                pchat::check_key_share_for_channel(&ctx.shared, ch);
                pchat::query_key_holders(&ctx.shared, ch);

                // For SignalV1 channels, re-send our sender key distribution
                // to the channel so the new peer can decrypt our messages.
                let is_signal_v1 = ctx
                    .shared
                    .lock()
                    .ok()
                    .and_then(|s| {
                        s.channels.get(&ch).and_then(|c| c.pchat_protocol)
                    })
                    == Some(PchatProtocol::SignalV1);
                if is_signal_v1 {
                    pchat::send_signal_distribution(&ctx.shared, ch);
                }
            }
        }

        // Notify frontend about current-channel change.
        if own_channel_changed {
            if let Some(ch) = self.channel_id {
                ctx.emit(
                    "current-channel-changed",
                    CurrentChannelPayload { channel_id: ch },
                );

                // Update the foreground-service notification to show the
                // current channel name alongside the server name.
                #[cfg(target_os = "android")]
                {
                    use tauri::Manager;
                    let info = ctx.shared.lock().ok().and_then(|s| {
                        let channel_name = s.channels.get(&ch).map(|c| c.name.clone())?;
                        let host = s.connected_host.clone();
                        let app = s.tauri_app_handle.clone()?;
                        Some((app, host, channel_name))
                    });
                    if let Some((app, host, channel_name)) = info {
                        if let Some(handle) =
                            app.try_state::<crate::connection_service::ConnectionServiceHandle>()
                        {
                            crate::connection_service::update_service_channel(
                                &handle,
                                &host,
                                &channel_name,
                            );
                        }
                    }
                }

                // Send pchat-fetch for persistent channels (if not yet fetched).
                let should_fetch = should_fetch_pchat_history(&ctx.shared, ch);

                if should_fetch {
                    mark_channel_fetched(&ctx.shared, ch);
                    let shared = Arc::clone(&ctx.shared);
                    let _pchat_init_task = tokio::spawn(pchat_init_task(shared, ch));
                }
            }
        }
        // If we received only a hash (no full payload), the server omitted
        // the large blob. Request it so we display the texture / comment.
        // During initial sync `request_user_blobs` handles this in bulk,
        // so we only fire individual blob requests for post-sync updates.
        if is_synced {
            let need_texture = self.texture_hash.is_some() && self.texture.is_none();
            let need_comment = self.comment_hash.is_some() && self.comment.is_none();
            if need_texture || need_comment {
                let shared = Arc::clone(&ctx.shared);
                let sess = session;
                let _blob_task = tokio::spawn(request_user_blob(shared, sess, need_texture, need_comment));
            }
        }

        // Only notify frontend after initial sync is done.
        if is_synced {
            ctx.emit_empty("state-changed");
        }
    }
}

fn maybe_record_name(
    resolver: &Option<Arc<dyn crate::state::hash_names::HashNameResolver>>,
    hash: &str,
    name: &str,
) {
    if hash.is_empty() || name.is_empty() {
        return;
    }
    if let Some(ref r) = resolver {
        r.record(hash, name);
    }
}

fn set_channel_outcome(
    own_session: Option<u32>,
    session: u32,
    ch: u32,
    prev_channel: u32,
    is_new_user: bool,
    current_channel: &mut Option<u32>,
) -> (bool, Option<u32>) {
    if own_session == Some(session) {
        *current_channel = Some(ch);
        (true, None)
    } else if is_new_user || ch != prev_channel {
        (false, Some(ch))
    } else {
        (false, None)
    }
}

fn should_fetch_pchat_history(shared: &Arc<Mutex<SharedState>>, ch: u32) -> bool {
    let Ok(s) = shared.lock() else { return false };
    let mode = s.channels.get(&ch).and_then(|c| c.pchat_protocol);
    let already_fetched = s.pchat.as_ref().is_some_and(|p| p.fetched_channels.contains(&ch));
    s.pchat.is_some() && mode.is_some_and(|m| m.is_encrypted()) && !already_fetched
}

fn mark_channel_fetched(shared: &Arc<Mutex<SharedState>>, ch: u32) {
    let Ok(mut state) = shared.lock() else { return };
    if let Some(ref mut pchat) = state.pchat {
        let _ = pchat.fetched_channels.insert(ch);
    }
}

fn maybe_derive_archive_key_for_join(
    shared: &Arc<Mutex<SharedState>>,
    ch: u32,
) -> Option<(std::path::PathBuf, [u8; 32], String)> {
    let Ok(mut s) = shared.lock() else { return None };
    let p = s.pchat.as_mut()?;
    if p.key_manager.has_key(ch, PchatProtocol::FancyV1FullArchive) {
        return None;
    }
    let cert = p.own_cert_hash.clone();
    let key = mumble_protocol::persistent::encryption::derive_archive_key(&p.seed, ch);
    p.key_manager.store_archive_key(ch, key, KeyTrustLevel::Verified);
    p.key_manager.set_channel_originator(ch, cert.clone());
    info!(channel_id = ch, cert_hash = %cert, "derived archive key immediately on join");
    p.identity_dir.clone().map(|dir| (dir, key, cert))
}

fn derive_channel_key_as_originator(
    shared: &Arc<Mutex<SharedState>>,
    ch: u32,
) -> Option<(std::path::PathBuf, [u8; 32], String)> {
    let Ok(mut s) = shared.lock() else { return None };
    let mode = s.channels.get(&ch).and_then(|c| c.pchat_protocol);
    let p = s.pchat.as_mut()?;
    let cert = p.own_cert_hash.clone();
    match mode {
        Some(PchatProtocol::FancyV1FullArchive) => {
            let key = mumble_protocol::persistent::encryption::derive_archive_key(&p.seed, ch);
            p.key_manager.store_archive_key(ch, key, KeyTrustLevel::Verified);
            p.key_manager.set_channel_originator(ch, cert.clone());
            info!(channel_id = ch, cert_hash = %cert, "derived archive key (originator)");
            p.identity_dir.clone().map(|dir| (dir, key, cert))
        }
        Some(PchatProtocol::SignalV1) => {
            if !p.ensure_signal_bridge() {
                pchat::emit_signal_bridge_error(
                    shared,
                    "Signal bridge library could not be loaded. End-to-end encryption is unavailable.",
                );
            }
            info!(channel_id = ch, "signal bridge ensured on join (fallback)");
            None
        }
        _ => None,
    }
}

async fn pchat_init_task(shared: Arc<Mutex<SharedState>>, ch: u32) {
    pchat::emit_history_loading(&shared, ch, true);

    let mode = shared
        .lock()
        .ok()
        .and_then(|s| s.channels.get(&ch).and_then(|c| c.pchat_protocol));

    if mode == Some(PchatProtocol::FancyV1FullArchive) {
        let persist_info = maybe_derive_archive_key_for_join(&shared, ch);
        if let Some((dir, key, cert)) = persist_info {
            pchat::persist_archive_key(&dir, ch, &key, Some(&cert));
        }
        pchat::send_key_holder_report_async(&shared, ch).await;
    }

    if mode == Some(PchatProtocol::SignalV1) {
        let bridge_ok = pchat::ensure_signal_bridge_unlocked(&shared);
        if bridge_ok {
            pchat::send_signal_distribution(&shared, ch);
            pchat::send_key_holder_report_async(&shared, ch).await;
        } else {
            pchat::emit_signal_bridge_error(
                &shared,
                "Signal bridge library could not be loaded. End-to-end encryption is unavailable.",
            );
            pchat::emit_history_loading(&shared, ch, false);
            return;
        }
    }

    let already_has_key = {
        let s = shared.lock().ok();
        if let Some(ref s) = s {
            let pchat_mode = s.channels.get(&ch).and_then(|c| c.pchat_protocol);
            s.pchat.as_ref().is_some_and(|p| pchat_mode.is_some_and(|m| p.key_manager.has_key(ch, m)))
        } else {
            false
        }
    };

    if already_has_key {
        tracing::debug!(channel_id = ch, "pchat: key already exists, skipping 2s wait");
    } else {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    let needs_key = {
        let s = shared.lock().ok();
        if let Some(ref s) = s {
            let pchat_mode = s.channels.get(&ch).and_then(|c| c.pchat_protocol);
            s.pchat.as_ref().map(|p| pchat_mode.map(|m| !p.key_manager.has_key(ch, m)).unwrap_or(false)).unwrap_or(false)
        } else {
            false
        }
    };

    if needs_key {
        let persist_info = derive_channel_key_as_originator(&shared, ch);
        if let Some((dir, key, cert)) = persist_info {
            pchat::persist_archive_key(&dir, ch, &key, Some(&cert));
        }
        pchat::send_key_holder_report_async(&shared, ch).await;
    }

    let handle = shared.lock().ok().and_then(|s| s.client_handle.clone());
    let fetch_sent = if let Some(handle) = handle {
        let fetch = mumble_tcp::PchatFetch {
            channel_id: Some(ch),
            before_id: None,
            limit: Some(50),
            after_id: None,
        };
        if let Err(e) = handle.send(command::SendPchatFetch { fetch }).await {
            tracing::warn!("send pchat-fetch failed: {e}");
            false
        } else {
            info!(channel_id = ch, "sent pchat-fetch on join");
            true
        }
    } else {
        false
    };

    if fetch_sent {
        let shared_timeout = Arc::clone(&shared);
        let _timeout_task = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(15)).await;
            pchat::emit_history_loading(&shared_timeout, ch, false);
        });
    } else {
        pchat::emit_history_loading(&shared, ch, false);
    }
}

async fn request_user_blob(
    shared: Arc<Mutex<SharedState>>,
    sess: u32,
    need_texture: bool,
    need_comment: bool,
) {
    let Some(handle) = shared.lock().ok().and_then(|s| s.client_handle.clone()) else { return };
    let _ = handle
        .send(command::RequestBlob {
            session_texture: if need_texture { vec![sess] } else { Vec::new() },
            session_comment: if need_comment { vec![sess] } else { Vec::new() },
            channel_description: Vec::new(),
        })
        .await;
}
