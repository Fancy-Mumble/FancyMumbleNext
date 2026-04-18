//! Channel operations: select, join, listen, create, update, delete.

use mumble_protocol::command;
use mumble_protocol::persistent::PchatProtocol;
use tauri::Emitter;
use tracing::debug;

use super::parse_pchat_protocol_str;
use super::types::CurrentChannelPayload;
use super::AppState;

impl AppState {
    pub async fn select_channel(&self, channel_id: u32) -> Result<(), String> {
        let handle = {
            let mut state = self.inner.lock().map_err(|e| e.to_string())?;
            state.selected_channel = Some(channel_id);
            state.msgs.selected_dm_user = None;
            state.msgs.selected_group = None;
            let _ = state.msgs.channel_unread.remove(&channel_id);
            state.conn.client_handle.clone()
        };
        self.emit_unreads();

        if let Some(handle) = handle {
            let _ = handle
                .send(command::PermissionQuery { channel_id })
                .await;
        }

        Ok(())
    }

    pub async fn join_channel(&self, channel_id: u32) -> Result<(), String> {
        let handle = {
            let mut state = self.inner.lock().map_err(|e| e.to_string())?;
            state.current_channel = Some(channel_id);
            state.conn.client_handle.clone()
        };

        if let Some(handle) = handle {
            let _ = handle
                .send(command::JoinChannel { channel_id })
                .await;
            let _ = handle
                .send(command::PermissionQuery { channel_id })
                .await;
        }

        if let Some(app) = self.app_handle() {
            let _ = app.emit("current-channel-changed", CurrentChannelPayload { channel_id });
        }

        Ok(())
    }

    pub fn current_channel(&self) -> Option<u32> {
        self.inner
            .lock()
            .ok()
            .and_then(|s| s.current_channel)
    }

    pub async fn toggle_listen(&self, channel_id: u32) -> Result<bool, String> {
        debug!(channel_id, "toggle_listen called");
        let (handle, is_now_listened, add, remove) = {
            let mut state = self.inner.lock().map_err(|e| e.to_string())?;
            let handle = state.conn.client_handle.clone();

            if !state.permanently_listened.contains(&channel_id) {
                let no_listen_perm = state
                    .channels
                    .get(&channel_id)
                    .and_then(|ch| ch.permissions)
                    .is_some_and(|p| p & 0x800 == 0);
                if no_listen_perm {
                    return Err(
                        "You do not have permission to listen to this channel".into(),
                    );
                }
            }

            if state.permanently_listened.contains(&channel_id) {
                let _ = state.permanently_listened.remove(&channel_id);
                let is_selected = state.selected_channel == Some(channel_id);
                if is_selected {
                    (handle, false, vec![], vec![])
                } else {
                    (handle, false, vec![], vec![channel_id])
                }
            } else {
                let _ = state.permanently_listened.insert(channel_id);
                let is_selected = state.selected_channel == Some(channel_id);
                if is_selected {
                    (handle, true, vec![], vec![])
                } else {
                    (handle, true, vec![channel_id], vec![])
                }
            }
        };

        if let Some(handle) = handle {
            if !add.is_empty() || !remove.is_empty() {
                debug!(?add, ?remove, is_now_listened, "sending ChannelListen");
                if let Err(e) = handle
                    .send(command::ChannelListen {
                        add: add.clone(),
                        remove: remove.clone(),
                    })
                    .await
                {
                    tracing::error!("failed to send ChannelListen: {e}");
                }
            } else {
                debug!(
                    is_now_listened,
                    "toggle_listen: no protocol message needed (channel already listened via selection)"
                );
            }
        } else {
            tracing::warn!("toggle_listen: no client handle - not connected?");
        }

        Ok(is_now_listened)
    }

    pub fn listened_channels(&self) -> Vec<u32> {
        self.inner
            .lock()
            .map(|s| s.permanently_listened.iter().copied().collect())
            .unwrap_or_default()
    }

    #[allow(clippy::too_many_arguments, reason = "channel update mirrors the full server-side parameter surface as optional fields")]
    pub async fn update_channel(
        &self,
        channel_id: u32,
        name: Option<String>,
        description: Option<String>,
        position: Option<i32>,
        temporary: Option<bool>,
        max_users: Option<u32>,
        pchat_protocol: Option<String>,
        pchat_max_history: Option<u32>,
        pchat_retention_days: Option<u32>,
    ) -> Result<(), String> {
        let handle = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };
        match handle {
            Some(h) => {
                let parsed_protocol = pchat_protocol.as_ref().map(|s| parse_pchat_protocol_str(s));
                tracing::debug!(
                    channel_id,
                    ?name,
                    ?pchat_protocol,
                    ?parsed_protocol,
                    proto_value = ?parsed_protocol.map(PchatProtocol::to_proto),
                    ?pchat_max_history,
                    ?pchat_retention_days,
                    "sending update_channel command"
                );
                h.send(command::SetChannelState {
                    channel_id: Some(channel_id),
                    parent: None,
                    name,
                    description,
                    position,
                    temporary,
                    max_users,
                    pchat_protocol: parsed_protocol,
                    pchat_max_history,
                    pchat_retention_days,
                })
                .await
                .map_err(|e| e.to_string())
            }
            None => Err("Not connected".into()),
        }
    }

    pub async fn delete_channel(&self, channel_id: u32) -> Result<(), String> {
        let handle = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::DeleteChannel { channel_id })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    #[allow(clippy::too_many_arguments, reason = "channel creation mirrors the full server-side parameter surface as optional fields")]
    pub async fn create_channel(
        &self,
        parent_id: u32,
        name: String,
        description: Option<String>,
        position: Option<i32>,
        temporary: Option<bool>,
        max_users: Option<u32>,
        pchat_protocol: Option<String>,
        pchat_max_history: Option<u32>,
        pchat_retention_days: Option<u32>,
    ) -> Result<(), String> {
        let handle = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };
        match handle {
            Some(h) => {
                h.send(command::SetChannelState {
                    channel_id: None,
                    parent: Some(parent_id),
                    name: Some(name),
                    description,
                    position,
                    temporary,
                    max_users,
                    pchat_protocol: pchat_protocol.map(|s| parse_pchat_protocol_str(&s)),
                    pchat_max_history,
                    pchat_retention_days,
                })
                .await
                .map_err(|e| e.to_string())
            }
            None => Err("Not connected".into()),
        }
    }
}
