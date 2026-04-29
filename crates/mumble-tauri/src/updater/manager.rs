//! Internal updater state: holds the most recent [`tauri_plugin_updater::Update`]
//! returned by a check, so the install command can reuse it without
//! issuing another network request.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use tauri_plugin_updater::Update;

/// Shared state managed by Tauri ([`tauri::Manager::manage`]).
#[derive(Default)]
pub(crate) struct UpdaterState {
    pub(crate) pending: Mutex<Option<Update>>,
    /// When true, the bootstrapper window opens with `?auto=1` and starts
    /// the install immediately without waiting for the user to click.
    pub(crate) auto_install: AtomicBool,
    /// Version string the user chose to skip. Updates matching this
    /// version are silently ignored on the next startup check.
    pub(crate) skipped_version: Mutex<Option<String>>,
}

impl UpdaterState {
    pub(crate) fn store(&self, update: Update) {
        if let Ok(mut guard) = self.pending.lock() {
            *guard = Some(update);
        }
    }

    pub(crate) fn take(&self) -> Option<Update> {
        self.pending.lock().ok().and_then(|mut g| g.take())
    }

    pub(crate) fn snapshot(&self) -> Option<UpdateInfo> {
        self.pending.lock().ok().and_then(|g| g.as_ref().map(UpdateInfo::from))
    }

    pub(crate) fn set_auto_install(&self, enabled: bool) {
        self.auto_install.store(enabled, Ordering::Relaxed);
    }

    pub(crate) fn auto_install(&self) -> bool {
        self.auto_install.load(Ordering::Relaxed)
    }

    pub(crate) fn set_skipped_version(&self, version: Option<String>) {
        if let Ok(mut guard) = self.skipped_version.lock() {
            *guard = version;
        }
    }

    pub(crate) fn skipped_version(&self) -> Option<String> {
        self.skipped_version.lock().ok().and_then(|g| g.clone())
    }
}

/// Lightweight, serialisable snapshot of an [`Update`] for the frontend.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    pub date: Option<String>,
    pub body: Option<String>,
}

impl From<&Update> for UpdateInfo {
    fn from(u: &Update) -> Self {
        Self {
            version: u.version.clone(),
            current_version: u.current_version.clone(),
            date: u.date.map(|d| d.to_string()),
            body: u.body.clone(),
        }
    }
}
