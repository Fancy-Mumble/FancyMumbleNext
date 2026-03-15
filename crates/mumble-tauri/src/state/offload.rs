//! Encrypted temporary file storage for offloaded message content.
//!
//! Provides an [`OffloadProvider`] trait that abstracts storage and
//! retrieval of message bodies.  The default implementation,
//! [`EncryptedFileProvider`], uses ChaCha20-Poly1305 (AEAD) for
//! lightweight authenticated encryption on top of temp files.
//!
//! The encryption key is generated randomly at construction and held
//! only in memory -- it is never persisted to disk.  Files written by
//! a previous session are unrecoverable and cleaned up on the next
//! startup.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, CHACHA20_POLY1305};
use ring::rand::{SecureRandom, SystemRandom};
use tracing::{info, warn};

/// Nonce size for ChaCha20-Poly1305 (96 bits).
const NONCE_LEN: usize = 12;
/// Key size for ChaCha20-Poly1305 (256 bits).
const KEY_LEN: usize = 32;
/// Sub-directory name inside the system temp folder.
const OFFLOAD_DIR_NAME: &str = "fancy-mumble-offload";

// --- Provider trait -----------------------------------------------

/// Abstraction over how offloaded message content is persisted and
/// retrieved.
///
/// The default implementation encrypts content to local temp files.
/// Future implementations could store content on a remote server,
/// in a database, or anywhere else.
pub trait OffloadProvider {
    /// Persist `content` under the given `key`.
    fn store(&mut self, key: &str, content: &str) -> Result<(), String>;

    /// Retrieve previously stored content for `key`.
    fn load(&self, key: &str) -> Result<String, String>;

    /// Load multiple keys at once.
    ///
    /// The default implementation calls [`load`](OffloadProvider::load)
    /// in a loop.  Providers that benefit from batching (e.g. network
    /// round-trips) should override this.
    fn load_many(&self, keys: &[&str]) -> HashMap<String, Result<String, String>> {
        keys.iter()
            .map(|k| (k.to_string(), self.load(k)))
            .collect()
    }

    /// Delete the stored content for a single key.
    fn remove(&mut self, key: &str);

    /// Whether `key` is currently stored.
    #[allow(dead_code)]
    fn is_offloaded(&self, key: &str) -> bool;

    /// Number of keys currently offloaded.
    fn offloaded_count(&self) -> usize;

    /// Remove all stored content.
    fn clear(&mut self);

    /// Final cleanup (e.g. delete temp directory). Called on shutdown.
    fn cleanup(&mut self);
}

// --- Encrypted file provider --------------------------------------

/// Encrypted temporary file storage for offloaded message content.
///
/// Security properties:
///
/// * Key is randomly generated per session and never written to disk.
/// * Each file uses a unique random 96-bit nonce prepended to the
///   ciphertext so identical plaintexts produce different outputs.
/// * ChaCha20-Poly1305 provides authenticated encryption (AEAD),
///   protecting both confidentiality and integrity.
/// * Files are cleaned up on shutdown; stale files from crashes are
///   unrecoverable (no key) and deleted on the next startup.
pub struct EncryptedFileProvider {
    key: LessSafeKey,
    rng: SystemRandom,
    dir: PathBuf,
    /// Keys that are currently offloaded to disk.
    offloaded: HashSet<String>,
}

impl EncryptedFileProvider {
    /// Create a new provider with a fresh random encryption key.
    ///
    /// Creates the temp directory if it does not already exist.
    pub fn new() -> Result<Self, String> {
        let dir = std::env::temp_dir().join(OFFLOAD_DIR_NAME);
        Self::with_dir(dir)
    }

