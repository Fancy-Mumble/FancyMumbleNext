//! Encrypted local message cache for Signal Protocol channels.
//!
//! Decrypted message plaintext is cached in memory and persisted to an
//! AES-256-GCM encrypted file on disk.  The encryption key is derived
//! from the 32-byte identity seed via HKDF so the file is only
//! readable with the correct seed.  This mirrors Signal's architecture
//! of "decrypt once, store securely locally".

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN};
use ring::hkdf::{self, Salt, HKDF_SHA256};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use tracing::debug;

use super::types::ChatMessage;

/// File name for the encrypted local message cache.
const CACHE_FILE: &str = "signal_message_cache.enc";

/// HKDF info string for deriving the cache encryption key.
const HKDF_INFO: &[u8] = b"fancy-mumble-local-message-cache-v1";

/// Custom key type for HKDF output (32 bytes for AES-256).
struct CacheKeyLen;

impl hkdf::KeyType for CacheKeyLen {
    fn len(&self) -> usize {
        32
    }
}

/// A single cached message entry (serializable plaintext).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub(crate) struct CachedMessage {
    pub message_id: String,
    pub channel_id: u32,
    pub timestamp: u64,
    pub sender_hash: String,
    pub sender_name: String,
    pub body: String,
    pub is_own: bool,
}

/// AES-256-GCM encrypted local message cache.
///
/// Messages are stored in memory keyed by `channel_id` and persisted to
/// an encrypted file on disk.  The encryption key is derived from the
/// identity seed via HKDF so the file is only readable with the
/// correct seed.
pub(crate) struct LocalMessageCache {
    messages: HashMap<u32, Vec<CachedMessage>>,
    cache_key: LessSafeKey,
    cache_path: PathBuf,
}

impl LocalMessageCache {
    /// Create a new empty cache backed by the given identity directory and seed.
    pub fn new(identity_dir: &Path, seed: &[u8; 32]) -> Result<Self, String> {
        let cache_key = Self::derive_key(seed)?;
        Ok(Self {
            messages: HashMap::new(),
            cache_key,
            cache_path: identity_dir.join(CACHE_FILE),
        })
    }

    /// Derive the AES-256-GCM key from the identity seed via HKDF-SHA256.
    fn derive_key(seed: &[u8; 32]) -> Result<LessSafeKey, String> {
        let salt = Salt::new(HKDF_SHA256, &[]);
        let prk = salt.extract(seed);
        let okm = prk
            .expand(&[HKDF_INFO], CacheKeyLen)
            .map_err(|_| "HKDF expand failed".to_string())?;
        let mut key_bytes = [0u8; 32];
        okm.fill(&mut key_bytes)
            .map_err(|_| "HKDF fill failed".to_string())?;
        let unbound = UnboundKey::new(&AES_256_GCM, &key_bytes)
            .map_err(|_| "AES-256-GCM key creation failed".to_string())?;
        Ok(LessSafeKey::new(unbound))
    }

    /// Insert a message into the cache, deduplicating by `message_id`.
    pub fn insert(&mut self, msg: CachedMessage) {
        let channel = self.messages.entry(msg.channel_id).or_default();
        if !channel.iter().any(|m| m.message_id == msg.message_id) {
            channel.push(msg);
            channel.sort_by_key(|m| m.timestamp);
        }
    }

    /// Convert all cached messages into `ChatMessage` format, grouped by channel.
    pub fn all_chat_messages(&self) -> HashMap<u32, Vec<ChatMessage>> {
        self.messages
            .iter()
            .map(|(&channel_id, msgs)| {
                let chat_msgs = msgs
                    .iter()
                    .map(|m| ChatMessage {
                        sender_session: None,
                        sender_name: m.sender_name.clone(),
                        body: m.body.clone(),
                        channel_id: m.channel_id,
                        is_own: m.is_own,
                        dm_session: None,
                        group_id: None,
                        message_id: Some(m.message_id.clone()),
                        timestamp: Some(m.timestamp),
                        is_legacy: false,
                    })
                    .collect();
                (channel_id, chat_msgs)
            })
            .collect()
    }

    /// Save the cache to an AES-256-GCM encrypted file on disk.
    pub fn save(&self) -> Result<(), String> {
        let json =
            serde_json::to_vec(&self.messages).map_err(|e| format!("serialize cache: {e}"))?;
        let encrypted = self.encrypt(&json)?;
        std::fs::write(&self.cache_path, &encrypted)
            .map_err(|e| format!("write cache: {e}"))?;
        debug!(
            path = ?self.cache_path,
            messages = self.total_count(),
            "saved local message cache"
        );
        Ok(())
    }

