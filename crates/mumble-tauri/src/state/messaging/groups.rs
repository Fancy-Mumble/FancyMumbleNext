//! Group chat operations: create, list, select, and send messages.

use mumble_protocol::command;
use tauri::Emitter;

use super::own_session_hash;
use crate::state::types::{ChatMessage, GroupChat, GroupCreatedPayload};
use crate::state::AppState;

impl AppState {
    pub async fn create_group(
        &self,
        name: String,
        member_sessions: Vec<u32>,
    ) -> Result<GroupChat, String> {
        let (own_session, full_members) = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            let own = state.conn.own_session.ok_or("Not connected")?;
            let mut members = member_sessions;
            if !members.contains(&own) {
                members.insert(0, own);
            }
            (own, members)
        };

        let group = GroupChat {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            members: full_members.clone(),
            creator: own_session,
        };

        if let Ok(mut state) = self.inner.lock() {
            let _ = state.msgs.group_chats.insert(group.id.clone(), group.clone());
        }

        let other_members: Vec<u32> = full_members
            .iter()
            .copied()
            .filter(|&s| s != own_session)
            .collect();

        if !other_members.is_empty() {
            let payload = serde_json::json!({
                "action": "create",
                "group": group,
            });
            let data = payload.to_string().into_bytes();
            self.send_plugin_data(other_members, data, "fancy-group".into())
                .await?;
        }

        if let Some(app) = self.app_handle() {
            let _ = app.emit("group-created", GroupCreatedPayload { group: group.clone() });
        }

        Ok(group)
    }

    pub fn groups(&self) -> Vec<GroupChat> {
        self.inner
            .lock()
            .map(|s| s.msgs.group_chats.values().cloned().collect())
            .unwrap_or_default()
    }

    pub fn group_messages(&self, group_id: &str) -> Vec<ChatMessage> {
        self.inner
            .lock()
            .map(|s| s.msgs.by_group.get(group_id).cloned().unwrap_or_default())
            .unwrap_or_default()
    }

    pub fn select_group(&self, group_id: String) -> Result<(), String> {
        {
            let mut state = self.inner.lock().map_err(|e| e.to_string())?;
            state.msgs.selected_group = Some(group_id.clone());
            state.selected_channel = None;
            state.msgs.selected_dm_user = None;
            let _ = state.msgs.group_unread.remove(&group_id);
        }
        self.emit_group_unreads();
        Ok(())
    }

    pub async fn send_group_message(
        &self,
        group_id: String,
        body: String,
    ) -> Result<(), String> {
        let (handle, own_session, own_name, own_hash, is_fancy, targets) = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            let group = state
                .msgs.group_chats
                .get(&group_id)
                .ok_or("Group not found")?;
            let own = state.conn.own_session.ok_or("Not connected")?;
            let targets: Vec<u32> = group
                .members
                .iter()
                .copied()
                .filter(|&s| s != own)
                .collect();
            let hash = own_session_hash(&state);
            (
                state.conn.client_handle.clone(),
                Some(own),
                state.conn.own_name.clone(),
                hash,
                state.server.fancy_version.is_some(),
                targets,
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

        let wire_body = format!("<!-- FANCY_GROUP:{group_id} -->{body}");

        handle
            .send(command::SendTextMessage {
                channel_ids: vec![],
                user_sessions: targets,
                tree_ids: vec![],
                message: wire_body,
                message_id: message_id.clone(),
                timestamp,
                edit_id: None,
            })
            .await
            .map_err(|e| format!("Failed to send group message: {e}"))?;

        if let Ok(mut state) = self.inner.lock() {
            let mut msg = ChatMessage {
                sender_session: own_session,
                sender_name: own_name,
                sender_hash: own_hash,
                body,
                channel_id: 0,
                is_own: true,
                dm_session: None,
                group_id: Some(group_id),
                message_id,
                timestamp,
                is_legacy: false,
                edited_at: None,
                pinned: false,
                pinned_by: None,
                pinned_at: None,
            };
            msg.ensure_id();
            let bucket = state
                .msgs.by_group
                .entry(msg.group_id.clone().unwrap_or_default())
                .or_default();
            crate::state::push_capped(bucket, msg);
        }

        Ok(())
    }
}
