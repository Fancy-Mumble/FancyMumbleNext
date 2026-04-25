//! Direct message (DM) operations.

use mumble_protocol::command;

use super::own_session_hash;
use crate::state::types::ChatMessage;
use crate::state::AppState;

impl AppState {
    pub async fn send_dm(&self, target_session: u32, body: String) -> Result<(), String> {
        let (handle, own_session, own_name, own_hash, is_fancy) = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            let hash = own_session_hash(&state);
            (
                state.conn.client_handle.clone(),
                state.conn.own_session,
                state.conn.own_name.clone(),
                hash,
                state.server.fancy_version.is_some(),
            )
        };

        let handle = handle.ok_or("Not connected")?;

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let message_id = if is_fancy {
            Some(uuid::Uuid::new_v4().to_string())
        } else {
            None
        };
        let timestamp = if is_fancy { Some(now_ms) } else { None };

        handle
            .send(command::SendTextMessage {
                channel_ids: vec![],
                user_sessions: vec![target_session],
                tree_ids: vec![],
                message: body.clone(),
                message_id: message_id.clone(),
                timestamp,
                edit_id: None,
            })
            .await
            .map_err(|e| format!("Failed to send DM: {e}"))?;

        if let Ok(mut state) = self.inner.lock() {
            let mut msg = ChatMessage {
                sender_session: own_session,
                sender_name: own_name,
                sender_hash: own_hash,
                body,
                channel_id: 0,
                is_own: true,
                dm_session: Some(target_session),
                message_id,
                timestamp,
                is_legacy: false,
                edited_at: None,
                pinned: false,
                pinned_by: None,
                pinned_at: None,
            };
            msg.ensure_id();
            let bucket = state.msgs.by_dm.entry(target_session).or_default();
            crate::state::push_capped(bucket, msg);
        }

        Ok(())
    }

    pub fn select_dm_user(&self, session: u32) -> Result<(), String> {
        {
            let mut state = self.inner.lock().map_err(|e| e.to_string())?;
            state.msgs.selected_dm_user = Some(session);
            state.selected_channel = None;
            let _ = state.msgs.dm_unread.remove(&session);
        }
        self.emit_dm_unreads();
        Ok(())
    }
}
