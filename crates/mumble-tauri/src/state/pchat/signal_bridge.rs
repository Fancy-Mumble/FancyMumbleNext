//! Signal Protocol bridge loading, sender key distribution, and
//! stashed envelope retry.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tracing::{debug, info, warn};

use mumble_protocol::command;
use mumble_protocol::persistent::protocol::signal_v1::SignalBridge;
use mumble_protocol::persistent::wire::{MessageEnvelope, WireCodec};

use crate::state::local_cache::CachedMessage;
use crate::state::SharedState;

use super::persistence::load_signal_state;
use super::PchatState;

// -- Bridge loading ---------------------------------------------------

/// Attempt to load the Signal Protocol bridge DLL.
///
/// Searches for the platform-specific library name in several locations:
/// 1. Next to the executable (Windows installers, `AppImage`, dev mode)
/// 2. `../lib/fancy-mumble/` relative to the exe (Linux deb packages)
/// 3. Extra search directory (e.g. Android `nativeLibraryDir`)
/// 4. On Android, bare filename as fallback (`dlopen` resolves it)
///
/// Returns `None` (with a warning) if the library is not found anywhere.
pub(crate) fn load_signal_bridge(
    own_cert_hash: &str,
    extra_search_dir: Option<&Path>,
) -> Option<Arc<SignalBridge>> {
    let lib_name = if cfg!(windows) {
        "signal_bridge.dll"
    } else if cfg!(target_os = "macos") {
        "libsignal_bridge.dylib"
    } else {
        "libsignal_bridge.so"
    };

    let mut candidates: Vec<PathBuf> = Vec::new();

    #[cfg(not(target_os = "android"))]
    {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf));

        if let Some(ref dir) = exe_dir {
            candidates.push(dir.join(lib_name));
            candidates.push(dir.join("signal-bridge").join(lib_name));
            candidates.push(dir.join("../lib/fancy-mumble").join(lib_name));
            candidates.push(
                dir.join("../lib/fancy-mumble/signal-bridge")
                    .join(lib_name),
            );
        }
    }

    if let Some(dir) = extra_search_dir {
        candidates.push(dir.join(lib_name));
    }

    candidates.push(PathBuf::from(lib_name));

    info!(?candidates, "signal bridge: searching for library");

    #[cfg(target_os = "android")]
    {
        for candidate in &candidates {
            match SignalBridge::new(candidate, own_cert_hash) {
                Ok(bridge) => {
                    info!(?candidate, "loaded signal bridge");
                    return Some(Arc::new(bridge));
                }
                Err(e) => {
                    debug!(?candidate, "signal bridge candidate failed: {e}");
                }
            }
        }
        warn!(
            ?candidates,
            "signal bridge library not found; SignalV1 channels will not work"
        );
        return None;
    }

    #[cfg(not(target_os = "android"))]
    {
        let lib_path = candidates.iter().find(|p| p.exists());

        let Some(lib_path) = lib_path else {
            warn!(
                ?candidates,
                "signal bridge library not found; SignalV1 channels will not work"
            );
            return None;
        };

        match SignalBridge::new(lib_path, own_cert_hash) {
            Ok(bridge) => {
                info!(?lib_path, "loaded signal bridge");
                Some(Arc::new(bridge))
            }
            Err(e) => {
                warn!(?lib_path, "failed to load signal bridge: {e}");
                None
            }
        }
    }
}

// -- Ensure bridge is loaded (PchatState methods) ---------------------

impl PchatState {
    /// Ensure the signal bridge is loaded and wired into the key manager.
    ///
    /// Returns `true` if the bridge is loaded (or was already loaded),
    /// `false` if loading failed.
    pub(crate) fn ensure_signal_bridge(&mut self) -> bool {
        if self.signal_bridge.is_some() {
            return true;
        }
        if self.signal_bridge_load_failed {
            return false;
        }
        let bridge = load_signal_bridge(&self.own_cert_hash, None);
        if let Some(ref b) = bridge {
            self.key_manager.set_signal_bridge(Arc::clone(b));
            load_signal_state(self.identity_dir.as_deref(), b);
        } else {
            self.signal_bridge_load_failed = true;
        }
        let loaded = bridge.is_some();
        self.signal_bridge = bridge;
        loaded
    }
}

