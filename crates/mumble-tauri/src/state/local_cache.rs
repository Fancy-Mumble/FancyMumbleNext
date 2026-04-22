//! Encrypted local message cache for Signal Protocol channels.
//!
//! Decrypted message plaintext is cached in memory and persisted to an
//! AES-256-GCM encrypted file on disk.  The encryption key is derived
//! from the 32-byte identity seed via HKDF so the file is only
//! readable with the correct seed.  This mirrors Signal's architecture
//! of "decrypt once, store securely locally".

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN};
use ring::hkdf::{self, Salt, HKDF_SHA256};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use tracing::debug;

use super::types::ChatMessage;

/// File name for the encrypted local message cache.
const CACHE_FILE: &str = "signal_message_cache.enc";

/// File name for the encrypted local reaction cache.
const REACTION_CACHE_FILE: &str = "signal_reaction_cache.enc";

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

/// A single cached reaction entry (serializable plaintext).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct CachedReaction {
    pub message_id: String,
    pub emoji: String,
    pub sender_hash: String,
    pub sender_name: String,
    pub timestamp: u64,
}

/// AES-256-GCM encrypted local message cache.
///
/// Messages and reactions are stored in memory keyed by `channel_id`
/// and persisted to encrypted files on disk.  The encryption key is
/// derived from the identity seed via HKDF so the files are only
/// readable with the correct seed.
pub(crate) struct LocalMessageCache {
    messages: HashMap<u32, Vec<CachedMessage>>,
    /// Per-channel set of `message_id`s present in `messages`, used for
    /// O(1) dedup on insert.  Rebuilt from `messages` after load.
    message_ids: HashMap<u32, HashSet<String>>,
    reactions: HashMap<u32, Vec<CachedReaction>>,
    cache_key: LessSafeKey,
    cache_path: PathBuf,
    reaction_cache_path: PathBuf,
}

