//! Messaging: channel messages, encryption, and message storage.

mod dm;
mod unreads;

use mumble_protocol::client::ClientHandle;
use mumble_protocol::command;
use mumble_protocol::persistent::PchatProtocol;

use super::types::ChatMessage;
use super::{pchat, AppState, SharedState};

struct OwnMessageData {
    channel_id: u32,
    own_session: Option<u32>,
    own_name: String,
    own_hash: Option<String>,
    body: String,
    message_id: Option<String>,
    timestamp: Option<u64>,
    pchat_protocol: Option<PchatProtocol>,
}

fn own_session_hash(state: &SharedState) -> Option<String> {
    state
        .conn.own_session
        .and_then(|sid| state.users.get(&sid))
        .and_then(|u| u.hash.clone())
}

fn cache_own_signal_message(state: &mut SharedState, msg: &ChatMessage, channel_id: u32) {
    let own_cert_hash = state
        .pchat_ctx.pchat
        .as_ref()
        .map(|ps| ps.own_cert_hash.clone())
        .unwrap_or_default();
    if let Some(cache) = state.pchat_ctx.pchat.as_mut().and_then(|ps| ps.local_cache.as_mut()) {
        cache.insert(super::local_cache::CachedMessage {
            message_id: msg.message_id.clone().unwrap_or_default(),
            channel_id,
            timestamp: msg.timestamp.unwrap_or(0),
            sender_hash: own_cert_hash,
            sender_name: msg.sender_name.clone(),
            body: msg.body.clone(),
            is_own: true,
        });
    }
}

