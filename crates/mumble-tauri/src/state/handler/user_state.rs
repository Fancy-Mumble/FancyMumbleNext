use std::sync::Arc;

use mumble_protocol::command;
use mumble_protocol::persistent::PersistenceMode;
use mumble_protocol::persistent::KeyTrustLevel;
use mumble_protocol::proto::mumble_tcp;
use tracing::info;

use super::{HandleMessage, HandlerContext};
use crate::state::pchat;
use crate::state::types::{CurrentChannelPayload, UserEntry};

impl HandleMessage for mumble_tcp::UserState {
    fn handle(&self, ctx: &HandlerContext) {
        let Some(session) = self.session else { return };

        let (is_synced, own_channel_changed) = {
            let mut state_guard = ctx.shared.lock().ok();
            if let Some(ref mut state) = state_guard {
                let user = state.users.entry(session).or_insert_with(|| UserEntry {
                    session,
                    name: String::new(),
                    channel_id: 0,
                    texture: None,
                    comment: None,
                    mute: false,
                    deaf: false,
                    suppress: false,
                    self_mute: false,
                    self_deaf: false,
                    priority_speaker: false,
                    hash: None,
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
                let mut own_ch = false;
                if let Some(ch) = self.channel_id {
                    user.channel_id = ch;
                    // Track when our own user moves channels.
                    if state.own_session == Some(session) {
                        state.current_channel = Some(ch);
                        own_ch = true;
                    }
                }
                (state.synced, own_ch)
            } else {
                (false, false)
            }
        };
        // Notify frontend about current-channel change.
        if own_channel_changed {
            if let Some(ch) = self.channel_id {
                ctx.emit(
                    "current-channel-changed",
                    CurrentChannelPayload { channel_id: ch },
                );

                // Send pchat-fetch for persistent channels (if not yet fetched).
                let should_fetch = {
                    let state = ctx.shared.lock().ok();
                    if let Some(ref s) = state {
                        let mode = s
                            .channels
                            .get(&ch)
                            .and_then(|c| c.pchat_mode)
                            .map(PersistenceMode::from);
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
                            pchat.fetched_channels.insert(ch);
                        }
                    }

                    let shared = Arc::clone(&ctx.shared);
                    tokio::spawn(async move {
                        // Notify frontend that history loading has started.
                        pchat::emit_history_loading(&shared, ch, true);

                        // For FullArchive, derive the key immediately (deterministic
                        // from seed) so we can skip the 2-second peer-exchange wait.
                        {
                            let mode = {
                                let s = shared.lock().ok();
                                s.as_ref().and_then(|s| {
                                    s.channels.get(&ch).and_then(|c| c.pchat_mode).map(PersistenceMode::from)
                                })
                            };
                            if mode == Some(PersistenceMode::FullArchive) {
                                use mumble_protocol::persistent::KeyTrustLevel;
                                if let Ok(mut s) = shared.lock() {
                                    if let Some(ref mut p) = s.pchat {
                                        if !p.key_manager.has_key(ch, PersistenceMode::FullArchive) {
                                            let cert = p.own_cert_hash.clone();
                                            let key = mumble_protocol::persistent::encryption::derive_archive_key(&p.seed, ch);
                                            p.key_manager.store_archive_key(ch, key, KeyTrustLevel::Verified);
                                            p.key_manager.set_channel_originator(ch, cert.clone());
                                            info!(channel_id = ch, cert_hash = %cert, "derived archive key immediately on join");
                                        }
                                    }
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
                                    .and_then(|c| c.pchat_mode)
                                    .map(PersistenceMode::from);
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
                                    .and_then(|c| c.pchat_mode)
                                    .map(PersistenceMode::from);
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
                            if let Ok(mut s) = shared.lock() {
                                let mode = s
                                    .channels
                                    .get(&ch)
                                    .and_then(|c| c.pchat_mode)
                                    .map(PersistenceMode::from);
                                if let Some(ref mut pchat) = s.pchat {
                                    let cert = pchat.own_cert_hash.clone();
                                    match mode {
                                        Some(PersistenceMode::FullArchive) => {
                                            let key = mumble_protocol::persistent::encryption::derive_archive_key(&pchat.seed, ch);
                                            pchat.key_manager.store_archive_key(
                                                ch,
                                                key,
                                                KeyTrustLevel::Verified,
                                            );
                                            pchat.key_manager.set_channel_originator(ch, cert.clone());
                                            info!(channel_id = ch, cert_hash = %cert, "derived archive key (originator)");
                                        }
                                        Some(PersistenceMode::PostJoin) => {
                                            let key: [u8; 32] = rand::random();
                                            pchat.key_manager.store_epoch_key(
                                                ch,
                                                0,
                                                key,
                                                KeyTrustLevel::Verified,
                                            );
                                            pchat.key_manager.set_channel_originator(ch, cert.clone());
                                            info!(channel_id = ch, cert_hash = %cert, "self-generated epoch key (originator)");
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }

                        // NOW send fetch -- key is guaranteed to exist
                        // (either from exchange or self-generation).
                        let handle = {
                            let state = shared.lock().ok();
                            state.as_ref().and_then(|s| s.client_handle.clone())
                        };

                        if let Some(handle) = handle {
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
                            } else {
                                info!(channel_id = ch, "sent pchat-fetch on join");
                            }
                        }

                        // NOTE: emit_history_loading(false) is NOT called here.
                        // It will be emitted by the PchatFetchResponse handler
                        // once messages are actually ready for display.
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
