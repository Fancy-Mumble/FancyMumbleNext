//! Message provider abstraction for persistent chat.
//!
//! [`MessageProvider`] abstracts volatile (in-memory) and persistent
//! (server-backed) message storage behind a single trait.
//! [`VolatileMessageProvider`] provides standard Mumble behaviour.
//! [`CompositeMessageProvider`] routes to volatile or persistent
//! based on each channel's [`PersistenceMode`].

use std::collections::HashMap;

use crate::error::Result;
use crate::persistent::{MessageRange, PersistenceMode, StoredMessage};

// ---- MessageProvider trait ------------------------------------------

/// Channel message provider -- abstracts volatile vs persistent channels.
///
/// Implementations may store messages in memory, on disk, or proxy to
/// a server. The trait is object-safe and `Send + Sync` so it can be
/// held inside shared application state.
pub trait MessageProvider: Send + Sync {
    /// Retrieve messages visible to the current user.
    ///
    /// Returns messages in chronological order (oldest first).
    fn get_messages(&self, channel_id: u32, range: &MessageRange) -> Result<Vec<StoredMessage>>;

    /// Store a new outgoing or incoming message.
    fn store_message(&mut self, channel_id: u32, message: StoredMessage) -> Result<()>;

    /// Replace a message identified by `replaces_id` from the same
    /// `sender_hash`. Returns `true` if a match was found and replaced.
    ///
    /// Implementations MUST search their local store for a message
    /// whose `message_id` matches `replaces_id` and whose
    /// `sender_hash` matches `replacement.sender_hash`.
    fn replace_message(
        &mut self,
        channel_id: u32,
        replaces_id: &str,
        replacement: StoredMessage,
    ) -> Result<bool>;

    /// Check if more messages are available beyond what is loaded.
    fn has_more(&self, channel_id: u32) -> bool;

    /// The persistence mode for this channel (`NONE` for legacy).
    fn mode(&self, channel_id: u32) -> PersistenceMode;
}

// ---- VolatileMessageProvider ----------------------------------------

/// Volatile provider -- standard Mumble behaviour.
///
/// Messages exist only in memory for the current session. Used for
/// channels with [`PersistenceMode::None`] or as the legacy fallback.
#[derive(Debug, Default)]
pub struct VolatileMessageProvider {
    messages: HashMap<u32, Vec<StoredMessage>>,
}

impl VolatileMessageProvider {
    /// Create an empty volatile provider.
    pub fn new() -> Self {
        Self::default()
    }

    /// Remove all messages for a channel.
    pub fn clear_channel(&mut self, channel_id: u32) {
        self.messages.remove(&channel_id);
    }

    /// Remove all messages.
    pub fn clear_all(&mut self) {
        self.messages.clear();
    }
}

impl MessageProvider for VolatileMessageProvider {
    fn get_messages(&self, channel_id: u32, range: &MessageRange) -> Result<Vec<StoredMessage>> {
        let msgs = match self.messages.get(&channel_id) {
            Some(v) => v,
            None => return Ok(Vec::new()),
        };

        Ok(apply_range(msgs, range))
    }

    fn store_message(&mut self, channel_id: u32, message: StoredMessage) -> Result<()> {
        self.messages.entry(channel_id).or_default().push(message);
        Ok(())
    }