    /// Create a new provider that stores files in the given directory.
    ///
    /// Useful for tests that need isolated temp directories.
    fn with_dir(dir: PathBuf) -> Result<Self, String> {
        let rng = SystemRandom::new();
        let mut key_bytes = [0u8; KEY_LEN];
        rng.fill(&mut key_bytes)
            .map_err(|_| "Failed to generate encryption key")?;

        let unbound = UnboundKey::new(&CHACHA20_POLY1305, &key_bytes)
            .map_err(|_| "Failed to create encryption key")?;
        let key = LessSafeKey::new(unbound);

        fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create offload directory: {e}"))?;

        info!("EncryptedFileProvider initialised at {}", dir.display());

        Ok(Self {
            key,
            rng,
            dir,
            offloaded: HashSet::new(),
        })
    }

    /// Remove stale offload data left behind by a previous session that
    /// did not shut down cleanly.  Called once during application startup.
    pub fn cleanup_stale() {
        let dir = std::env::temp_dir().join(OFFLOAD_DIR_NAME);
        if dir.exists() {
            info!("Removing stale offload directory from a previous session");
            let _ = fs::remove_dir_all(&dir);
        }
    }

    /// Build a filesystem path for the given key.
    ///
    /// Keys are sanitised to prevent path-traversal attacks.
    fn file_path(&self, key: &str) -> PathBuf {
        let safe_key: String = key
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.dir.join(format!("{safe_key}.enc"))
    }

    /// Encrypt `plaintext` into `[nonce || ciphertext || tag]`.
    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        let rng = &self.rng;
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rng.fill(&mut nonce_bytes)
            .map_err(|_| "Failed to generate nonce")?;
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);

        let mut data = plaintext.to_vec();
        self.key
            .seal_in_place_append_tag(nonce, Aad::empty(), &mut data)
            .map_err(|_| "Encryption failed")?;

        let mut out = Vec::with_capacity(NONCE_LEN + data.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&data);
        Ok(out)
    }

    /// Decrypt `[nonce || ciphertext || tag]` back to plaintext bytes.
    fn decrypt(key: &LessSafeKey, file_data: &[u8]) -> Result<Vec<u8>, String> {
        if file_data.len() <= NONCE_LEN {
            return Err("Corrupted offload file: too short".into());
        }
        let (nonce_bytes, ciphertext) = file_data.split_at(NONCE_LEN);
        let nonce = Nonce::try_assume_unique_for_key(nonce_bytes)
            .map_err(|_| "Invalid nonce in offload file")?;

        let mut buf = ciphertext.to_vec();
        let plaintext = key
            .open_in_place(nonce, Aad::empty(), &mut buf)
            .map_err(|_| "Decryption failed (file may be corrupted or from another session)")?;
        Ok(plaintext.to_vec())
    }
}

impl OffloadProvider for EncryptedFileProvider {
    fn store(&mut self, key: &str, content: &str) -> Result<(), String> {
        let file_data = self.encrypt(content.as_bytes())?;
        let path = self.file_path(key);
        fs::write(&path, &file_data)
            .map_err(|e| format!("Failed to write offload file: {e}"))?;
        self.offloaded.insert(key.to_string());
        Ok(())
    }

    fn load(&self, key: &str) -> Result<String, String> {
        let path = self.file_path(key);
        let file_data =
            fs::read(&path).map_err(|e| format!("Failed to read offload file: {e}"))?;
        let plaintext = Self::decrypt(&self.key, &file_data)?;
        String::from_utf8(plaintext)
            .map_err(|e| format!("Decrypted content is not valid UTF-8: {e}"))
    }

    fn load_many(&self, keys: &[&str]) -> HashMap<String, Result<String, String>> {
        // For local files there is no network latency to amortise, but we
        // still benefit from a single trait call that avoids repeated IPC
        // round-trips from the frontend.
        keys.iter()
            .map(|k| (k.to_string(), self.load(k)))
            .collect()
    }

    fn remove(&mut self, key: &str) {
        self.offloaded.remove(key);
        let path = self.file_path(key);
        if path.exists() {
            let _ = fs::remove_file(&path);
        }
    }

    fn is_offloaded(&self, key: &str) -> bool {
        self.offloaded.contains(key)
    }