    /// Load the cache from the encrypted file on disk.
    pub fn load(&mut self) -> Result<(), String> {
        if !self.cache_path.exists() {
            debug!(path = ?self.cache_path, "no local message cache found");
            return Ok(());
        }
        let encrypted =
            std::fs::read(&self.cache_path).map_err(|e| format!("read cache: {e}"))?;
        let json = self.decrypt(&encrypted)?;
        self.messages =
            serde_json::from_slice(&json).map_err(|e| format!("deserialize cache: {e}"))?;
        debug!(
            path = ?self.cache_path,
            messages = self.total_count(),
            "loaded local message cache"
        );
        Ok(())
    }

    fn total_count(&self) -> usize {
        self.messages.values().map(Vec::len).sum()
    }

    /// Encrypt plaintext with AES-256-GCM.  Output format: `[12-byte nonce][ciphertext+tag]`.
    fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        let rng = SystemRandom::new();
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rng.fill(&mut nonce_bytes)
            .map_err(|_| "RNG failed".to_string())?;

        let mut in_out = plaintext.to_vec();
        let nonce = Nonce::assume_unique_for_key(nonce_bytes);
        self.cache_key
            .seal_in_place_append_tag(nonce, Aad::empty(), &mut in_out)
            .map_err(|_| "AES-GCM seal failed".to_string())?;

        let mut result = Vec::with_capacity(NONCE_LEN + in_out.len());
        result.extend_from_slice(&nonce_bytes);
        result.extend_from_slice(&in_out);
        Ok(result)
    }

    /// Decrypt data produced by `encrypt()`.  Expects `[12-byte nonce][ciphertext+tag]`.
    fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        if data.len() < NONCE_LEN {
            return Err("cache file too short".to_string());
        }
        let (nonce_bytes, ciphertext) = data.split_at(NONCE_LEN);
        let nonce_arr: [u8; NONCE_LEN] = nonce_bytes
            .try_into()
            .map_err(|_| "invalid nonce length".to_string())?;
        let nonce = Nonce::assume_unique_for_key(nonce_arr);

        let mut in_out = ciphertext.to_vec();
        let plaintext = self
            .cache_key
            .open_in_place(nonce, Aad::empty(), &mut in_out)
            .map_err(|_| "AES-GCM open failed (wrong key or corrupted)".to_string())?;
        Ok(plaintext.to_vec())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]

    use super::*;
    use tempfile::TempDir;

    fn test_seed() -> [u8; 32] {
        [42u8; 32]
    }

    #[test]
    fn round_trip_encrypt_decrypt() {
        let dir = TempDir::new().unwrap();
        let cache = LocalMessageCache::new(dir.path(), &test_seed()).unwrap();
        let plaintext = b"hello world";
        let encrypted = cache.encrypt(plaintext).unwrap();
        let decrypted = cache.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn wrong_seed_fails_decrypt() {
        let dir = TempDir::new().unwrap();
        let cache1 = LocalMessageCache::new(dir.path(), &test_seed()).unwrap();
        let encrypted = cache1.encrypt(b"secret").unwrap();

        let other_seed = [99u8; 32];
        let cache2 = LocalMessageCache::new(dir.path(), &other_seed).unwrap();
        assert!(cache2.decrypt(&encrypted).is_err());
    }

    #[test]
    fn save_load_round_trip() {
        let dir = TempDir::new().unwrap();
        let seed = test_seed();

        let mut cache = LocalMessageCache::new(dir.path(), &seed).unwrap();
        cache.insert(CachedMessage {
            message_id: "msg-1".to_string(),
            channel_id: 5,
            timestamp: 1000,
            sender_hash: "abc".to_string(),
            sender_name: "Alice".to_string(),
            body: "Hello!".to_string(),
            is_own: false,
        });
        cache.insert(CachedMessage {
            message_id: "msg-2".to_string(),
            channel_id: 5,
            timestamp: 2000,
            sender_hash: "def".to_string(),
            sender_name: "Bob".to_string(),
            body: "Hi!".to_string(),
            is_own: true,
        });
        cache.save().unwrap();

        let mut cache2 = LocalMessageCache::new(dir.path(), &seed).unwrap();
        cache2.load().unwrap();

        let msgs = cache2.all_chat_messages();
        assert_eq!(msgs.len(), 1); // 1 channel
        let ch5 = &msgs[&5];
        assert_eq!(ch5.len(), 2);
        assert_eq!(ch5[0].body, "Hello!");
        assert_eq!(ch5[1].body, "Hi!");
    }

    #[test]
    fn dedup_by_message_id() {
        let dir = TempDir::new().unwrap();
        let mut cache = LocalMessageCache::new(dir.path(), &test_seed()).unwrap();
        let msg = CachedMessage {
            message_id: "dup-1".to_string(),
            channel_id: 1,
            timestamp: 100,
            sender_hash: "x".to_string(),
            sender_name: "X".to_string(),
            body: "test".to_string(),
            is_own: false,
        };
        cache.insert(msg.clone());
        cache.insert(msg);
        assert_eq!(cache.total_count(), 1);
    }
}
