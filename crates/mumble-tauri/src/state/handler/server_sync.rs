use std::collections::HashMap;
use std::sync::Arc;

use mumble_protocol::command;
use mumble_protocol::persistent::PchatProtocol;
use mumble_protocol::proto::mumble_tcp;
use tracing::{debug, info, warn};

use super::{HandleMessage, HandlerContext};
use crate::state::pchat;
use crate::state::types::{ConnectionStatus, CurrentChannelPayload};

impl HandleMessage for mumble_tcp::ServerSync {
    #[allow(clippy::too_many_lines, reason = "server sync handler bootstraps full connection state: user blobs, channel descriptions, permissions, and pchat init")]
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

            // `ServerSync.permissions` carries the permission bitmask for
            // the root channel (channel 0).  Store it as both the root
            // channel entry AND as a fallback for channels that never
            // receive a dedicated `PermissionQuery` response.
            // The Mumble server skips per-channel PermissionQuery replies
            // for SuperUser (user_id == 0) since they always have All
            // permissions; the fallback covers that case.
            if let Some(perms) = self.permissions {
                let perms_u32 = perms as u32;
                state.root_permissions = Some(perms_u32);
                info!(
                    permissions_hex = format!("0x{perms_u32:08X}"),
                    "ServerSync root channel permissions (stored as fallback)"
                );
                if let Some(ch) = state.channels.get_mut(&0) {
                    ch.permissions = Some(perms_u32);
                }
            } else {
                debug!("ServerSync has no permissions field");
            }

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