impl LocalMessageCache {
    /// Create a new empty cache backed by the given identity directory and seed.
    pub fn new(identity_dir: &Path, seed: &[u8; 32]) -> Result<Self, String> {
        let cache_key = Self::derive_key(seed)?;
        Ok(Self {
            messages: HashMap::new(),
            message_ids: HashMap::new(),
            reactions: HashMap::new(),
            cache_key,
            cache_path: identity_dir.join(CACHE_FILE),
            reaction_cache_path: identity_dir.join(REACTION_CACHE_FILE),
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
    ///
    /// Uses a per-channel `HashSet` for O(1) dedup and binary-search
    /// insertion to keep messages ordered by timestamp without a full
    /// re-sort.  This keeps the per-message cost O(log N) instead of
    /// the previous O(N log N), which becomes critical for long-running
    /// chats with thousands of messages (insert is called inside the
    /// `SharedState` lock).
    pub fn insert(&mut self, msg: CachedMessage) {
        let channel_id = msg.channel_id;
        let ids = self.message_ids.entry(channel_id).or_default();
        if !ids.insert(msg.message_id.clone()) {
            return;
        }
        let channel = self.messages.entry(channel_id).or_default();
        let pos = channel.partition_point(|m| m.timestamp <= msg.timestamp);
        channel.insert(pos, msg);
    }

    /// Rebuild the `message_ids` index from `messages` after load.
    fn rebuild_message_id_index(&mut self) {
        self.message_ids = self
            .messages
            .iter()
            .map(|(&channel_id, msgs)| {
                let ids: HashSet<String> =
                    msgs.iter().map(|m| m.message_id.clone()).collect();
                (channel_id, ids)
            })
            .collect();
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
                        sender_hash: Some(m.sender_hash.clone()),
                        body: m.body.clone(),
                        channel_id: m.channel_id,
                        is_own: m.is_own,
                        dm_session: None,
                        group_id: None,
                        message_id: Some(m.message_id.clone()),
                        timestamp: Some(m.timestamp),
                        is_legacy: false,
                        edited_at: None,
                        pinned: false,
                        pinned_by: None,
                        pinned_at: None,
                    })
                    .collect();
                (channel_id, chat_msgs)
            })
            .collect()
    }

    // -- Reaction methods ---------------------------------------------

    /// Insert a reaction into the cache, deduplicating by
    /// `(message_id, emoji, sender_hash)`.
    pub fn insert_reaction(&mut self, channel_id: u32, reaction: CachedReaction) {
        let channel = self.reactions.entry(channel_id).or_default();
        let exists = channel.iter().any(|r| {
            r.message_id == reaction.message_id
                && r.emoji == reaction.emoji
                && r.sender_hash == reaction.sender_hash
        });
        if !exists {
            channel.push(reaction);
        }
    }

    /// Remove a reaction from the cache by
    /// `(message_id, emoji, sender_hash)`.
    pub fn remove_reaction(
        &mut self,
        channel_id: u32,
        message_id: &str,
        emoji: &str,
        sender_hash: &str,
    ) {
        if let Some(channel) = self.reactions.get_mut(&channel_id) {
            channel.retain(|r| {
                !(r.message_id == message_id
                    && r.emoji == emoji
                    && r.sender_hash == sender_hash)
            });
            if channel.is_empty() {
                let _ = self.reactions.remove(&channel_id);
            }
        }
    }

    /// Return all cached reactions grouped by channel.
    pub fn all_reactions(&self) -> &HashMap<u32, Vec<CachedReaction>> {
        &self.reactions
    }

    // -- Persistence --------------------------------------------------

    /// Save the message cache to an AES-256-GCM encrypted file on disk.
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

    /// Save the reaction cache to an AES-256-GCM encrypted file on disk.
    pub fn save_reactions(&self) -> Result<(), String> {
        let json = serde_json::to_vec(&self.reactions)
            .map_err(|e| format!("serialize reaction cache: {e}"))?;
        let encrypted = self.encrypt(&json)?;
        std::fs::write(&self.reaction_cache_path, &encrypted)
            .map_err(|e| format!("write reaction cache: {e}"))?;
        debug!(
            path = ?self.reaction_cache_path,
            reactions = self.reaction_count(),
            "saved local reaction cache"
        );
        Ok(())
    }

    /// Load the message cache from the encrypted file on disk.
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
        self.rebuild_message_id_index();
        debug!(
            path = ?self.cache_path,
            messages = self.total_count(),
            "loaded local message cache"
        );
        Ok(())
    }

    /// Load the reaction cache from the encrypted file on disk.
    pub fn load_reactions(&mut self) -> Result<(), String> {
        if !self.reaction_cache_path.exists() {
            debug!(path = ?self.reaction_cache_path, "no local reaction cache found");
            return Ok(());
        }
        let encrypted = std::fs::read(&self.reaction_cache_path)
            .map_err(|e| format!("read reaction cache: {e}"))?;
        let json = self.decrypt(&encrypted)?;
        self.reactions = serde_json::from_slice(&json)
            .map_err(|e| format!("deserialize reaction cache: {e}"))?;
        debug!(
            path = ?self.reaction_cache_path,
            reactions = self.reaction_count(),
            "loaded local reaction cache"
        );
        Ok(())
    }

    fn total_count(&self) -> usize {
        self.messages.values().map(Vec::len).sum()
    }

    fn reaction_count(&self) -> usize {
        self.reactions.values().map(Vec::len).sum()
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

    #[test]
    fn out_of_order_inserts_are_sorted_by_timestamp() {
        // Regression: insert preserves timestamp order using binary-search
        // insertion instead of a per-call full re-sort (O(N^2) -> O(N log N)).
        let dir = TempDir::new().unwrap();
        let mut cache = LocalMessageCache::new(dir.path(), &test_seed()).unwrap();
        let make = |id: &str, ts: u64| CachedMessage {
            message_id: id.to_string(),
            channel_id: 1,
            timestamp: ts,
            sender_hash: "x".to_string(),
            sender_name: "X".to_string(),
            body: id.to_string(),
            is_own: false,
        };

        cache.insert(make("c", 300));
        cache.insert(make("a", 100));
        cache.insert(make("d", 400));
        cache.insert(make("b", 200));

        let msgs = cache.all_chat_messages();
        let ch1 = &msgs[&1];
        assert_eq!(ch1.len(), 4);
        let bodies: Vec<&str> = ch1.iter().map(|m| m.body.as_str()).collect();
        assert_eq!(bodies, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn dedup_index_rebuilt_after_load() {
        // Regression: after load(), the message_ids index must be
        // populated so subsequent inserts of existing ids are deduped.
        let dir = TempDir::new().unwrap();
        let seed = test_seed();
        let mut cache = LocalMessageCache::new(dir.path(), &seed).unwrap();
        let msg = CachedMessage {
            message_id: "persist-1".to_string(),
            channel_id: 1,
            timestamp: 100,
            sender_hash: "x".to_string(),
            sender_name: "X".to_string(),
            body: "first".to_string(),
            is_own: false,
        };
        cache.insert(msg.clone());
        cache.save().unwrap();

        let mut cache2 = LocalMessageCache::new(dir.path(), &seed).unwrap();
        cache2.load().unwrap();
        // Re-inserting the same id after load must NOT add a duplicate.
        cache2.insert(msg);
        assert_eq!(cache2.total_count(), 1);
    }

    // -- Reaction tests -----------------------------------------------

    fn test_reaction(message_id: &str, emoji: &str, sender_hash: &str) -> CachedReaction {
        CachedReaction {
            message_id: message_id.to_string(),
            emoji: emoji.to_string(),
            sender_hash: sender_hash.to_string(),
            sender_name: format!("User-{sender_hash}"),
            timestamp: 1000,
        }
    }

    #[test]
    fn reaction_insert_and_dedup() {
        let dir = TempDir::new().unwrap();
        let mut cache = LocalMessageCache::new(dir.path(), &test_seed()).unwrap();

        let r = test_reaction("msg-1", "\u{1f44d}", "alice");
        cache.insert_reaction(5, r.clone());
        cache.insert_reaction(5, r);
        assert_eq!(cache.reaction_count(), 1);
    }

    #[test]
    fn reaction_remove() {
        let dir = TempDir::new().unwrap();
        let mut cache = LocalMessageCache::new(dir.path(), &test_seed()).unwrap();

        cache.insert_reaction(5, test_reaction("msg-1", "\u{1f44d}", "alice"));
        cache.insert_reaction(5, test_reaction("msg-1", "\u{2764}", "alice"));
        assert_eq!(cache.reaction_count(), 2);

        cache.remove_reaction(5, "msg-1", "\u{1f44d}", "alice");
        assert_eq!(cache.reaction_count(), 1);

        let remaining = &cache.all_reactions()[&5];
        assert_eq!(remaining[0].emoji, "\u{2764}");
    }

    #[test]
    fn reaction_remove_cleans_empty_channel() {
        let dir = TempDir::new().unwrap();
        let mut cache = LocalMessageCache::new(dir.path(), &test_seed()).unwrap();

        cache.insert_reaction(5, test_reaction("msg-1", "\u{1f44d}", "alice"));
        cache.remove_reaction(5, "msg-1", "\u{1f44d}", "alice");
        assert!(cache.all_reactions().is_empty());
    }

    #[test]
    fn reaction_save_load_round_trip() {
        let dir = TempDir::new().unwrap();
        let seed = test_seed();

        let mut cache = LocalMessageCache::new(dir.path(), &seed).unwrap();
        cache.insert_reaction(5, test_reaction("msg-1", "\u{1f44d}", "alice"));
        cache.insert_reaction(5, test_reaction("msg-1", "\u{2764}", "bob"));
        cache.insert_reaction(7, test_reaction("msg-2", "\u{1f389}", "carol"));
        cache.save_reactions().unwrap();

        let mut cache2 = LocalMessageCache::new(dir.path(), &seed).unwrap();
        cache2.load_reactions().unwrap();

        assert_eq!(cache2.reaction_count(), 3);
        let ch5 = &cache2.all_reactions()[&5];
        assert_eq!(ch5.len(), 2);
        let ch7 = &cache2.all_reactions()[&7];
        assert_eq!(ch7.len(), 1);
        assert_eq!(ch7[0].emoji, "\u{1f389}");
    }

    #[test]
    fn reaction_cache_independent_from_message_cache() {
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
        cache.insert_reaction(5, test_reaction("msg-1", "\u{1f44d}", "abc"));
        cache.save().unwrap();
        cache.save_reactions().unwrap();

        // Load only messages -- reactions should still be empty.
        let mut cache2 = LocalMessageCache::new(dir.path(), &seed).unwrap();
        cache2.load().unwrap();
        assert_eq!(cache2.total_count(), 1);
        assert_eq!(cache2.reaction_count(), 0);

        // Now load reactions.
        cache2.load_reactions().unwrap();
        assert_eq!(cache2.reaction_count(), 1);
    }
}
