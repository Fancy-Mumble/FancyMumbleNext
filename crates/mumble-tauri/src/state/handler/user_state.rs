use std::sync::{Arc, Mutex};

use mumble_protocol::command;
use mumble_protocol::persistent::PchatProtocol;
use mumble_protocol::persistent::KeyTrustLevel;
use mumble_protocol::proto::mumble_tcp;
use tracing::info;

use super::{HandleMessage, HandlerContext};
use crate::state::{pchat, SharedState};
use crate::state::types::{CurrentChannelPayload, ServerLogEntry, UserEntry};

impl HandleMessage for mumble_tcp::UserState {
    fn handle(&self, ctx: &HandlerContext) {
        let Some(session) = self.session else { return };

        let (is_synced, own_channel_changed, remote_channel_move, is_new_user, user_name, old_snapshot, new_snapshot, move_channel_name) = {
            let mut state_guard = ctx.shared.lock().ok();
            if let Some(ref mut state) = state_guard {
                let resolver = state.pchat_ctx.hash_name_resolver.clone();
                let is_new_user = !state.users.contains_key(&session);

                let old_snapshot = state.users.get(&session).map(|u| MuteDeafSnapshot {
                    mute: u.mute, deaf: u.deaf, self_mute: u.self_mute, self_deaf: u.self_deaf,
                });

                let user = state.users.entry(session).or_insert_with(|| UserEntry::new(session));
                apply_user_state_fields(user, self);

                if let (Some(ref hash), name) = (&user.hash, &user.name) {
                    maybe_record_name(&resolver, hash, name);
                }

                let (user_name, new_snapshot) = snapshot_user(user);

                // Apply channel move and drop the borrow on `user` before
                // touching state.current_channel.
                let channel_move = self.channel_id.map(|ch| {
                    let prev = user.channel_id;
                    user.channel_id = ch;
                    (ch, prev)
                });

                let (own_ch, remote_ch) = if let Some((ch, prev)) = channel_move {
                    set_channel_outcome(
                        state.conn.own_session, session, ch, prev, is_new_user, &mut state.current_channel,
                    )
                } else {
                    (false, None)
                };

                let move_channel_name = self.channel_id
                    .filter(|_| !is_new_user)
                    .and_then(|ch| state.channels.get(&ch))
                    .map(|c| c.name.clone());

                (state.conn.synced, own_ch, remote_ch, is_new_user, user_name, old_snapshot, new_snapshot, move_channel_name)
            } else {
                (false, false, None, false, String::new(), None, MuteDeafSnapshot::default(), None)
            }
        };

        emit_activity_logs(ctx, is_synced, &user_name, is_new_user, move_channel_name, old_snapshot, &new_snapshot);

        if is_synced {
            if let Some(ch) = remote_channel_move {
                handle_remote_channel_move(&ctx.shared, ch);
            }
        }

        if own_channel_changed {
            if let Some(ch) = self.channel_id {
                handle_own_channel_change(ctx, ch);
            }
        }

        if is_synced {
            request_missing_blobs(ctx, self, session);
            ctx.emit_empty("state-changed");
        }
    }
}

fn apply_user_state_fields(user: &mut UserEntry, proto: &mumble_tcp::UserState) {
    if let Some(ref name) = proto.name { user.name = name.clone(); }
    if let Some(ref texture) = proto.texture { user.texture = (!texture.is_empty()).then(|| texture.clone()); }
    if let Some(ref comment) = proto.comment { user.comment = (!comment.is_empty()).then(|| comment.clone()); }
    if let Some(mute) = proto.mute { user.mute = mute; }
    if let Some(deaf) = proto.deaf { user.deaf = deaf; }
    if let Some(suppress) = proto.suppress { user.suppress = suppress; }
    if let Some(self_mute) = proto.self_mute { user.self_mute = self_mute; }
    if let Some(self_deaf) = proto.self_deaf { user.self_deaf = self_deaf; }
    if let Some(priority) = proto.priority_speaker { user.priority_speaker = priority; }
    if let Some(ref hash) = proto.hash { user.hash = Some(hash.clone()); }
    if !proto.client_features.is_empty() { user.client_features = proto.client_features.clone(); }
    if let Some(uid) = proto.user_id { user.user_id = Some(uid); }
}

fn snapshot_user(user: &UserEntry) -> (String, MuteDeafSnapshot) {
    (
        user.name.clone(),
        MuteDeafSnapshot { mute: user.mute, deaf: user.deaf, self_mute: user.self_mute, self_deaf: user.self_deaf },
    )
}

fn emit_activity_logs(
    ctx: &HandlerContext,
    is_synced: bool,
    user_name: &str,
    is_new_user: bool,
    move_channel_name: Option<String>,
    old_snapshot: Option<MuteDeafSnapshot>,
    new_snapshot: &MuteDeafSnapshot,
) {
    if !is_synced || user_name.is_empty() { return; }
    let mut logs: Vec<ServerLogEntry> = Vec::new();
    if is_new_user {
        logs.push(ServerLogEntry::now(format!("{user_name} connected")));
    }
    if let Some(ch_name) = move_channel_name {
        logs.push(ServerLogEntry::now(format!("{user_name} moved to {ch_name}")));
    }
    build_mute_deaf_log(user_name, old_snapshot, new_snapshot, &mut logs);
    for entry in logs {
        ctx.emit("server-log", entry);
    }
}