impl AppState {
    pub async fn fetch_older_messages(
        &self,
        channel_id: u32,
        before_id: Option<String>,
        limit: u32,
    ) -> Result<(), String> {
        let handle = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };
        let handle = handle.ok_or("Not connected")?;
        pchat::send_fetch(&handle, channel_id, before_id, limit).await
    }

    pub async fn send_message(&self, channel_id: u32, body: String) -> Result<(), String> {
        let (handle, own_session, own_name, own_hash, is_fancy, pchat_protocol) = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            let pchat_proto = state
                .channels
                .get(&channel_id)
                .and_then(|ch| ch.pchat_protocol);
            let hash = own_session_hash(&state);
            (
                state.conn.client_handle.clone(),
                state.conn.own_session,
                state.conn.own_name.clone(),
                hash,
                state.server.fancy_version.is_some(),
                pchat_proto,
            )
        };

        let handle = handle.ok_or("Not connected")?;

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let message_id = is_fancy.then(|| uuid::Uuid::new_v4().to_string());
        let timestamp = is_fancy.then_some(now_ms);

        let disable_dual = pchat_protocol
            .is_some_and(|p| p.is_encrypted())
            && self
                .inner
                .lock()
                .map(|s| s.prefs.disable_dual_path)
                .unwrap_or(false);
        let text_body = if disable_dual {
            "[Encrypted message]".to_string()
        } else {
            body.clone()
        };

        let prebuilt_pchat = self.prebuilt_pchat_message(
            pchat_protocol, &message_id, channel_id, &body, &own_name, now_ms,
        )?;

        handle
            .send(command::SendTextMessage {
                channel_ids: vec![channel_id],
                user_sessions: vec![],
                tree_ids: vec![],
                message: text_body,
                message_id: message_id.clone(),
                timestamp,
                edit_id: None,
            })
            .await
            .map_err(|e| format!("Failed to send message: {e}"))?;

        if let Some((proto_msg, client)) = prebuilt_pchat {
            if let Err(e) = client
                .send(command::SendPchatMessage { message: proto_msg })
                .await
            {
                tracing::warn!("send pchat-msg failed: {e}");
            }
        }

        self.store_own_message(OwnMessageData {
            channel_id, own_session, own_name, own_hash,
            body, message_id, timestamp, pchat_protocol,
        });
        Ok(())
    }

    fn prebuilt_pchat_message(
        &self,
        pchat_protocol: Option<PchatProtocol>,
        message_id: &Option<String>,
        channel_id: u32,
        body: &str,
        own_name: &str,
        now_ms: u64,
    ) -> Result<Option<(mumble_protocol::proto::mumble_tcp::PchatMessage, ClientHandle)>, String> {
        let Some(protocol) = pchat_protocol.filter(PchatProtocol::is_encrypted) else {
            return Ok(None);
        };
        let Some(ref msg_id) = message_id else {
            return Ok(None);
        };
        let session = self.inner.lock().ok()
            .and_then(|s| s.conn.own_session)
            .unwrap_or(0);
        self.build_pchat_encrypted(&pchat::OutboundMessage {
            channel_id,
            protocol,
            message_id: msg_id,
            body,
            sender_name: own_name,
            sender_session: session,
            timestamp: now_ms,
        })
    }

    fn store_own_message(&self, msg_data: OwnMessageData) {
        let Ok(mut state) = self.inner.lock() else { return };
        let mut msg = ChatMessage {
            sender_session: msg_data.own_session,
            sender_name: msg_data.own_name,
            sender_hash: msg_data.own_hash,
            body: msg_data.body,
            channel_id: msg_data.channel_id,
            is_own: true,
            dm_session: None,
            message_id: msg_data.message_id,
            timestamp: msg_data.timestamp,
            is_legacy: false,
            edited_at: None,
            pinned: false,
            pinned_by: None,
            pinned_at: None,
        };
        msg.ensure_id();

        if msg_data.pchat_protocol.is_some_and(|p| p == PchatProtocol::SignalV1) {
            cache_own_signal_message(&mut state, &msg, msg_data.channel_id);
        }

        let bucket = state.msgs.by_channel.entry(msg_data.channel_id).or_default();
        super::push_capped(bucket, msg);
    }

    pub async fn edit_message(
        &self,
        channel_id: u32,
        message_id: String,
        new_body: String,
    ) -> Result<(), String> {
        let (handle, is_fancy) = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            (
                state.conn.client_handle.clone(),
                state.server.fancy_version.is_some(),
            )
        };

        let handle = handle.ok_or("Not connected")?;
        if !is_fancy {
            return Err("Message editing requires a Fancy Mumble server".into());
        }

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        handle
            .send(command::SendTextMessage {
                channel_ids: vec![channel_id],
                user_sessions: vec![],
                tree_ids: vec![],
                message: new_body.clone(),
                message_id: Some(uuid::Uuid::new_v4().to_string()),
                timestamp: Some(now_ms),
                edit_id: Some(message_id.clone()),
            })
            .await
            .map_err(|e| format!("Failed to send edit: {e}"))?;

        if let Ok(mut state) = self.inner.lock() {
            if let Some(msgs) = state.msgs.by_channel.get_mut(&channel_id) {
                if let Some(msg) = msgs.iter_mut().find(|m| m.message_id.as_deref() == Some(&message_id)) {
                    msg.body = new_body;
                    msg.edited_at = Some(now_ms);
                }
            }
        }

        Ok(())
    }

    /// Build an encrypted `PchatMessage` proto without sending it.
    pub(super) fn build_pchat_encrypted(
        &self,
        outbound: &pchat::OutboundMessage<'_>,
    ) -> Result<Option<(mumble_protocol::proto::mumble_tcp::PchatMessage, ClientHandle)>, String> {
        let mut state = self.inner.lock().map_err(|e| e.to_string())?;
        let client = state.conn.client_handle.clone();
        if let (Some(ref mut pchat_state), Some(client)) = (&mut state.pchat_ctx.pchat, client) {
            if outbound.protocol == PchatProtocol::SignalV1
                && pchat_state.signal_bridge.is_none()
                && !pchat_state.signal_bridge_load_failed
            {
                tracing::info!("send_message: lazy-loading signal bridge");
                let _ = pchat_state.ensure_signal_bridge();
            }
            match pchat_state.build_encrypted_message(outbound) {
                Ok(proto_msg) => Ok(Some((proto_msg, client))),
                Err(e) => {
                    tracing::warn!("pchat encrypt failed: {e}");
                    Err(format!("Encryption failed: {e}"))
                }
            }
        } else {
            Ok(None)
        }
    }
}
