//! On-disk persistence for archive keys, signal bridge state, and the
//! local encrypted message cache.

use std::collections::HashMap;
use std::path::Path;

use tracing::{debug, warn};

use fancy_utils::hex::{bytes_to_hex, hex_decode};
use mumble_protocol::persistent::protocol::signal_v1::SignalBridge;

use super::settings::*;
use super::PchatState;

// -- Archive key persistence ------------------------------------------

/// On-disk representation of a single archive key.
#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedArchiveKey {
    /// 32-byte key encoded as 64-character hex string.
    key_hex: String,
    /// Cert hash of the key originator (who generated the key).
    originator: Option<String>,
}

/// Persist a single archive key to disk.
///
/// Reads the existing JSON file, upserts the entry for `channel_id`,
/// and writes back.
pub(crate) fn persist_archive_key(
    identity_dir: &Path,
    channel_id: u32,
    key: &[u8; 32],
    originator: Option<&str>,
) {
    let path = identity_dir.join(ARCHIVE_KEYS_FILE);

    let mut keys: HashMap<String, PersistedArchiveKey> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    let key_hex: String = bytes_to_hex(key);
    let _ = keys.insert(
        channel_id.to_string(),
        PersistedArchiveKey {
            key_hex,
            originator: originator.map(String::from),
        },
    );

    match serde_json::to_string_pretty(&keys) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                warn!("failed to persist archive key: {e}");
            } else {
                debug!(channel_id, "persisted archive key to disk");
            }
        }
        Err(e) => warn!("failed to serialize archive keys: {e}"),
    }
}

/// Delete the persisted archive key for a single channel.
pub(crate) fn delete_persisted_archive_key(identity_dir: &Path, channel_id: u32) {
    let path = identity_dir.join(ARCHIVE_KEYS_FILE);

    let mut keys: HashMap<String, PersistedArchiveKey> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    if keys.remove(&channel_id.to_string()).is_some() {
        match serde_json::to_string_pretty(&keys) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    warn!("failed to update archive keys file: {e}");
                } else {
                    debug!(channel_id, "removed persisted archive key from disk");
                }
            }
            Err(e) => warn!("failed to serialize archive keys: {e}"),
        }
    }
}

/// Load all persisted archive keys from disk.
///
/// Returns `(channel_id, key_bytes, originator)` tuples. Entries with
/// invalid hex or wrong key length are silently skipped.
pub(crate) fn load_persisted_archive_keys(
    identity_dir: &Path,
) -> Vec<(u32, [u8; 32], Option<String>)> {
    let path = identity_dir.join(ARCHIVE_KEYS_FILE);

    let keys: HashMap<String, PersistedArchiveKey> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    keys.into_iter()
        .filter_map(|(ch_str, entry)| {
            let ch: u32 = ch_str.parse().ok()?;
            let key_bytes = hex_decode(&entry.key_hex)?;
            if key_bytes.len() != 32 {
                return None;
            }
            let mut key = [0u8; 32];
            key.copy_from_slice(&key_bytes);
            Some((ch, key, entry.originator))
        })
        .collect()
}

// -- Signal bridge state persistence ----------------------------------

/// Save the signal bridge state to disk so it can be restored on reconnect.
impl PchatState {
    pub(crate) fn save_signal_state(&self) {
        let Some(ref bridge) = self.signal_bridge else {
            debug!("no signal bridge loaded; skipping signal state save");
            return;
        };
        let Some(ref dir) = self.identity_dir else {
            debug!("no identity_dir set; skipping signal state save");
            return;
        };
        match bridge.export_state() {
            Ok(data) => {
                let path = dir.join(SIGNAL_STATE_FILE);
                if let Err(e) = std::fs::write(&path, &data) {
                    warn!(?path, "failed to write signal state: {e}");
                } else {
                    debug!(?path, bytes = data.len(), "saved signal bridge state");
                }
            }
            Err(e) => {
                warn!("failed to export signal bridge state: {e}");
            }
        }
    }

    /// Save the local message cache to disk (AES-256-GCM encrypted).
    pub(crate) fn save_local_cache(&self) {
        if let Some(ref cache) = self.local_cache {
            if let Err(e) = cache.save() {
                warn!("failed to save local message cache: {e}");
            }
        }
    }
}

/// Load a previously saved signal state from disk into the bridge.
pub(super) fn load_signal_state(identity_dir: Option<&Path>, bridge: &SignalBridge) {
    let Some(dir) = identity_dir else {
        return;
    };
    let path = dir.join(SIGNAL_STATE_FILE);
    if !path.exists() {
        debug!(?path, "no saved signal state found");
        return;
    }
    match std::fs::read(&path) {
        Ok(data) => match bridge.import_state(&data) {
            Ok(()) => debug!(?path, "restored signal bridge state from disk"),
            Err(e) => warn!(?path, "failed to import signal state: {e}"),
        },
        Err(e) => warn!(?path, "failed to read signal state file: {e}"),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
    use super::*;

    #[test]
    fn persist_and_load_archive_key_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let key: [u8; 32] = [42u8; 32];
        let originator = "abc123";

        persist_archive_key(dir.path(), 5, &key, Some(originator));

        let loaded = load_persisted_archive_keys(dir.path());
        assert_eq!(loaded.len(), 1);
        let (ch, loaded_key, loaded_orig) = &loaded[0];
        assert_eq!(*ch, 5);
        assert_eq!(*loaded_key, key);
        assert_eq!(loaded_orig.as_deref(), Some(originator));
    }

    #[test]
    fn persist_multiple_channels() {
        let dir = tempfile::tempdir().unwrap();
        let key1 = [1u8; 32];
        let key2 = [2u8; 32];

        persist_archive_key(dir.path(), 1, &key1, Some("orig1"));
        persist_archive_key(dir.path(), 7, &key2, None);

        let mut loaded = load_persisted_archive_keys(dir.path());
        loaded.sort_by_key(|(ch, _, _)| *ch);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0], (1, key1, Some("orig1".to_string())));
        assert_eq!(loaded[1], (7, key2, None));
    }

    #[test]
    fn persist_overwrites_existing_key() {
        let dir = tempfile::tempdir().unwrap();
        let key_old = [10u8; 32];
        let key_new = [20u8; 32];

        persist_archive_key(dir.path(), 3, &key_old, Some("orig_old"));
        persist_archive_key(dir.path(), 3, &key_new, Some("orig_new"));

        let loaded = load_persisted_archive_keys(dir.path());
        assert_eq!(loaded.len(), 1);
        let (ch, key, orig) = &loaded[0];
        assert_eq!(*ch, 3);
        assert_eq!(*key, key_new);
        assert_eq!(orig.as_deref(), Some("orig_new"));
    }

    #[test]
    fn load_from_nonexistent_dir_returns_empty() {
        let dir = Path::new("/nonexistent/path/12345");
        let loaded = load_persisted_archive_keys(dir);
        assert!(loaded.is_empty());
    }

    #[test]
    fn load_ignores_corrupt_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(ARCHIVE_KEYS_FILE), "not valid json").unwrap();
        let loaded = load_persisted_archive_keys(dir.path());
        assert!(loaded.is_empty());
    }

    #[test]
    fn load_ignores_wrong_key_length() {
        let dir = tempfile::tempdir().unwrap();
        let json = r#"{"1": {"key_hex": "aabb", "originator": null}}"#;
        std::fs::write(dir.path().join(ARCHIVE_KEYS_FILE), json).unwrap();
        let loaded = load_persisted_archive_keys(dir.path());
        assert!(loaded.is_empty());
    }
}