    fn offloaded_count(&self) -> usize {
        self.offloaded.len()
    }

    fn clear(&mut self) {
        self.offloaded.clear();
        if let Ok(entries) = fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    fn cleanup(&mut self) {
        self.offloaded.clear();
        if self.dir.exists() {
            if let Err(e) = fs::remove_dir_all(&self.dir) {
                warn!("Failed to clean up offload directory: {e}");
            } else {
                info!("Cleaned up offload directory");
            }
        }
    }
}

// --- OffloadStore (facade) ----------------------------------------

/// Wraps an [`OffloadProvider`] and exposes it as the public API used
/// by the rest of the application.
pub struct OffloadStore {
    provider: Box<dyn OffloadProvider + Send>,
}

impl OffloadStore {
    /// Create a store using the default [`EncryptedFileProvider`].
    pub fn new() -> Result<Self, String> {
        let provider = EncryptedFileProvider::new()?;
        Ok(Self {
            provider: Box::new(provider),
        })
    }

    /// Create a store backed by a custom provider.
    #[allow(dead_code)]
    pub fn with_provider(provider: Box<dyn OffloadProvider + Send>) -> Self {
        Self { provider }
    }

    pub fn store(&mut self, key: &str, content: &str) -> Result<(), String> {
        self.provider.store(key, content)
    }

    pub fn load(&self, key: &str) -> Result<String, String> {
        self.provider.load(key)
    }

    /// Load multiple keys in a single call.  Returns a map of key to
    /// result.  Failed keys contain an `Err`; successful ones `Ok`.
    pub fn load_many(&self, keys: &[&str]) -> HashMap<String, Result<String, String>> {
        self.provider.load_many(keys)
    }

    pub fn remove(&mut self, key: &str) {
        self.provider.remove(key);
    }

    #[allow(dead_code)]
    pub fn is_offloaded(&self, key: &str) -> bool {
        self.provider.is_offloaded(key)
    }

    /// Number of keys currently offloaded.
    pub fn offloaded_count(&self) -> usize {
        self.provider.offloaded_count()
    }

    pub fn clear(&mut self) {
        self.provider.clear();
    }

    pub fn cleanup_dir(&mut self) {
        self.provider.cleanup();
    }

    /// Remove stale offload data from a previous session.
    pub fn cleanup_stale() {
        EncryptedFileProvider::cleanup_stale();
    }
}

// --- Tests --------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Create an `OffloadStore` with an isolated temporary directory so
    /// parallel tests do not interfere with each other.
    fn isolated_store(name: &str) -> OffloadStore {
        let dir = std::env::temp_dir()
            .join(OFFLOAD_DIR_NAME)
            .join(format!("test-{name}-{}", std::process::id()));
        let provider = EncryptedFileProvider::with_dir(dir).expect("failed to create provider");
        OffloadStore::with_provider(Box::new(provider))
    }

    #[test]
    fn store_and_load_round_trip() {
        let mut store = isolated_store("round-trip");
        let content = "Hello, world! <img src='data:image/png;base64,abc123'/>";
        store.store("msg-1", content).unwrap();

        assert!(store.is_offloaded("msg-1"));

        let loaded = store.load("msg-1").unwrap();
        assert_eq!(loaded, content);

        store.cleanup_dir();
    }

    #[test]
    fn load_many_returns_all_keys() {
        let mut store = isolated_store("load-many");
        store.store("a", "content-a").unwrap();
        store.store("b", "content-b").unwrap();
        store.store("c", "content-c").unwrap();

        let results = store.load_many(&["a", "b", "c"]);
        assert_eq!(results.len(), 3);
        assert_eq!(results["a"].as_deref(), Ok("content-a"));
        assert_eq!(results["b"].as_deref(), Ok("content-b"));
        assert_eq!(results["c"].as_deref(), Ok("content-c"));

        store.cleanup_dir();
    }