    fn replace_message(
        &mut self,
        channel_id: u32,
        replaces_id: &str,
        replacement: StoredMessage,
    ) -> Result<bool> {
        if let Some(msgs) = self.messages.get_mut(&channel_id) {
            if let Some(pos) = msgs.iter().position(|m| {
                m.message_id == replaces_id && m.sender_hash == replacement.sender_hash
            }) {
                msgs[pos] = replacement;
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn has_more(&self, _channel_id: u32) -> bool {
        false // volatile storage never has "more" to fetch
    }

    fn mode(&self, _channel_id: u32) -> PersistenceMode {
        PersistenceMode::None
    }
}

// ---- PersistentProviderBackend trait ---------------------------------

/// Backend trait for persistent storage (local cache + server proxy).
///
/// Implementors manage a local cache of decrypted messages and can
/// issue fetch requests for older history. This trait is separate from
/// [`MessageProvider`] so that encryption, caching, and network I/O
/// concerns can be composed independently.
pub trait PersistentProviderBackend: Send + Sync {
    /// Get cached messages for a channel.
    fn cached_messages(
        &self,
        channel_id: u32,
        range: &MessageRange,
    ) -> Result<Vec<StoredMessage>>;

    /// Store a message into the local cache.
    fn cache_message(&mut self, channel_id: u32, message: StoredMessage) -> Result<()>;

    /// Replace a cached message, matching by `replaces_id` and `sender_hash`.
    fn replace_cached_message(
        &mut self,
        channel_id: u32,
        replaces_id: &str,
        replacement: StoredMessage,
    ) -> Result<bool>;

    /// Whether more messages can be fetched from the server.
    fn server_has_more(&self, channel_id: u32) -> bool;

    /// The persistence mode from the channel's server config.
    fn channel_mode(&self, channel_id: u32) -> PersistenceMode;
}

/// In-memory implementation of [`PersistentProviderBackend`].
///
/// Suitable for consumers that do not need disk persistence (the
/// default for `mumble-protocol`). The Tauri layer can provide a
/// database-backed implementation instead.
#[derive(Debug, Default)]
pub struct InMemoryPersistentBackend {
    cache: HashMap<u32, Vec<StoredMessage>>,
    modes: HashMap<u32, PersistenceMode>,
    has_more: HashMap<u32, bool>,
}

impl InMemoryPersistentBackend {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a channel's persistence mode.
    pub fn set_mode(&mut self, channel_id: u32, mode: PersistenceMode) {
        self.modes.insert(channel_id, mode);
    }

    /// Mark whether the server has more history for a channel.
    pub fn set_has_more(&mut self, channel_id: u32, has_more: bool) {
        self.has_more.insert(channel_id, has_more);
    }
}

impl PersistentProviderBackend for InMemoryPersistentBackend {
    fn cached_messages(
        &self,
        channel_id: u32,
        range: &MessageRange,
    ) -> Result<Vec<StoredMessage>> {
        let msgs = match self.cache.get(&channel_id) {
            Some(v) => v,
            None => return Ok(Vec::new()),
        };
        Ok(apply_range(msgs, range))
    }

    fn cache_message(&mut self, channel_id: u32, message: StoredMessage) -> Result<()> {
        self.cache.entry(channel_id).or_default().push(message);
        Ok(())
    }

    fn replace_cached_message(
        &mut self,
        channel_id: u32,
        replaces_id: &str,
        replacement: StoredMessage,
    ) -> Result<bool> {
        if let Some(msgs) = self.cache.get_mut(&channel_id) {
            if let Some(pos) = msgs.iter().position(|m| {
                m.message_id == replaces_id && m.sender_hash == replacement.sender_hash
            }) {
                msgs[pos] = replacement;
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn server_has_more(&self, channel_id: u32) -> bool {
        self.has_more.get(&channel_id).copied().unwrap_or(false)
    }

    fn channel_mode(&self, channel_id: u32) -> PersistenceMode {
        self.modes
            .get(&channel_id)
            .copied()
            .unwrap_or(PersistenceMode::None)
    }
}

// ---- PersistentMessageProvider (wraps a backend) --------------------

/// Persistent message provider backed by a [`PersistentProviderBackend`].
///
/// This provider delegates cache operations to the backend and exposes
/// the [`MessageProvider`] trait. Encryption/decryption is handled by
/// the caller (or a higher-level orchestrator) before data reaches
/// this provider.
pub struct PersistentMessageProvider {
    backend: Box<dyn PersistentProviderBackend>,
}

impl PersistentMessageProvider {
    pub fn new(backend: Box<dyn PersistentProviderBackend>) -> Self {
        Self { backend }
    }

    /// Access the backend for direct configuration (e.g. setting modes).
    pub fn backend_mut(&mut self) -> &mut dyn PersistentProviderBackend {
        &mut *self.backend
    }
}

impl std::fmt::Debug for PersistentMessageProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PersistentMessageProvider")
            .finish_non_exhaustive()
    }
}

impl MessageProvider for PersistentMessageProvider {
    fn get_messages(&self, channel_id: u32, range: &MessageRange) -> Result<Vec<StoredMessage>> {
        self.backend.cached_messages(channel_id, range)
    }

    fn store_message(&mut self, channel_id: u32, message: StoredMessage) -> Result<()> {
        self.backend.cache_message(channel_id, message)
    }

    fn replace_message(
        &mut self,
        channel_id: u32,
        replaces_id: &str,
        replacement: StoredMessage,
    ) -> Result<bool> {
        self.backend
            .replace_cached_message(channel_id, replaces_id, replacement)
    }

    fn has_more(&self, channel_id: u32) -> bool {
        self.backend.server_has_more(channel_id)
    }

    fn mode(&self, channel_id: u32) -> PersistenceMode {
        self.backend.channel_mode(channel_id)
    }
}

// ---- CompositeMessageProvider ---------------------------------------

/// Composite provider that delegates based on channel persistence mode.
///
/// Routes to [`VolatileMessageProvider`] for `NONE` channels and to
/// [`PersistentMessageProvider`] for encrypted channels. The
/// `replace_message` method searches BOTH providers because the
/// original may be a plaintext `TextMessage` in volatile storage even
/// though the replacement is an encrypted persistent message (epoch
/// fork re-send, design doc section 6.2).
pub struct CompositeMessageProvider {
    volatile: VolatileMessageProvider,
    persistent: PersistentMessageProvider,
}

impl CompositeMessageProvider {
    pub fn new(
        volatile: VolatileMessageProvider,
        persistent: PersistentMessageProvider,
    ) -> Self {
        Self {
            volatile,
            persistent,
        }
    }

    /// Access the volatile sub-provider.
    pub fn volatile(&self) -> &VolatileMessageProvider {
        &self.volatile
    }

    /// Access the volatile sub-provider mutably.
    pub fn volatile_mut(&mut self) -> &mut VolatileMessageProvider {
        &mut self.volatile
    }

    /// Access the persistent sub-provider.
    pub fn persistent(&self) -> &PersistentMessageProvider {
        &self.persistent
    }

    /// Access the persistent sub-provider mutably.
    pub fn persistent_mut(&mut self) -> &mut PersistentMessageProvider {
        &mut self.persistent
    }
}

impl std::fmt::Debug for CompositeMessageProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompositeMessageProvider")
            .field("volatile", &self.volatile)
            .finish_non_exhaustive()
    }
}

impl MessageProvider for CompositeMessageProvider {
    fn get_messages(&self, channel_id: u32, range: &MessageRange) -> Result<Vec<StoredMessage>> {
        match self.effective_mode(channel_id) {
            PersistenceMode::PostJoin | PersistenceMode::FullArchive => {
                self.persistent.get_messages(channel_id, range)
            }
            _ => self.volatile.get_messages(channel_id, range),
        }
    }

    fn store_message(&mut self, channel_id: u32, message: StoredMessage) -> Result<()> {
        match self.effective_mode(channel_id) {
            PersistenceMode::PostJoin | PersistenceMode::FullArchive => {
                self.persistent.store_message(channel_id, message)
            }
            _ => self.volatile.store_message(channel_id, message),
        }
    }

    fn replace_message(
        &mut self,
        channel_id: u32,
        replaces_id: &str,
        replacement: StoredMessage,
    ) -> Result<bool> {
        // Search BOTH providers: the original may be a plaintext
        // TextMessage in volatile (from real-time delivery) even
        // though the replacement is an encrypted persistent message.
        if self
            .volatile
            .replace_message(channel_id, replaces_id, replacement.clone())?
        {
            return Ok(true);
        }
        self.persistent
            .replace_message(channel_id, replaces_id, replacement)
    }

    fn has_more(&self, channel_id: u32) -> bool {
        match self.effective_mode(channel_id) {
            PersistenceMode::PostJoin | PersistenceMode::FullArchive => {
                self.persistent.has_more(channel_id)
            }
            _ => self.volatile.has_more(channel_id),
        }
    }

    fn mode(&self, channel_id: u32) -> PersistenceMode {
        self.effective_mode(channel_id)
    }
}

impl CompositeMessageProvider {
    fn effective_mode(&self, channel_id: u32) -> PersistenceMode {
        self.persistent.mode(channel_id)
    }
}

// ---- Range application helper ---------------------------------------

/// Apply a [`MessageRange`] to a chronologically-ordered message slice.
fn apply_range(msgs: &[StoredMessage], range: &MessageRange) -> Vec<StoredMessage> {
    match range {
        MessageRange::Latest(limit) => {
            let start = msgs.len().saturating_sub(*limit);
            msgs[start..].to_vec()
        }
        MessageRange::Before { message_id, limit } => {
            if let Some(pos) = msgs.iter().position(|m| m.message_id == *message_id) {
                let start = pos.saturating_sub(*limit);
                msgs[start..pos].to_vec()
            } else {
                Vec::new()
            }
        }
        MessageRange::After { message_id, limit } => {
            if let Some(pos) = msgs.iter().position(|m| m.message_id == *message_id) {
                let end = (pos + 1 + *limit).min(msgs.len());
                msgs[pos + 1..end].to_vec()
            } else {
                Vec::new()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_message(id: &str, channel: u32, sender: &str) -> StoredMessage {
        StoredMessage {
            message_id: id.to_string(),
            channel_id: channel,
            timestamp: 0,
            sender_hash: sender.to_string(),
            sender_name: sender.to_string(),
            body: format!("body-{id}"),
            encrypted: false,
            epoch: None,
            chain_index: None,
            replaces_id: None,
        }
    }

    // ---- VolatileMessageProvider ------------------------------------

    #[test]
    fn volatile_store_and_retrieve() {
        let mut vp = VolatileMessageProvider::new();
        vp.store_message(1, make_message("a", 1, "alice"))
            .unwrap();
        vp.store_message(1, make_message("b", 1, "bob")).unwrap();

        let msgs = vp.get_messages(1, &MessageRange::Latest(10)).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].message_id, "a");
        assert_eq!(msgs[1].message_id, "b");
    }

    #[test]
    fn volatile_latest_limits() {
        let mut vp = VolatileMessageProvider::new();
        for i in 0..5 {
            vp.store_message(1, make_message(&i.to_string(), 1, "alice"))
                .unwrap();
        }
        let msgs = vp.get_messages(1, &MessageRange::Latest(3)).unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].message_id, "2");
    }

    #[test]
    fn volatile_empty_channel() {
        let vp = VolatileMessageProvider::new();
        let msgs = vp.get_messages(99, &MessageRange::Latest(10)).unwrap();
        assert!(msgs.is_empty());
    }

    #[test]
    fn volatile_replace_message() {
        let mut vp = VolatileMessageProvider::new();
        vp.store_message(1, make_message("a", 1, "alice"))
            .unwrap();

        let replacement = StoredMessage {
            body: "replaced".into(),
            ..make_message("a-new", 1, "alice")
        };
        let result = vp.replace_message(1, "a", replacement).unwrap();
        assert!(result);

        let msgs = vp.get_messages(1, &MessageRange::Latest(10)).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].body, "replaced");
    }

    #[test]
    fn volatile_replace_wrong_sender_fails() {
        let mut vp = VolatileMessageProvider::new();
        vp.store_message(1, make_message("a", 1, "alice"))
            .unwrap();

        let replacement = make_message("a-new", 1, "bob");
        let result = vp.replace_message(1, "a", replacement).unwrap();
        assert!(!result);
    }

    #[test]
    fn volatile_mode_is_none() {
        let vp = VolatileMessageProvider::new();
        assert_eq!(vp.mode(1), PersistenceMode::None);
    }

    #[test]
    fn volatile_has_more_is_false() {
        let vp = VolatileMessageProvider::new();
        assert!(!vp.has_more(1));
    }

    #[test]
    fn volatile_clear_channel() {
        let mut vp = VolatileMessageProvider::new();
        vp.store_message(1, make_message("a", 1, "alice"))
            .unwrap();
        vp.store_message(2, make_message("b", 2, "bob")).unwrap();
        vp.clear_channel(1);
        assert!(vp.get_messages(1, &MessageRange::Latest(10)).unwrap().is_empty());
        assert_eq!(vp.get_messages(2, &MessageRange::Latest(10)).unwrap().len(), 1);
    }

    // ---- MessageRange application -----------------------------------

    #[test]
    fn range_before() {
        let mut vp = VolatileMessageProvider::new();
        for i in 0..5 {
            vp.store_message(1, make_message(&i.to_string(), 1, "alice"))
                .unwrap();
        }
        let msgs = vp
            .get_messages(
                1,
                &MessageRange::Before {
                    message_id: "3".into(),
                    limit: 2,
                },
            )
            .unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].message_id, "1");
        assert_eq!(msgs[1].message_id, "2");
    }

    #[test]
    fn range_after() {
        let mut vp = VolatileMessageProvider::new();
        for i in 0..5 {
            vp.store_message(1, make_message(&i.to_string(), 1, "alice"))
                .unwrap();
        }
        let msgs = vp
            .get_messages(
                1,
                &MessageRange::After {
                    message_id: "1".into(),
                    limit: 2,
                },
            )
            .unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].message_id, "2");
        assert_eq!(msgs[1].message_id, "3");
    }

    #[test]
    fn range_before_unknown_cursor() {
        let mut vp = VolatileMessageProvider::new();
        vp.store_message(1, make_message("a", 1, "alice"))
            .unwrap();
        let msgs = vp
            .get_messages(
                1,
                &MessageRange::Before {
                    message_id: "nonexistent".into(),
                    limit: 10,
                },
            )
            .unwrap();
        assert!(msgs.is_empty());
    }

    // ---- InMemoryPersistentBackend ----------------------------------

    #[test]
    fn in_memory_backend_modes() {
        let mut backend = InMemoryPersistentBackend::new();
        assert_eq!(backend.channel_mode(1), PersistenceMode::None);

        backend.set_mode(1, PersistenceMode::PostJoin);
        assert_eq!(backend.channel_mode(1), PersistenceMode::PostJoin);
    }

    #[test]
    fn in_memory_backend_has_more() {
        let mut backend = InMemoryPersistentBackend::new();
        assert!(!backend.server_has_more(1));

        backend.set_has_more(1, true);
        assert!(backend.server_has_more(1));
    }

    // ---- CompositeMessageProvider -----------------------------------

    #[test]
    fn composite_routes_to_volatile_for_none() {
        let mut backend = InMemoryPersistentBackend::new();
        backend.set_mode(1, PersistenceMode::None);

        let mut composite = CompositeMessageProvider::new(
            VolatileMessageProvider::new(),
            PersistentMessageProvider::new(Box::new(backend)),
        );

        composite
            .store_message(1, make_message("a", 1, "alice"))
            .unwrap();

        let msgs = composite.get_messages(1, &MessageRange::Latest(10)).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].message_id, "a");

        // Verify it went into volatile, not persistent
        assert_eq!(
            composite.volatile().get_messages(1, &MessageRange::Latest(10)).unwrap().len(),
            1
        );
    }

    #[test]
    fn composite_routes_to_persistent_for_post_join() {
        let mut backend = InMemoryPersistentBackend::new();
        backend.set_mode(1, PersistenceMode::PostJoin);

        let mut composite = CompositeMessageProvider::new(
            VolatileMessageProvider::new(),
            PersistentMessageProvider::new(Box::new(backend)),
        );

        composite
            .store_message(1, make_message("a", 1, "alice"))
            .unwrap();

        let msgs = composite.get_messages(1, &MessageRange::Latest(10)).unwrap();
        assert_eq!(msgs.len(), 1);

        // Verify volatile is empty
        assert!(composite
            .volatile()
            .get_messages(1, &MessageRange::Latest(10))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn composite_replace_searches_both_providers() {
        let mut backend = InMemoryPersistentBackend::new();
        backend.set_mode(1, PersistenceMode::PostJoin);

        let mut composite = CompositeMessageProvider::new(
            VolatileMessageProvider::new(),
            PersistentMessageProvider::new(Box::new(backend)),
        );

        // Store original in volatile (simulating real-time TextMessage delivery)
        composite
            .volatile_mut()
            .store_message(1, make_message("orig", 1, "alice"))
            .unwrap();

        // Replace should find it in volatile even though mode is PostJoin
        let replacement = StoredMessage {
            body: "replaced".into(),
            ..make_message("new", 1, "alice")
        };
        let found = composite.replace_message(1, "orig", replacement).unwrap();
        assert!(found);

        let msgs = composite
            .volatile()
            .get_messages(1, &MessageRange::Latest(10))
            .unwrap();
        assert_eq!(msgs[0].body, "replaced");
    }

    #[test]
    fn composite_mode_reflects_persistent_config() {
        let mut backend = InMemoryPersistentBackend::new();
        backend.set_mode(1, PersistenceMode::FullArchive);
        backend.set_mode(2, PersistenceMode::None);

        let composite = CompositeMessageProvider::new(
            VolatileMessageProvider::new(),
            PersistentMessageProvider::new(Box::new(backend)),
        );

        assert_eq!(composite.mode(1), PersistenceMode::FullArchive);
        assert_eq!(composite.mode(2), PersistenceMode::None);
        assert_eq!(composite.mode(99), PersistenceMode::None);
    }
}