fn handle_remote_channel_move(shared: &Arc<Mutex<SharedState>>, ch: u32) {
    pchat::check_key_share_for_channel(shared, ch);
    pchat::query_key_holders(shared, ch);

    let is_signal_v1 = shared
        .lock()
        .ok()
        .and_then(|s| s.channels.get(&ch).and_then(|c| c.pchat_protocol))
        == Some(PchatProtocol::SignalV1);
    if is_signal_v1 {
        pchat::send_signal_distribution(shared, ch);
    }
}

fn handle_own_channel_change(ctx: &HandlerContext, ch: u32) {
    ctx.emit("current-channel-changed", CurrentChannelPayload { channel_id: ch });

    #[cfg(target_os = "android")]
    {
        use tauri::Manager;
        let info = ctx.shared.lock().ok().and_then(|s| {
            let channel_name = s.channels.get(&ch).map(|c| c.name.clone())?;
            let host = s.server.host.clone();
            let app = s.conn.tauri_app_handle.clone()?;
            Some((app, host, channel_name))
        });
        if let Some((app, host, channel_name)) = info {
            if let Some(handle) =
                app.try_state::<crate::platform::android::connection_service::ConnectionServiceHandle>()
            {
                crate::platform::android::connection_service::update_service_channel(&handle, &host, &channel_name);
            }
        }
    }

    let should_fetch = should_fetch_pchat_history(&ctx.shared, ch);
    if should_fetch {
        mark_channel_fetched(&ctx.shared, ch);
        let shared = Arc::clone(&ctx.shared);
        let _pchat_init_task = tokio::spawn(pchat_init_task(shared, ch));
    }
}

fn request_missing_blobs(ctx: &HandlerContext, proto: &mumble_tcp::UserState, session: u32) {
    let need_texture = proto.texture_hash.is_some() && proto.texture.is_none();
    let need_comment = proto.comment_hash.is_some() && proto.comment.is_none();
    if need_texture || need_comment {
        let shared = Arc::clone(&ctx.shared);
        let _blob_task = tokio::spawn(request_user_blob(shared, session, need_texture, need_comment));
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

#[derive(Default)]
struct MuteDeafSnapshot {
    mute: bool,
    deaf: bool,
    self_mute: bool,
    self_deaf: bool,
}

fn build_mute_deaf_log(
    name: &str,
    old: Option<MuteDeafSnapshot>,
    new: &MuteDeafSnapshot,
    logs: &mut Vec<ServerLogEntry>,
) {
    let Some(old) = old else { return };
    if name.is_empty() {
        return;
    }
    // Server-side mute/deaf (admin action).
    if old.mute != new.mute {
        let action = if new.mute { "muted" } else { "unmuted" };
        logs.push(ServerLogEntry::now(format!("{name} was {action} by the server")));
    }
    if old.deaf != new.deaf {
        let action = if new.deaf { "deafened" } else { "undeafened" };
        logs.push(ServerLogEntry::now(format!("{name} was {action} by the server")));
    }
    // Self-mute/deaf.
    if old.self_mute != new.self_mute {
        let action = if new.self_mute { "muted" } else { "unmuted" };
        logs.push(ServerLogEntry::now(format!("{name} self-{action}")));
    }
    if old.self_deaf != new.self_deaf {
        let action = if new.self_deaf { "deafened" } else { "undeafened" };
        logs.push(ServerLogEntry::now(format!("{name} self-{action}")));
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
    let already_fetched = s.pchat_ctx.pchat.as_ref().is_some_and(|p| p.fetched_channels.contains(&ch));
    s.pchat_ctx.pchat.is_some() && mode.is_some_and(|m| m.is_encrypted()) && !already_fetched
}

fn mark_channel_fetched(shared: &Arc<Mutex<SharedState>>, ch: u32) {
    let Ok(mut state) = shared.lock() else { return };
    if let Some(ref mut pchat) = state.pchat_ctx.pchat {
        let _ = pchat.fetched_channels.insert(ch);
    }
}

fn maybe_derive_archive_key_for_join(
    shared: &Arc<Mutex<SharedState>>,
    ch: u32,
) -> Option<(std::path::PathBuf, [u8; 32], String)> {
    let Ok(mut s) = shared.lock() else { return None };
    let p = s.pchat_ctx.pchat.as_mut()?;
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
    let p = s.pchat_ctx.pchat.as_mut()?;
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
            s.pchat_ctx.pchat.as_ref().is_some_and(|p| pchat_mode.is_some_and(|m| p.key_manager.has_key(ch, m)))
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
            s.pchat_ctx.pchat.as_ref().map(|p| pchat_mode.map(|m| !p.key_manager.has_key(ch, m)).unwrap_or(false)).unwrap_or(false)
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

    let handle = shared.lock().ok().and_then(|s| s.conn.client_handle.clone());
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
    let Some(handle) = shared.lock().ok().and_then(|s| s.conn.client_handle.clone()) else { return };
    let _ = handle
        .send(command::RequestBlob {
            session_texture: if need_texture { vec![sess] } else { Vec::new() },
            session_comment: if need_comment { vec![sess] } else { Vec::new() },
            channel_description: Vec::new(),
        })
        .await;
}