/// Lock-free variant of [`PchatState::ensure_signal_bridge`] for async
/// contexts.
///
/// Performs the potentially slow DLL search **outside** the `SharedState`
/// mutex, then briefly re-acquires the lock to store the result.
pub(crate) fn ensure_signal_bridge_unlocked(shared: &Arc<Mutex<SharedState>>) -> bool {
    // Fast path: already loaded or already failed.
    {
        let s = match shared.lock() {
            Ok(s) => s,
            Err(e) => {
                warn!("ensure_signal_bridge: mutex poisoned (fast path): {e}");
                return false;
            }
        };
        if let Some(pchat) = s.pchat.as_ref() {
            if pchat.signal_bridge.is_some() {
                return true;
            }
            if pchat.signal_bridge_load_failed {
                return false;
            }
        } else {
            warn!("ensure_signal_bridge: pchat state not initialised");
            return false;
        }
    }

    let (cert_hash, identity_dir) = {
        let s = match shared.lock() {
            Ok(s) => s,
            Err(e) => {
                warn!("ensure_signal_bridge: mutex poisoned (cert read): {e}");
                return false;
            }
        };
        match s.pchat.as_ref() {
            Some(p) => (p.own_cert_hash.clone(), p.identity_dir.clone()),
            None => return false,
        }
    };

    let bridge = load_signal_bridge(&cert_hash, None);

    if let Some(ref b) = bridge {
        load_signal_state(identity_dir.as_deref(), b);
    }

    let loaded = bridge.is_some();
    match shared.lock() {
        Ok(mut s) => {
            if let Some(ref mut pchat) = s.pchat {
                if let Some(ref b) = bridge {
                    pchat.key_manager.set_signal_bridge(Arc::clone(b));
                } else {
                    pchat.signal_bridge_load_failed = true;
                }
                pchat.signal_bridge = bridge;
            }
        }
        Err(e) => {
            warn!("ensure_signal_bridge: mutex poisoned (store): {e}");
        }
    }
    loaded
}

// -- Sender key distribution ------------------------------------------

/// Create our sender key distribution for a channel and send it to
/// the server via `PchatSenderKeyDistribution`.
///
/// The server stores the latest SKDM per (sender, channel) and relays
/// it to online members.  On offline queue drain the server bundles the
/// relevant distributions so reconnecting clients can decrypt.
pub(crate) fn send_signal_distribution(
    shared: &Arc<Mutex<SharedState>>,
    channel_id: u32,
) {
    {
        let s = shared.lock().ok();
        let bridge_unavailable = s
            .as_ref()
            .and_then(|s| s.pchat.as_ref())
            .is_some_and(|p| p.signal_bridge_load_failed);
        if bridge_unavailable {
            return;
        }
    }

    let (handle, distribution) = {
        let Ok(mut state) = shared.lock() else { return };

        let Some(ref mut pchat) = state.pchat else {
            return;
        };
        if !pchat.ensure_signal_bridge() {
            warn!(channel_id, "cannot send signal distribution: bridge not loaded");
            return;
        }
        let Some(ref bridge) = pchat.signal_bridge else {
            warn!(channel_id, "cannot send signal distribution: bridge not loaded");
            return;
        };

        let dist = match bridge.create_distribution(channel_id) {
            Ok(d) => d,
            Err(e) => {
                warn!(channel_id, "create_distribution failed: {e}");
                return;
            }
        };

        (state.client_handle.clone(), dist)
    };

    let Some(handle) = handle else { return };

    let _dist_task = tokio::spawn(async move {
        if let Err(e) = handle
            .send(command::SendPchatSenderKeyDistribution {
                channel_id,
                distribution,
            })
            .await
        {
            warn!(channel_id, "failed to send signal distribution: {e}");
        } else {
            debug!(channel_id, "sent signal sender key distribution");
        }
    });
}

// -- Stashed envelope retry -------------------------------------------

