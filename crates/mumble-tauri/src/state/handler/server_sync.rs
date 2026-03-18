use std::sync::Arc;

use mumble_protocol::command;
use mumble_protocol::persistent::PersistenceMode;
use mumble_protocol::proto::mumble_tcp;
use tracing::{debug, info, warn};

use super::{HandleMessage, HandlerContext};
use crate::state::pchat;
use crate::state::types::{ConnectionStatus, CurrentChannelPayload};

impl HandleMessage for mumble_tcp::ServerSync {
    fn handle(&self, ctx: &HandlerContext) {
        let sessions: Vec<u32>;
        let initial_channel: Option<u32>;
        {
            let Ok(mut state) = ctx.shared.lock() else {
                return;
            };
            state.status = ConnectionStatus::Connected;
            state.own_session = self.session;
            state.synced = true;
            state.max_bandwidth = self.max_bandwidth;
            state.welcome_text = self.welcome_text.clone();

            // Now that we know our session, look up the channel
            // from UserState messages that arrived before ServerSync.
            initial_channel = self
                .session
                .and_then(|s| state.users.get(&s))
                .map(|u| u.channel_id);
            if let Some(ch) = initial_channel {
                state.current_channel = Some(ch);
            }

            // Collect all user sessions so we can request their
            // texture + comment blobs from the server.
            sessions = state.users.keys().copied().collect();
        }
        ctx.emit_empty("server-connected");

        // Notify frontend about the initial channel assignment.
        if let Some(ch) = initial_channel {
            ctx.emit(
                "current-channel-changed",
                CurrentChannelPayload { channel_id: ch },
            );
        }

        // Request full texture & comment blobs for every user.
        if !sessions.is_empty() {
            let shared = Arc::clone(&ctx.shared);
            tokio::spawn(async move {
                let handle = {
                    let state = shared.lock().ok();
                    state.and_then(|s| s.client_handle.clone())
                };
                if let Some(handle) = handle {
                    let _ = handle
                        .send(command::RequestBlob {
                            session_texture: sessions.clone(),
                            session_comment: sessions,
                            channel_description: Vec::new(),
                        })
                        .await;
                }
            });
        }

        // Request full description blobs for channels that only
        // have a description_hash (the server omits large
        // descriptions during initial sync).
        {
            let channel_ids_needing_desc: Vec<u32>;
            {
                let state = ctx.shared.lock().ok();
                channel_ids_needing_desc = state
                    .map(|s| {
                        s.channels
                            .values()
                            .filter(|ch| {
                                ch.description.is_empty() && ch.description_hash.is_some()
                            })
                            .map(|ch| ch.id)
                            .collect()
                    })
                    .unwrap_or_default();
            }
            if !channel_ids_needing_desc.is_empty() {
                let shared = Arc::clone(&ctx.shared);
                tokio::spawn(async move {
                    let handle = {
                        let state = shared.lock().ok();
                        state.and_then(|s| s.client_handle.clone())
                    };
                    if let Some(handle) = handle {
                        let _ = handle
                            .send(command::RequestBlob {
                                session_texture: Vec::new(),
                                session_comment: Vec::new(),
                                channel_description: channel_ids_needing_desc,
                            })
                            .await;
                    }
                });
            }
        }

        // Request permissions for all known channels so the UI
        // can grey out actions the user is not allowed to perform.
        {
            let channel_ids: Vec<u32>;
            {
                let state = ctx.shared.lock().ok();
                channel_ids = state
                    .map(|s| s.channels.keys().copied().collect())
                    .unwrap_or_default();
            }
            let shared = Arc::clone(&ctx.shared);
            tokio::spawn(async move {
                let handle = {
                    let state = shared.lock().ok();
                    state.and_then(|s| s.client_handle.clone())
                };
                if let Some(handle) = handle {
                    for ch_id in channel_ids {
                        let _ = handle
                            .send(command::PermissionQuery { channel_id: ch_id })
                            .await;
                    }
                }
            });
        }

        // ---- Persistent chat: initialise PchatState and send key-announce ----
        {
            let (seed, cert_hash, handle, is_fancy) = {
                let state = ctx.shared.lock().ok();
                if let Some(ref s) = state {
                    let own_hash = s
                        .own_session
                        .and_then(|sess| s.users.get(&sess))
                        .and_then(|u| u.hash.clone());
                    (
                        s.pchat_seed,
                        own_hash,
                        s.client_handle.clone(),
                        s.server_fancy_version.is_some(),
                    )
                } else {
                    (None, None, None, false)
                }
            };

            debug!(
                has_seed = seed.is_some(),
                has_cert_hash = cert_hash.is_some(),
                has_handle = handle.is_some(),
                is_fancy = is_fancy,
                "pchat init check"
            );

            if let (Some(seed), Some(cert_hash), Some(_handle), true) =
                (seed, cert_hash, handle, is_fancy)
            {
                match pchat::PchatState::new(seed, cert_hash.clone()) {
                    Ok(pchat_state) => {
                        info!(cert_hash = %cert_hash, "pchat initialised");

                        // Store pchat state
                        if let Ok(mut state) = ctx.shared.lock() {
                            state.pchat = Some(pchat_state);
                        }

                        // Send key-announce asynchronously
                        let shared = Arc::clone(&ctx.shared);
                        tokio::spawn(async move {
                            let (announce_proto, cert, h) = {
                                let state = shared.lock().ok();
                                if let Some(ref s) = state {
                                    if let Some(ref p) = s.pchat {
                                        let wire_announce = p.key_manager.build_key_announce(
                                            &p.own_cert_hash,
                                            pchat::now_millis(),
                                        );
                                        let proto = pchat::wire_key_announce_to_proto(&wire_announce);
                                        (
                                            Some(proto),
                                            Some(p.own_cert_hash.clone()),
                                            s.client_handle.clone(),
                                        )
                                    } else {
                                        (None, None, None)
                                    }
                                } else {
                                    (None, None, None)
                                }
                            };

                            if let (Some(proto), Some(cert), Some(handle)) =
                                (announce_proto, cert, h)
                            {
                                if let Err(e) = handle
                                    .send(command::SendPchatKeyAnnounce { announce: proto })
                                    .await
                                {
                                    warn!("failed to send key-announce: {e}");
                                } else {
                                    info!(cert_hash = %cert, "sent pchat key-announce");
                                }
                            }

                            // Fetch history + self-generate key for our initial channel.
                            debug!("pchat: looking up initial channel and mode");
                            let (ch, mode) = {
                                let s = shared.lock().ok();
                                if let Some(ref s) = s {
                                    let ch = s.current_channel;
                                    let mode = ch.and_then(|c| {
                                        s.channels.get(&c).and_then(|ce| ce.pchat_mode)
                                    }).map(PersistenceMode::from);
                                    (ch, mode)
                                } else {
                                    (None, None)
                                }
                            };

                            debug!(
                                channel = ?ch,
                                mode = ?mode,
                                "pchat: initial channel/mode resolved"
                            );

                            if let (Some(ch), Some(mode)) = (ch, mode) {
                                if mode.is_encrypted() {
                                    // Notify frontend that history loading has started.
                                    pchat::emit_history_loading(&shared, ch, true);

                                    // For FullArchive, derive the key immediately (it's
                                    // deterministic from the seed) so we can skip the
                                    // 2-second peer-exchange wait.
                                    if mode == PersistenceMode::FullArchive {
                                        use mumble_protocol::persistent::KeyTrustLevel;
                                        if let Ok(mut s) = shared.lock() {
                                            if let Some(ref mut p) = s.pchat {
                                                if !p.key_manager.has_key(ch, mode) {
                                                    let cert = p.own_cert_hash.clone();
                                                    let key = mumble_protocol::persistent::encryption::derive_archive_key(&p.seed, ch);
                                                    p.key_manager.store_archive_key(ch, key, KeyTrustLevel::Verified);
                                                    p.key_manager.set_channel_originator(ch, cert.clone());
                                                    info!(channel_id = ch, cert_hash = %cert, "derived archive key immediately (no wait needed)");
                                                }
                                            }
                                        }
                                    }

                                    // Check if we already have a key.
                                    let already_has_key = {
                                        let s = shared.lock().ok();
                                        s.as_ref()
                                            .and_then(|s| s.pchat.as_ref())
                                            .is_some_and(|p| p.key_manager.has_key(ch, mode))
                                    };

                                    if already_has_key {
                                        debug!(channel_id = ch, "pchat: key already exists, skipping 2s wait");
                                    } else {
                                        // Wait for key exchange from peers before
                                        // self-generating. This gives online members
                                        // time to respond to the server's key-request.
                                        debug!(channel_id = ch, ?mode, "pchat: waiting 2s for key-exchange before self-gen");
                                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                                    }

                                    let needs_key = {
                                        let s = shared.lock().ok();
                                        if let Some(ref s) = s {
                                            s.pchat.as_ref().map(|p| !p.key_manager.has_key(ch, mode)).unwrap_or(false)
                                        } else {
                                            false
                                        }
                                    };
                                    debug!(channel_id = ch, needs_key, "pchat: key check after 2s wait");
                                    if needs_key {
                                        use mumble_protocol::persistent::KeyTrustLevel;
                                        if let Ok(mut s) = shared.lock() {
                                            let mode = s.channels.get(&ch).and_then(|c| c.pchat_mode).map(PersistenceMode::from);
                                            if let Some(ref mut p) = s.pchat {
                                                let cert = p.own_cert_hash.clone();
                                                match mode {
                                                    Some(PersistenceMode::FullArchive) => {
                                                        let key = mumble_protocol::persistent::encryption::derive_archive_key(&p.seed, ch);
                                                        p.key_manager.store_archive_key(ch, key, KeyTrustLevel::Verified);
                                                        p.key_manager.set_channel_originator(ch, cert.clone());
                                                        info!(channel_id = ch, cert_hash = %cert, "derived archive key on initial join");
                                                    }
                                                    Some(PersistenceMode::PostJoin) => {
                                                        let key: [u8; 32] = rand::random();
                                                        p.key_manager.store_epoch_key(ch, 0, key, KeyTrustLevel::Verified);
                                                        p.key_manager.set_channel_originator(ch, cert.clone());
                                                        info!(channel_id = ch, cert_hash = %cert, "self-generated epoch key on initial join");
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }
                                    }

                                    // NOW fetch history — key is guaranteed to exist.
                                    debug!(channel_id = ch, "pchat: about to send pchat-fetch");
                                    {
                                        let s = shared.lock().ok();
                                        if let Some(ref s) = s {
                                            if let Some(ref p) = s.pchat {
                                                let has = p.key_manager.has_key(ch, mode);
                                                debug!(channel_id = ch, has_key = has, "pchat: key state before fetch");
                                            }
                                        }
                                    }
                                    if let Ok(mut s) = shared.lock() {
                                        if let Some(ref mut p) = s.pchat {
                                            p.fetched_channels.insert(ch);
                                        }
                                    }
                                    let fetch = mumble_tcp::PchatFetch {
                                        channel_id: Some(ch),
                                        before_id: None,
                                        limit: Some(50),
                                        after_id: None,
                                    };
                                    let handle = shared.lock().ok().and_then(|s| s.client_handle.clone());
                                    if let Some(handle) = handle {
                                        let _ = handle.send(command::SendPchatFetch { fetch }).await;
                                        info!(channel_id = ch, "sent initial pchat-fetch");
                                    }

                                    // NOTE: emit_history_loading(false) is NOT called here.
                                    // It will be emitted by the PchatFetchResponse handler
                                    // once messages are actually ready for display.
                                }
                            }
                        });
                    }
                    Err(e) => {
                        warn!("failed to init pchat: {e}");
                    }
                }
            } else {
                info!("pchat not initialised (no seed, cert hash, or non-fancy server)");
            }
        }
    }
}
