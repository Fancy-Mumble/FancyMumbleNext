//! Unread message tracking: per-channel, per-DM, and per-group counts.

use std::collections::HashMap;

use tauri::Emitter;

use crate::state::types::{DmUnreadPayload, GroupUnreadPayload, UnreadPayload};
use crate::state::AppState;

impl AppState {
    pub fn unread_counts(&self) -> HashMap<u32, u32> {
        self.inner
            .lock()
            .map(|s| s.msgs.channel_unread.clone())
            .unwrap_or_default()
    }

    pub fn mark_read(&self, channel_id: u32) {
        if let Ok(mut state) = self.inner.lock() {
            let _ = state.msgs.channel_unread.remove(&channel_id);
        }
        self.emit_unreads();
    }

    pub(in crate::state) fn emit_unreads(&self) {
        if let Some(handle) = self.app_handle() {
            let unreads = self.unread_counts();
            let _ = handle.emit("unread-changed", UnreadPayload { unreads });
        }
    }

    pub fn dm_unread_counts(&self) -> HashMap<u32, u32> {
        self.inner
            .lock()
            .map(|s| s.msgs.dm_unread.clone())
            .unwrap_or_default()
    }

    pub fn mark_dm_read(&self, session: u32) {
        if let Ok(mut state) = self.inner.lock() {
            let _ = state.msgs.dm_unread.remove(&session);
        }
        self.emit_dm_unreads();
    }

    pub(in crate::state) fn emit_dm_unreads(&self) {
        if let Some(handle) = self.app_handle() {
            let unreads = self.dm_unread_counts();
            let _ = handle.emit("dm-unread-changed", DmUnreadPayload { unreads });
        }
    }

    pub fn group_unread_counts(&self) -> HashMap<String, u32> {
        self.inner
            .lock()
            .map(|s| s.msgs.group_unread.clone())
            .unwrap_or_default()
    }

    pub fn mark_group_read(&self, group_id: &str) {
        if let Ok(mut state) = self.inner.lock() {
            let _ = state.msgs.group_unread.remove(group_id);
        }
        self.emit_group_unreads();
    }

    pub(in crate::state) fn emit_group_unreads(&self) {
        if let Some(handle) = self.app_handle() {
            let unreads = self.group_unread_counts();
            let _ = handle.emit("group-unread-changed", GroupUnreadPayload { unreads });
        }
    }
}