        // Start Android foreground service to keep the process alive
        // when the app is backgrounded (prevents Doze from killing the
        // TCP connection).
        #[cfg(target_os = "android")]
        {
            use tauri::Manager;
            let (app_handle, host, channel_name) = {
                let state = ctx.shared.lock().ok();
                state
                    .map(|s| {
                        let ch_name = initial_channel
                            .and_then(|ch| s.channels.get(&ch))
                            .map(|c| c.name.clone());
                        (s.tauri_app_handle.clone(), s.connected_host.clone(), ch_name)
                    })
                    .unwrap_or_default()
            };
            if let Some(app) = app_handle {
                if let Some(handle) =
                    app.try_state::<crate::connection_service::ConnectionServiceHandle>()
                {
                    crate::connection_service::start_service(&handle, &host);
                    if let Some(ref ch_name) = channel_name {
                        crate::connection_service::update_service_channel(
                            &handle, &host, ch_name,
                        );
                    }
                }

                // Register FCM device token with the server so it can
                // send permission-checked push notifications directly
                // to this device (instead of broadcasting via topics).
                if let Some(fcm) = app.try_state::<crate::fcm_service::FcmPluginHandle>() {
                    info!("FCM: FcmPluginHandle found, requesting device token");
                    match crate::fcm_service::get_token(&fcm) {
                        Some(token) => {
                            info!(len = token.len(), "FCM: device token obtained, sending push registration");
                            let client_handle = ctx.shared.lock().ok().and_then(|s| s.client_handle.clone());
                            if let Some(handle) = client_handle {
                                let payload = serde_json::json!({ "token": token });
                                let data = serde_json::to_vec(&payload).unwrap_or_default();
                                let _push_register_task = tokio::spawn(async move {
                                    match handle
                                        .send(command::SendPluginData {
                                            receiver_sessions: vec![],
                                            data,
                                            data_id: "fancy-push-register".to_string(),
                                        })
                                        .await
                                    {
                                        Ok(_) => info!("FCM: push registration sent to server"),
                                        Err(e) => warn!("FCM: failed to send push registration: {e}"),
                                    }
                                });
                            } else {
                                warn!("FCM: no client handle, cannot send push registration");
                            }
                        }
                        None => warn!("FCM: no device token available, skipping push registration"),
                    }
                } else {
                    info!("FCM: FcmPluginHandle not available (not Android?)");
                }
            }
        }

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
            let _blob_request_task = tokio::spawn(async move {
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
                let _desc_blob_task = tokio::spawn(async move {
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
            let _permissions_task = tokio::spawn(async move {
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
            let (seed, cert_hash, handle, is_fancy, id_dir) = {
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
                        s.pchat_identity_dir.clone(),
                    )
                } else {
                    (None, None, None, false, None)
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
                match pchat::PchatState::new(seed, cert_hash.clone(), id_dir) {
                    Ok(mut pchat_state) => {
                        // Restore archive keys persisted from a previous session
                        // so the client can decrypt history without a new key exchange.
                        if let Some(ref dir) = pchat_state.identity_dir {
                            let persisted = pchat::load_persisted_archive_keys(dir);
                            for (ch, key, originator) in persisted {
                                use mumble_protocol::persistent::KeyTrustLevel;
                                pchat_state.key_manager.store_archive_key(
                                    ch,
                                    key,
                                    KeyTrustLevel::Verified,
                                );
                                if let Some(orig) = originator {
                                    pchat_state
                                        .key_manager
                                        .set_channel_originator(ch, orig);
                                }
                                info!(
                                    channel_id = ch,
                                    "restored persisted archive key"
                                );
                            }
                        }

                        // Load the encrypted local cache BEFORE acquiring the
                        // lock.  cache.load() does file I/O + AES-GCM decryption
                        // which can take hundreds of milliseconds.  Doing it
                        // outside the lock prevents stalling frontend commands
                        // (get_channels, get_users, etc.) that need the same
                        // Mutex.
                        let cached_messages = {
                            if let Some(ref mut cache) = pchat_state.local_cache {
                                if let Err(e) = cache.load() {
                                    warn!("failed to load local message cache: {e}");
                                    HashMap::new()
                                } else {
                                    let cached = cache.all_chat_messages();
                                    for (ch_id, msgs) in &cached {
                                        if !msgs.is_empty() {
                                            info!(
                                                channel_id = ch_id,
                                                count = msgs.len(),
                                                "restored cached messages"
                                            );
                                        }
                                    }
                                    cached
                                }
                            } else {
                                HashMap::new()
                            }
                        };

                        info!(cert_hash = %cert_hash, "pchat initialised");

                        // Store pchat state and insert cached messages.
                        // The lock is only held for the in-memory insertion.
                        if let Ok(mut state) = ctx.shared.lock() {
                            state.pchat = Some(pchat_state);

                            for (ch_id, msgs) in cached_messages {
                                if !msgs.is_empty() {
                                    state.messages
                                        .entry(ch_id)
                                        .or_default()
                                        .extend(msgs);
                                }
                            }
                        }

                        // Send key-announce asynchronously
                        let shared = Arc::clone(&ctx.shared);
                        let _key_announce_task = tokio::spawn(async move {
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
                                        s.channels.get(&c).and_then(|ce| ce.pchat_protocol)
                                    });
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
                                    if mode == PchatProtocol::FancyV1FullArchive {
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
                                        pchat::send_key_holder_report_async(&shared, ch).await;
                                    }

                                    // For SignalV1, load the bridge and create our sender
                                    // key distribution immediately (no peer exchange needed).
                                    if mode == PchatProtocol::SignalV1 {
                                        let bridge_ok = pchat::ensure_signal_bridge_unlocked(&shared);
                                        if bridge_ok {
                                            pchat::send_signal_distribution(&shared, ch);
                                            pchat::send_key_holder_report_async(&shared, ch).await;
                                        } else {
                                            pchat::emit_signal_bridge_error(
                                                &shared,
                                                "Signal bridge library could not be loaded. End-to-end encryption is unavailable.",
                                            );
                                            // Cannot decrypt without the bridge -- clear
                                            // loading and skip the rest of the init flow.
                                            pchat::emit_history_loading(&shared, ch, false);
                                            return;
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
                                            let mode = s.channels.get(&ch).and_then(|c| c.pchat_protocol);
                                            if let Some(ref mut p) = s.pchat {
                                                let cert = p.own_cert_hash.clone();
                                                match mode {
                                                    Some(PchatProtocol::FancyV1FullArchive) => {
                                                        let key = mumble_protocol::persistent::encryption::derive_archive_key(&p.seed, ch);
                                                        p.key_manager.store_archive_key(ch, key, KeyTrustLevel::Verified);
                                                        p.key_manager.set_channel_originator(ch, cert.clone());
                                                        info!(channel_id = ch, cert_hash = %cert, "derived archive key on initial join");
                                                    }
                                                    Some(PchatProtocol::FancyV1PostJoin) => {
                                                        let key: [u8; 32] = rand::random();
                                                        p.key_manager.store_epoch_key(ch, 0, key, KeyTrustLevel::Verified);
                                                        p.key_manager.set_channel_originator(ch, cert.clone());
                                                        info!(channel_id = ch, cert_hash = %cert, "self-generated epoch key on initial join");
                                                    }
                                                    Some(PchatProtocol::SignalV1) => {
                                                        // Bridge should already be loaded from the
                                                        // immediate init above; this is a fallback.
                                                        if !p.ensure_signal_bridge() {
                                                            pchat::emit_signal_bridge_error(
                                                                &shared,
                                                                "Signal bridge library could not be loaded. End-to-end encryption is unavailable.",
                                                            );
                                                        }
                                                        info!(channel_id = ch, "signal bridge ensured on initial join (fallback)");
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }
                                        pchat::send_key_holder_report_async(&shared, ch).await;
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
                                            let _ = p.fetched_channels.insert(ch);
                                        }
                                    }
                                    let fetch = mumble_tcp::PchatFetch {
                                        channel_id: Some(ch),
                                        before_id: None,
                                        limit: Some(50),
                                        after_id: None,
                                    };
                                    let handle = shared.lock().ok().and_then(|s| s.client_handle.clone());
                                    let fetch_sent = if let Some(handle) = handle {
                                        let _ = handle.send(command::SendPchatFetch { fetch }).await;
                                        info!(channel_id = ch, "sent initial pchat-fetch");
                                        true
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
