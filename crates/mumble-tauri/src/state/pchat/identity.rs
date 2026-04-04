//! Identity storage: per-identity TLS certificates and pchat seeds.
//!
//! Each identity maps to a directory under `<app_data>/identities/<label>/`
//! containing TLS certificate PEM files and a 32-byte pchat seed.

use std::path::{Path, PathBuf};

use tracing::{info, warn};

use fancy_utils::hex::{bytes_to_hex, hex_decode};

use super::settings::*;

/// Encapsulates all filesystem operations for identity management.
///
/// Wraps the application data directory and provides methods for
/// creating, loading, listing, exporting, and importing identities.
pub(crate) struct IdentityStore {
    app_data_dir: PathBuf,
}

impl IdentityStore {
    pub fn new(app_data_dir: PathBuf) -> Self {
        Self { app_data_dir }
    }

    /// Return the directory for a given identity label:
    /// `<app_data>/identities/<label>/`
    pub fn identity_dir(&self, label: &str) -> PathBuf {
        self.app_data_dir.join(IDENTITIES_DIR).join(label)
    }

    /// Migrate legacy storage layout to per-identity folders.
    ///
    /// Old layout:
    /// ```text
    /// {app_data}/certs/{label}.cert.pem
    /// {app_data}/certs/{label}.key.pem
    /// {app_data}/pchat/identity_seed.bin     (single global seed)
    /// ```
    ///
    /// New layout:
    /// ```text
    /// {app_data}/identities/{label}/tls.cert.pem
    /// {app_data}/identities/{label}/tls.key.pem
    /// {app_data}/identities/{label}/pchat_seed.bin
    /// ```
    pub fn migrate_legacy_storage(&self) {
        let legacy_certs = self.app_data_dir.join(LEGACY_CERTS_DIR);
        if !legacy_certs.exists() {
            return;
        }

        let global_seed: Option<[u8; 32]> = std::fs::read(
            self.app_data_dir.join(LEGACY_PCHAT_DIR).join(LEGACY_SEED_FILE),
        )
        .ok()
        .and_then(|data| <[u8; 32]>::try_from(data.as_slice()).ok());

        let Ok(entries) = std::fs::read_dir(&legacy_certs) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Some(label) = name.strip_suffix(".cert.pem") else {
                continue;
            };
            let new_dir = self.identity_dir(label);
            if new_dir.exists() {
                continue;
            }
            if std::fs::create_dir_all(&new_dir).is_err() {
                continue;
            }

            let old_cert = legacy_certs.join(format!("{label}.cert.pem"));
            let old_key = legacy_certs.join(format!("{label}.key.pem"));
            let _ = std::fs::copy(&old_cert, new_dir.join(TLS_CERT_FILE));
            let _ = std::fs::copy(&old_key, new_dir.join(TLS_KEY_FILE));

            if let Some(seed) = global_seed {
                let _ = std::fs::write(new_dir.join(SEED_FILE), seed);
            }

            info!(label, "migrated legacy identity to per-identity storage");
        }