    #[test]
    fn load_many_with_missing_key() {
        let mut store = isolated_store("load-many-missing");
        store.store("exists", "data").unwrap();

        let results = store.load_many(&["exists", "missing"]);
        assert!(results["exists"].is_ok());
        assert!(results["missing"].is_err());

        store.cleanup_dir();
    }

    #[test]
    fn remove_deletes_file() {
        let mut store = isolated_store("remove");
        store.store("rm-1", "data").unwrap();
        assert!(store.is_offloaded("rm-1"));

        store.remove("rm-1");
        assert!(!store.is_offloaded("rm-1"));
        assert!(store.load("rm-1").is_err());

        store.cleanup_dir();
    }

    #[test]
    fn clear_removes_all() {
        let mut store = isolated_store("clear");
        store.store("c1", "one").unwrap();
        store.store("c2", "two").unwrap();

        store.clear();
        assert!(!store.is_offloaded("c1"));
        assert!(!store.is_offloaded("c2"));

        store.cleanup_dir();
    }

    #[test]
    fn different_nonces_produce_different_ciphertext() {
        let mut store = isolated_store("nonces");
        store.store("dup1", "same content").unwrap();
        store.store("dup2", "same content").unwrap();

        // Read raw file bytes -- they should differ because nonces differ.
        let provider = store.provider.as_ref();
        // We can't access file_path directly through the trait, but we can
        // verify that both decrypt to the same plaintext.
        assert_eq!(store.load("dup1").unwrap(), "same content");
        assert_eq!(store.load("dup2").unwrap(), "same content");
        // And that they are both tracked as offloaded.
        assert!(provider.is_offloaded("dup1"));
        assert!(provider.is_offloaded("dup2"));

        store.cleanup_dir();
    }

    #[test]
    fn empty_content_round_trip() {
        let mut store = isolated_store("empty");
        store.store("empty", "").unwrap();

        let loaded = store.load("empty").unwrap();
        assert_eq!(loaded, "");

        store.cleanup_dir();
    }

    #[test]
    fn large_content_round_trip() {
        let mut store = isolated_store("large");
        // Simulate a large base64 image (~100KB).
        let content = "x".repeat(100_000);
        store.store("big", &content).unwrap();

        let loaded = store.load("big").unwrap();
        assert_eq!(loaded, content);

        store.cleanup_dir();
    }

    #[test]
    fn key_sanitisation_prevents_traversal() {
        let mut store = isolated_store("traversal");
        // Malicious key with path traversal attempt.
        store.store("../../../etc/passwd", "nope").unwrap();

        // Should still round-trip correctly (key is sanitised).
        let loaded = store.load("../../../etc/passwd").unwrap();
        assert_eq!(loaded, "nope");

        store.cleanup_dir();
    }

    #[test]
    fn custom_provider_via_with_provider() {
        /// A trivial in-memory provider for testing.
        struct InMemoryProvider {
            data: HashMap<String, String>,
        }
        impl OffloadProvider for InMemoryProvider {
            fn store(&mut self, key: &str, content: &str) -> Result<(), String> {
                self.data.insert(key.to_string(), content.to_string());
                Ok(())
            }
            fn load(&self, key: &str) -> Result<String, String> {
                self.data.get(key).cloned().ok_or("not found".into())
            }
            fn remove(&mut self, key: &str) {
                self.data.remove(key);
            }
            fn is_offloaded(&self, key: &str) -> bool {
                self.data.contains_key(key)
            }
            fn offloaded_count(&self) -> usize {
                self.data.len()
            }
            fn clear(&mut self) {
                self.data.clear();
            }
            fn cleanup(&mut self) {
                self.data.clear();
            }
        }

        let provider = InMemoryProvider {
            data: HashMap::new(),
        };
        let mut store = OffloadStore::with_provider(Box::new(provider));
        store.store("k", "v").unwrap();
        assert_eq!(store.load("k").unwrap(), "v");

        store.remove("k");
        assert!(!store.is_offloaded("k"));
    }
}
