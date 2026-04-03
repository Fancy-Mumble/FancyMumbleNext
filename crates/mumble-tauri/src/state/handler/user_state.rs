use std::sync::Arc;

use mumble_protocol::command;
use mumble_protocol::persistent::PchatProtocol;
use mumble_protocol::persistent::KeyTrustLevel;
use mumble_protocol::proto::mumble_tcp;
use tracing::info;

use super::{HandleMessage, HandlerContext};
use crate::state::pchat;
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
                    user.texture = if texture.is_empty() {
                        None
                    } else {
                        Some(texture.clone())
                    };
                }
                if let Some(ref comment) = self.comment {
                    user.comment = if comment.is_empty() {
                        None
                    } else {
                        Some(comment.clone())
                    };
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
                    if !hash.is_empty() && !name.is_empty() {
                        if let Some(ref resolver) = resolver {
                            resolver.record(hash, name);
                        }
                    }
                }

                let mut own_ch = false;
                let mut remote_ch: Option<u32> = None;
                if let Some(ch) = self.channel_id {
                    let prev_channel = user.channel_id;
                    user.channel_id = ch;
                    // Track when our own user moves channels.
                    if state.own_session == Some(session) {
                        state.current_channel = Some(ch);
                        own_ch = true;
                    } else if is_new_user || ch != prev_channel {
                        // Trigger re-evaluation when a new remote peer appears
                        // or when one moves to a different channel.
                        remote_ch = Some(ch);
                    }
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
                let should_fetch = {
                    let state = ctx.shared.lock().ok();
                    if let Some(ref s) = state {
                        let mode = s
                            .channels
                            .get(&ch)
                            .and_then(|c| c.pchat_protocol);
                        let has_pchat = s.pchat.is_some();
                        let already_fetched = s
                            .pchat
                            .as_ref()
                            .is_some_and(|p| p.fetched_channels.contains(&ch));
                        has_pchat
                            && mode.is_some_and(|m| m.is_encrypted())
                            && !already_fetched
                    } else {
                        false
                    }
                };

                if should_fetch {
                    // Mark as fetched and send the request
                    if let Ok(mut state) = ctx.shared.lock() {
                        if let Some(ref mut pchat) = state.pchat {
                            let _ = pchat.fetched_channels.insert(ch);
                        }
                    }

                    let shared = Arc::clone(&ctx.shared);
                    let _pchat_init_task = tokio::spawn(async move {
                        // Notify frontend that history loading has started.
                        pchat::emit_history_loading(&shared, ch, true);

                        // For FullArchive, derive the key immediately (deterministic
                        // from seed) so we can skip the 2-second peer-exchange wait.
                        // If an archive key was restored from disk on init,
                        // has_key() will already be true and derivation is skipped.
                        {
                            let mode = {
                                let s = shared.lock().ok();
                                s.as_ref().and_then(|s| {
                                    s.channels.get(&ch).and_then(|c| c.pchat_protocol)
                                })
                            };
                            if mode == Some(PchatProtocol::FancyV1FullArchive) {
                                use mumble_protocol::persistent::KeyTrustLevel;
                                let persist_info = {
                                    if let Ok(mut s) = shared.lock() {
                                        if let Some(ref mut p) = s.pchat {
                                            if !p.key_manager.has_key(ch, PchatProtocol::FancyV1FullArchive) {
                                                let cert = p.own_cert_hash.clone();
                                                let key = mumble_protocol::persistent::encryption::derive_archive_key(&p.seed, ch);
                                                p.key_manager.store_archive_key(ch, key, KeyTrustLevel::Verified);
                                                p.key_manager.set_channel_originator(ch, cert.clone());
                                                info!(channel_id = ch, cert_hash = %cert, "derived archive key immediately on join");
                                                p.identity_dir.clone().map(|dir| (dir, key, cert))
                                            } else {
                                                None
                                            }
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                };
                                if let Some((dir, key, cert)) = persist_info {
                                    pchat::persist_archive_key(&dir, ch, &key, Some(&cert));
                                }
                                pchat::send_key_holder_report_async(&shared, ch).await;
                            }

                            // For SignalV1, load the bridge and create our sender
                            // key distribution immediately.
                            if mode == Some(PchatProtocol::SignalV1) {
                                let bridge_ok = if let Ok(mut s) = shared.lock() {
                                    s.pchat.as_mut().is_some_and(pchat::ensure_signal_bridge)
                                } else {
                                    false
                                };
                                if bridge_ok {
                                    pchat::send_signal_distribution(&shared, ch);
                                    pchat::send_key_holder_report_async(&shared, ch).await;
                                } else {
                                    pchat::emit_signal_bridge_error(
                                        &shared,
                                        "Signal bridge library could not be loaded. End-to-end encryption is unavailable.",
                                    );
                                }
                            }
                        }

                        // Check if we already have a key for this channel.
                        let already_has_key = {
                            let s = shared.lock().ok();
                            if let Some(ref s) = s {
                                let mode = s
                                    .channels
                                    .get(&ch)
                                    .and_then(|c| c.pchat_protocol);
                                if let Some(ref pchat) = s.pchat {
                                    mode.is_some_and(|m| pchat.key_manager.has_key(ch, m))
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        };

                        if already_has_key {
                            tracing::debug!(channel_id = ch, "pchat: key already exists, skipping 2s wait");
                        } else {
                            // Wait for potential key-exchange responses from other
                            // members, then self-generate the channel key if nobody
                            // sent us one (we are the originator).
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        }

                        let needs_key = {
                            let s = shared.lock().ok();
                            if let Some(ref s) = s {
                                let mode = s
                                    .channels
                                    .get(&ch)
                                    .and_then(|c| c.pchat_protocol);
                                if let Some(ref pchat) = s.pchat {
                                    mode.map(|m| !pchat.key_manager.has_key(ch, m))
                                        .unwrap_or(false)
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        };
                        if needs_key {
                            let persist_info = if let Ok(mut s) = shared.lock() {
                                let mode = s
                                    .channels
                                    .get(&ch)
                                    .and_then(|c| c.pchat_protocol);
                                if let Some(ref mut pchat) = s.pchat {
                                    let cert = pchat.own_cert_hash.clone();
                                    match mode {
                                        Some(PchatProtocol::FancyV1FullArchive) => {
                                            let key = mumble_protocol::persistent::encryption::derive_archive_key(&pchat.seed, ch);
                                            pchat.key_manager.store_archive_key(
                                                ch,
                                                key,
                                                KeyTrustLevel::Verified,
                                            );
                                            pchat.key_manager.set_channel_originator(ch, cert.clone());
                                            info!(channel_id = ch, cert_hash = %cert, "derived archive key (originator)");
                                            pchat.identity_dir.clone().map(|dir| (dir, key, cert))
                                        }
                                        Some(PchatProtocol::FancyV1PostJoin) => {
                                            let key: [u8; 32] = rand::random();
                                            pchat.key_manager.store_epoch_key(
                                                ch,
                                                0,
                                                key,
                                                KeyTrustLevel::Verified,
                                            );
                                            pchat.key_manager.set_channel_originator(ch, cert.clone());
                                            info!(channel_id = ch, cert_hash = %cert, "self-generated epoch key (originator)");
                                            None
                                        }
                                        Some(PchatProtocol::SignalV1) => {
                                            // Bridge should already be loaded; this
                                            // is a fallback path.
                                            if !pchat::ensure_signal_bridge(pchat) {
                                                pchat::emit_signal_bridge_error(
                                                    &shared,
                                                    "Signal bridge library could not be loaded. End-to-end encryption is unavailable.",
                                                );
                                            }
                                            info!(channel_id = ch, "signal bridge ensured on join (fallback)");
                                            None
                                        }
                                        _ => None,
                                    }
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            if let Some((dir, key, cert)) = persist_info {
                                pchat::persist_archive_key(&dir, ch, &key, Some(&cert));
                            }
                            pchat::send_key_holder_report_async(&shared, ch).await;
                        }

                        // NOW send fetch -- key is guaranteed to exist
                        // (either from exchange or self-generation).
                        let handle = {
                            let state = shared.lock().ok();
                            state.as_ref().and_then(|s| s.client_handle.clone())
                        };

                        let fetch_sent = if let Some(handle) = handle {
                            let fetch = mumble_tcp::PchatFetch {
                                channel_id: Some(ch),
                                before_id: None,
                                limit: Some(50),
                                after_id: None,
                            };
                            if let Err(e) = handle
                                .send(command::SendPchatFetch { fetch })
                                .await
                            {
                                tracing::warn!("send pchat-fetch failed: {e}");
                                false
                            } else {
                                info!(channel_id = ch, "sent pchat-fetch on join");
                                true
                            }
                        } else {
                            false
                        };

                        // Safety-net timeout: if the server never replies with a
                        // PchatFetchResponse (e.g. the channel has no stored messages
                        // yet), the loading indicator would be stuck forever.
                        // The PchatFetchResponse handler clears it immediately when
                        // the server does respond, making this a no-op in that case.
                        if fetch_sent {
                            let shared_timeout = Arc::clone(&shared);
                            let _timeout_task = tokio::spawn(async move {
                                tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                                pchat::emit_history_loading(&shared_timeout, ch, false);
                            });
                        } else {
                            // Fetch could not be sent -- clear immediately so the
                            // UI is not stuck on "Loading message history...".
                            pchat::emit_history_loading(&shared, ch, false);
                        }
                    });
                }
            }
        }
        // Only notify frontend after initial sync is done.
        if is_synced {
            ctx.emit_empty("state-changed");
        }
    }
}