        let _ = std::fs::remove_dir_all(&legacy_certs);
        let pchat_dir = self.app_data_dir.join(LEGACY_PCHAT_DIR);
        if pchat_dir.exists() {
            let _ = std::fs::remove_dir_all(&pchat_dir);
        }
    }

    /// Load or generate the 32-byte identity seed for a specific identity.
    ///
    /// Stored in `<app_data>/identities/<label>/pchat_seed.bin`.
    /// If the file does not exist, a new seed is generated from the OS CSPRNG.
    pub fn load_or_generate_seed(&self, label: &str) -> Result<[u8; 32], String> {
        let dir = self.identity_dir(label);
        let seed_path = dir.join(SEED_FILE);

        if seed_path.exists() {
            let data =
                std::fs::read(&seed_path).map_err(|e| format!("Failed to read seed: {e}"))?;
            if data.len() == 32 {
                let mut seed = [0u8; 32];
                seed.copy_from_slice(&data);
                info!(label, "loaded existing pchat identity seed");
                return Ok(seed);
            }
            warn!(
                label,
                len = data.len(),
                "seed file has wrong length, regenerating"
            );
        }

        let seed: [u8; 32] = rand::random();
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create identity dir: {e}"))?;
        std::fs::write(&seed_path, seed).map_err(|e| format!("Failed to write seed: {e}"))?;
        info!(label, "generated new pchat identity seed");
        Ok(seed)
    }

    /// Generate a self-signed TLS client certificate for an identity label.
    /// Does nothing if the cert already exists.
    pub fn generate_cert(&self, label: &str) -> Result<(), String> {
        let dir = self.identity_dir(label);
        let cert_path = dir.join(TLS_CERT_FILE);
        if cert_path.exists() {
            return Ok(());
        }

        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create identity dir: {e}"))?;

        let certified = rcgen::generate_simple_self_signed(vec![label.to_string()])
            .map_err(|e| e.to_string())?;
        let cert_pem = certified.cert.pem();
        let key_pem = certified.signing_key.serialize_pem();

        std::fs::write(&cert_path, cert_pem).map_err(|e| e.to_string())?;
        std::fs::write(dir.join(TLS_KEY_FILE), key_pem).map_err(|e| e.to_string())?;

        info!(label, "generated new TLS client certificate");
        Ok(())
    }

    /// Load TLS client certificate PEM bytes for an identity label.
    /// Returns `(cert_pem, key_pem)` or `(None, None)` if not found.
    pub fn load_cert(&self, label: &str) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
        let dir = self.identity_dir(label);
        let cert = std::fs::read(dir.join(TLS_CERT_FILE)).ok();
        let key = std::fs::read(dir.join(TLS_KEY_FILE)).ok();
        (cert, key)
    }

    /// List all identity labels (subdirectories of `identities/`).
    pub fn list_labels(&self) -> Vec<String> {
        let dir = self.app_data_dir.join(IDENTITIES_DIR);
        if !dir.exists() {
            return vec![];
        }
        let mut labels = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false)
                    && entry.path().join(TLS_CERT_FILE).exists()
                {
                    labels.push(entry.file_name().to_string_lossy().to_string());
                }
            }
        }
        labels.sort();
        labels
    }

    /// Delete an identity (TLS cert + pchat seed).
    pub fn delete(&self, label: &str) -> Result<(), String> {
        let dir = self.identity_dir(label);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Export an identity to a JSON bundle at the given `dest` path.
    pub fn export(&self, label: &str, dest: &Path) -> Result<(), String> {
        use serde_json::{json, Map, Value};

        let dir = self.identity_dir(label);
        if !dir.exists() {
            return Err(format!("Identity '{label}' not found"));
        }

        let mut bundle = Map::new();
        let _ = bundle.insert("_label".to_string(), Value::String(label.to_string()));

        for name in [TLS_CERT_FILE, TLS_KEY_FILE] {
            let path = dir.join(name);
            if path.exists() {
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| format!("Failed to read {name}: {e}"))?;
                let _ = bundle.insert(name.to_string(), Value::String(text));
            }
        }

        let seed_path = dir.join(SEED_FILE);
        if seed_path.exists() {
            let data = std::fs::read(&seed_path)
                .map_err(|e| format!("Failed to read seed: {e}"))?;
            let hex: String = bytes_to_hex(&data);
            let _ = bundle.insert(SEED_FILE.to_string(), Value::String(hex));
        }

        let json = serde_json::to_string_pretty(&json!(bundle))
            .map_err(|e| format!("Serialisation error: {e}"))?;
        std::fs::write(dest, json).map_err(|e| format!("Failed to write export file: {e}"))?;
        info!(label, ?dest, "exported identity");
        Ok(())
    }

    /// Import an identity from a JSON bundle at `src`.
    /// Returns the label embedded in the bundle.
    pub fn import(&self, src: &Path) -> Result<String, String> {
        use serde_json::Value;

        let json = std::fs::read_to_string(src)
            .map_err(|e| format!("Failed to read import file: {e}"))?;
        let bundle: serde_json::Map<String, Value> =
            serde_json::from_str(&json).map_err(|e| format!("Invalid identity file: {e}"))?;

        let label = bundle
            .get("_label")
            .and_then(Value::as_str)
            .ok_or("Missing _label in identity file")?
            .to_string();

        let dir = self.identity_dir(&label);
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create identity dir: {e}"))?;

        for name in [TLS_CERT_FILE, TLS_KEY_FILE] {
            if let Some(text) = bundle.get(name).and_then(Value::as_str) {
                std::fs::write(dir.join(name), text)
                    .map_err(|e| format!("Failed to write {name}: {e}"))?;
            }
        }

        if let Some(hex_str) = bundle.get(SEED_FILE).and_then(Value::as_str) {
            let data = hex_decode(hex_str).ok_or("Invalid hex for seed")?;
            std::fs::write(dir.join(SEED_FILE), data)
                .map_err(|e| format!("Failed to write seed: {e}"))?;
        }

        info!(label, ?src, "imported identity");
        Ok(label)
    }
}
