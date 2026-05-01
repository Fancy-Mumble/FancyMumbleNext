//! Content offloading: encrypt message bodies to temp files and restore
//! them on demand, keeping memory usage bounded for large chat histories.

use std::collections::HashMap;

use tauri::Manager;

use super::offload::OffloadStore;
use super::{AppState, SharedState};

impl AppState {
    pub fn init_offload_store(&self) -> Result<(), String> {
        let base_dir = self.offload_base_dir()?;
        OffloadStore::cleanup_stale(&base_dir);
        let store = OffloadStore::new(base_dir)?;
        if let Ok(mut state) = self.inner.snapshot().lock() {
            state.offload_store = Some(store);
        }
        Ok(())
    }

    pub(super) fn offload_base_dir(&self) -> Result<std::path::PathBuf, String> {
        if let Some(app) = self.app_handle() {
            if let Ok(cache) = app.path().cache_dir() {
                return Ok(cache);
            }
        }
        Ok(std::env::temp_dir())
    }

    pub fn offload_message(
        &self,
        message_id: String,
        scope: String,
        scope_id: String,
    ) -> Result<(), String> {
        let __session = self.inner.snapshot();
        let mut state = __session.lock().map_err(|e| e.to_string())?;

        let body = find_message_body(&state, &scope, &scope_id, &message_id)?;

        if body.starts_with("<!-- OFFLOADED:") {
            return Ok(());
        }

        let content_len = body.len();

        let store = state
            .offload_store
            .as_mut()
            .ok_or("Offload store not initialised")?;
        store.store(&message_id, &body)?;

        set_message_body(
            &mut state,
            &scope,
            &scope_id,
            &message_id,
            format!("<!-- OFFLOADED:{message_id}:{content_len} -->"),
        );

        Ok(())
    }

    pub fn load_offloaded_message(
        &self,
        message_id: String,
        scope: String,
        scope_id: String,
    ) -> Result<String, String> {
        let __session = self.inner.snapshot();
        let mut state = __session.lock().map_err(|e| e.to_string())?;

        let store = state
            .offload_store
            .as_mut()
            .ok_or("Offload store not initialised")?;
        let body = store.load(&message_id)?;
        store.remove(&message_id);

        set_message_body(&mut state, &scope, &scope_id, &message_id, body.clone());

        Ok(body)
    }

    pub fn load_offloaded_messages_batch(
        &self,
        message_ids: Vec<String>,
        scope: String,
        scope_id: String,
    ) -> Result<HashMap<String, String>, String> {
        let __session = self.inner.snapshot();
        let mut state = __session.lock().map_err(|e| e.to_string())?;

        let store = state
            .offload_store
            .as_mut()
            .ok_or("Offload store not initialised")?;

        let key_refs: Vec<&str> = message_ids.iter().map(String::as_str).collect();
        let results = store.load_many(&key_refs);

        let mut restored = HashMap::new();
        for (key, result) in &results {
            if let Ok(body) = result {
                store.remove(key);
                let _ = restored.insert(key.clone(), body.clone());
            }
        }

        for (key, body) in &restored {
            set_message_body(&mut state, &scope, &scope_id, key, body.clone());
        }

        Ok(restored)
    }

    pub fn clear_offloaded(&self) {
        if let Ok(mut state) = self.inner.snapshot().lock() {
            if let Some(store) = state.offload_store.as_mut() {
                store.clear();
            }
        }
    }

    pub fn shutdown_offload_store(&self) {
        if let Ok(mut state) = self.inner.snapshot().lock() {
            if let Some(store) = state.offload_store.as_mut() {
                store.cleanup_dir();
            }
        }
    }
}

// -- Helpers ----------------------------------------------------------

pub(super) fn find_message_body(
    state: &SharedState,
    scope: &str,
    scope_id: &str,
    message_id: &str,
) -> Result<String, String> {
    let messages = match scope {
        "channel" => {
            let ch_id: u32 = scope_id.parse().map_err(|_| "Invalid channel ID")?;
            state.msgs.by_channel.get(&ch_id)
        }
        "dm" => {
            let session: u32 = scope_id.parse().map_err(|_| "Invalid DM session")?;
            state.msgs.by_dm.get(&session)
        }
        _ => return Err(format!("Unknown scope: {scope}")),
    };
    let messages = messages.ok_or("No messages found for scope")?;
    let msg = messages
        .iter()
        .find(|m| m.message_id.as_deref() == Some(message_id))
        .ok_or("Message not found")?;
    Ok(msg.body.clone())
}

pub(super) fn set_message_body(
    state: &mut SharedState,
    scope: &str,
    scope_id: &str,
    message_id: &str,
    body: String,
) {
    let messages = match scope {
        "channel" => {
            let ch_id: u32 = scope_id.parse().unwrap_or(0);
            state.msgs.by_channel.get_mut(&ch_id)
        }
        "dm" => {
            let session: u32 = scope_id.parse().unwrap_or(0);
            state.msgs.by_dm.get_mut(&session)
        }
        _ => None,
    };
    if let Some(messages) = messages {
        if let Some(msg) = messages
            .iter_mut()
            .find(|m| m.message_id.as_deref() == Some(message_id))
        {
            msg.body = body;
        }
    }
}