/// Retry decrypting stashed `SignalV1` envelopes from `sender_hash`.
///
/// Called after successfully processing a sender key distribution.
fn retry_stashed_signal_envelopes(
    state: &mut SharedState,
    sender_hash: &str,
    sender_channel: u32,
) -> usize {
    let decoded: Vec<(String, u32, String, String)> = {
        let Some(pchat) = state.pchat.as_mut() else {
            return 0;
        };

        let mut remaining = Vec::new();
        let mut matched = Vec::new();
        for env in pchat.pending_signal_envelopes.drain(..) {
            if env.sender_hash == sender_hash && env.channel_id == sender_channel {
                matched.push(env);
            } else {
                remaining.push(env);
            }
        }
        pchat.pending_signal_envelopes = remaining;

        if matched.is_empty() {
            return 0;
        }

        debug!(
            sender = %sender_hash,
            channel_id = sender_channel,
            count = matched.len(),
            "retrying stashed signal envelopes after distribution"
        );

        let mut results = Vec::new();
        let mut still_pending = Vec::new();

        for env in matched {
            let decrypt_result = pchat
                .key_manager
                .decrypt_signal(&env.sender_hash, env.channel_id, &env.envelope_bytes);

            match decrypt_result {
                Ok(plaintext) => match pchat.codec.decode::<MessageEnvelope>(&plaintext) {
                    Ok(envelope) => {
                        pchat.cache_signal_message(CachedMessage {
                            message_id: env.message_id.clone(),
                            channel_id: env.channel_id,
                            timestamp: env.timestamp,
                            sender_hash: env.sender_hash.clone(),
                            sender_name: envelope.sender_name.clone(),
                            body: envelope.body.clone(),
                            is_own: false,
                        });
                        results.push((
                            env.message_id.clone(),
                            env.channel_id,
                            envelope.sender_name,
                            envelope.body,
                        ));
                    }
                    Err(e) => {
                        warn!(
                            message_id = %env.message_id,
                            "stashed envelope: failed to decode after decrypt: {e}"
                        );
                    }
                },
                Err(e) => {
                    warn!(
                        message_id = %env.message_id,
                        sender = %env.sender_hash,
                        "stashed envelope: still failed to decrypt, keeping stashed: {e}"
                    );
                    still_pending.push(env);
                }
            }
        }

        pchat.pending_signal_envelopes.extend(still_pending);

        results
    };

    let mut replaced_count = 0usize;
    for (message_id, channel_id, sender_name, body) in &decoded {
        let mid: &str = message_id;
        if let Some(msgs) = state.messages.get_mut(channel_id) {
            if let Some(msg) = msgs
                .iter_mut()
                .find(|m| m.message_id.as_deref() == Some(mid))
            {
                msg.body.clone_from(body);
                msg.sender_name.clone_from(sender_name);
                replaced_count += 1;
            }
        }
    }

    if replaced_count > 0 {
        debug!(
            replaced_count,
            sender = %sender_hash,
            channel_id = sender_channel,
            "replaced placeholder messages with decrypted content"
        );
    }

    replaced_count
}

// -- Handle incoming sender key --------------------------------------

/// Process a Signal sender key distribution identified by hash and channel.
///
/// Used by the `PchatSenderKeyDistribution` handler where the server
/// already provides `sender_hash` and `channel_id`.
/// Returns `true` if stashed envelopes were decrypted.
pub(crate) fn handle_signal_sender_key_by_hash(
    shared: &Arc<Mutex<SharedState>>,
    sender_hash: &str,
    sender_channel: u32,
    data: &[u8],
) -> bool {
    {
        let s = shared.lock().ok();
        let bridge_unavailable = s
            .as_ref()
            .and_then(|s| s.pchat.as_ref())
            .is_some_and(|p| p.signal_bridge_load_failed);
        if bridge_unavailable {
            return false;
        }
    }

    let Ok(mut state) = shared.lock() else {
        return false;
    };

    {
        let Some(ref mut pchat) = state.pchat else {
            return false;
        };
        if !pchat.ensure_signal_bridge() {
            warn!("signal bridge not loaded, cannot process sender key");
            return false;
        }
        let Some(ref bridge) = pchat.signal_bridge else {
            warn!("signal bridge not loaded, cannot process sender key");
            return false;
        };

        match bridge.process_distribution(sender_hash, sender_channel, data) {
            Ok(()) => {
                debug!(
                    sender = %sender_hash,
                    channel_id = sender_channel,
                    "processed signal sender key distribution"
                );
            }
            Err(e) => {
                warn!(
                    sender = %sender_hash,
                    channel_id = sender_channel,
                    "failed to process signal distribution: {e}"
                );
                return false;
            }
        }
    }

    retry_stashed_signal_envelopes(&mut state, sender_hash, sender_channel) > 0
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
    use super::*;

    #[test]
    fn ensure_signal_bridge_caches_failure() {
        let mut pchat =
            PchatState::new([0u8; 32], "test_cert_hash".to_string(), None).unwrap();

        assert!(!pchat.signal_bridge_load_failed);
        assert!(pchat.signal_bridge.is_none());

        // First call: DLL won't be found, should fail and set the flag.
        let result = pchat.ensure_signal_bridge();
        assert!(!result);
        assert!(pchat.signal_bridge_load_failed);

        // Second call: should short-circuit via the cached flag.
        let result = pchat.ensure_signal_bridge();
        assert!(!result);
        assert!(pchat.signal_bridge_load_failed);
    }
}
