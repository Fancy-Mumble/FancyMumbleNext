#![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
use std::sync::{Arc, Mutex};

use serde_json::Value;

use mumble_protocol::message::ControlMessage;
use mumble_protocol::proto::mumble_tcp;
use mumble_protocol::persistent::PchatProtocol;

use super::{dispatch, EventEmitter, HandleMessage, HandlerContext};
use crate::state::hash_names::HashNameResolver;
use crate::state::types::*;
use crate::state::SharedState;

// -- Test infrastructure -------------------------------------------

/// Mock event emitter that records all emitted events.
struct MockEmitter {
    events: Mutex<Vec<(String, Value)>>,
    attention_count: Mutex<u32>,
}

impl MockEmitter {
    fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
            attention_count: Mutex::new(0),
        }
    }

    fn events(&self) -> Vec<(String, Value)> {
        self.events.lock().unwrap().clone()
    }

    fn event_names(&self) -> Vec<String> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .map(|(n, _)| n.clone())
            .collect()
    }

    fn attention_count(&self) -> u32 {
        *self.attention_count.lock().unwrap()
    }
}

impl EventEmitter for MockEmitter {
    fn emit_json(&self, event: &str, payload: Value) {
        self.events
            .lock()
            .unwrap()
            .push((event.to_string(), payload));
    }

    fn request_user_attention(&self) {
        *self.attention_count.lock().unwrap() += 1;
    }

    fn send_notification(&self, _title: &str, _body: &str) {
        // No-op in tests.
    }
}

/// Wrapper so we can share the mock emitter between test code and the handler.
struct ArcEmitter(Arc<MockEmitter>);

impl EventEmitter for ArcEmitter {
    fn emit_json(&self, event: &str, payload: Value) {
        self.0.emit_json(event, payload);
    }

    fn request_user_attention(&self) {
        self.0.request_user_attention();
    }

    fn send_notification(&self, title: &str, body: &str) {
        self.0.send_notification(title, body);
    }
}

fn make_ctx() -> (HandlerContext, Arc<MockEmitter>) {
    let emitter = Arc::new(MockEmitter::new());
    let ctx = HandlerContext {
        shared: Arc::new(Mutex::new(SharedState::default())),
        emitter: Box::new(ArcEmitter(Arc::clone(&emitter))),
    };
    (ctx, emitter)
}

fn make_user(session: u32, name: &str) -> UserEntry {
    UserEntry {
        session,
        name: name.into(),
        channel_id: 0,
        user_id: None,
        texture: None,
        comment: None,
        mute: false,
        deaf: false,
        suppress: false,
        self_mute: false,
        self_deaf: false,
        priority_speaker: false,
        hash: None,
        client_features: Vec::new(),
    }
}

/// Create a user that advertises E2EE persistent chat support.
fn make_e2ee_user(session: u32, name: &str) -> UserEntry {
    use mumble_protocol::proto::mumble_tcp::user_state::ClientFeature;
    let mut u = make_user(session, name);
    u.client_features = vec![ClientFeature::FeaturePchatE2ee as i32];
    u
}

// -- Ping ----------------------------------------------------------

#[test]
fn ping_with_timestamp_emits_latency() {
    let (ctx, emitter) = make_ctx();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let ping = mumble_tcp::Ping {
        timestamp: Some(now),
        ..Default::default()
    };
    ping.handle(&ctx);
    let events = emitter.event_names();
    assert_eq!(events, vec!["ping-latency"]);
}

#[test]
fn ping_without_timestamp_emits_nothing() {
    let (ctx, emitter) = make_ctx();
    let ping = mumble_tcp::Ping::default();
    ping.handle(&ctx);
    assert!(emitter.events().is_empty());
}

// -- Version -------------------------------------------------------

#[test]
fn version_updates_state() {
    let (ctx, emitter) = make_ctx();
    let version = mumble_tcp::Version {
        release: Some("Mumble 1.5".into()),
        os: Some("Linux".into()),
        os_version: Some("5.15".into()),
        version_v1: Some(0x0001_0500),
        version_v2: Some(42),
        fancy_version: Some(mumble_protocol::state::fancy_version_encode(0, 1, 0)),
    };
    version.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert_eq!(
        state.server.fancy_version,
        Some(mumble_protocol::state::fancy_version_encode(0, 1, 0))
    );
    assert_eq!(
        state.server.version_info.release.as_deref(),
        Some("Mumble 1.5")
    );
    assert_eq!(state.server.version_info.os.as_deref(), Some("Linux"));
    assert_eq!(
        state.server.version_info.os_version.as_deref(),
        Some("5.15")
    );
    assert_eq!(state.server.version_info.version_v1, Some(0x0001_0500));
    assert_eq!(state.server.version_info.version_v2, Some(42));
    drop(state);

    // Version handler emits no events.
    assert!(emitter.events().is_empty());
}

// -- ServerSync ----------------------------------------------------

#[tokio::test]
async fn server_sync_sets_connected_state() {
    let (ctx, emitter) = make_ctx();
    let sync = mumble_tcp::ServerSync {
        session: Some(42),
        max_bandwidth: Some(72000),
        welcome_text: Some("Welcome!".into()),
        ..Default::default()
    };
    sync.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert_eq!(state.conn.status, ConnectionStatus::Connected);
    assert_eq!(state.conn.own_session, Some(42));
    assert!(state.conn.synced);
    assert_eq!(state.server.max_bandwidth, Some(72000));
    assert_eq!(state.server.welcome_text.as_deref(), Some("Welcome!"));
    drop(state);

    let names = emitter.event_names();
    assert!(names.contains(&"server-connected".to_string()));
}

#[tokio::test]
async fn server_sync_resolves_initial_channel() {
    let (ctx, emitter) = make_ctx();
    // Pre-populate a user (as if UserState arrived before ServerSync).
    {
        let mut state = ctx.shared.lock().unwrap();
        let mut user = make_user(42, "TestUser");
        user.channel_id = 5;
        let _ = state.users.insert(42, user);
    }

    let sync = mumble_tcp::ServerSync {
        session: Some(42),
        ..Default::default()
    };
    sync.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert_eq!(state.current_channel, Some(5));
    drop(state);

    let names = emitter.event_names();
    assert!(names.contains(&"current-channel-changed".to_string()));
}

#[tokio::test]
async fn server_sync_without_user_does_not_set_channel() {
    let (ctx, emitter) = make_ctx();

    let sync = mumble_tcp::ServerSync {
        session: Some(42),
        ..Default::default()
    };
    sync.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert_eq!(state.current_channel, None);
    drop(state);

    let names = emitter.event_names();
    assert!(!names.contains(&"current-channel-changed".to_string()));
}

#[tokio::test]
async fn server_sync_stores_root_channel_permissions() {
    let (ctx, _emitter) = make_ctx();

    // Pre-populate root channel (as if a ChannelState arrived before ServerSync).
    {
        let mut state = ctx.shared.lock().unwrap();
        let _ = state.channels.insert(
            0,
            ChannelEntry {
                id: 0,
                parent_id: None,
                name: "Root".into(),
                description: String::new(),
                description_hash: None,
                user_count: 0,
                permissions: None,
                temporary: false,
                position: 0,
                max_users: 0,
                pchat_protocol: None,
                pchat_max_history: None,
                pchat_retention_days: None,
                pchat_key_custodians: Vec::new(),
            },
        );
    }

    // ServerSync carries permissions for the root channel.  SuperUser
    // typically gets all bits set.
    let sync = mumble_tcp::ServerSync {
        session: Some(1),
        permissions: Some(0x7FFF_FFFF),
        ..Default::default()
    };
    sync.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert_eq!(
        state.channels[&0].permissions,
        Some(0x7FFF_FFFF_u32),
        "ServerSync.permissions should be stored on channel 0",
    );
}

// -- UserState -----------------------------------------------------

#[test]
fn user_state_inserts_new_user() {
    let (ctx, _) = make_ctx();
    let us = mumble_tcp::UserState {
        session: Some(10),
        name: Some("Alice".into()),
        channel_id: Some(1),
        ..Default::default()
    };
    us.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    let user = state.users.get(&10).unwrap();
    assert_eq!(user.name, "Alice");
    assert_eq!(user.channel_id, 1);
}

#[test]
fn user_state_updates_existing_user() {
    let (ctx, _) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        let _ = state.users.insert(10, make_user(10, "Alice"));
    }

    let us = mumble_tcp::UserState {
        session: Some(10),
        channel_id: Some(5),
        self_mute: Some(true),
        ..Default::default()
    };
    us.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    let user = state.users.get(&10).unwrap();
    assert_eq!(user.name, "Alice"); // unchanged
    assert_eq!(user.channel_id, 5); // updated
    assert!(user.self_mute);
}

#[test]
fn user_state_updates_all_boolean_fields() {
    let (ctx, _) = make_ctx();
    let us = mumble_tcp::UserState {
        session: Some(10),
        name: Some("Bob".into()),
        mute: Some(true),
        deaf: Some(true),
        suppress: Some(true),
        self_mute: Some(true),
        self_deaf: Some(true),
        priority_speaker: Some(true),
        ..Default::default()
    };
    us.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    let user = state.users.get(&10).unwrap();
    assert!(user.mute);
    assert!(user.deaf);
    assert!(user.suppress);
    assert!(user.self_mute);
    assert!(user.self_deaf);
    assert!(user.priority_speaker);
}

#[test]
fn user_state_texture_empty_clears() {
    let (ctx, _) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        let mut user = make_user(10, "Alice");
        user.texture = Some(vec![1, 2, 3]);
        let _ = state.users.insert(10, user);
    }

    let us = mumble_tcp::UserState {
        session: Some(10),
        texture: Some(vec![]),
        ..Default::default()
    };
    us.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert_eq!(state.users[&10].texture, None);
}

#[test]
fn user_state_comment_empty_clears() {
    let (ctx, _) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        let mut user = make_user(10, "Alice");
        user.comment = Some("old comment".into());
        let _ = state.users.insert(10, user);
    }

    let us = mumble_tcp::UserState {
        session: Some(10),
        comment: Some(String::new()),
        ..Default::default()
    };
    us.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert_eq!(state.users[&10].comment, None);
}

#[test]
fn user_state_emits_channel_change_for_own_user() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.conn.own_session = Some(10);
        state.conn.synced = true;
    }

    let us = mumble_tcp::UserState {
        session: Some(10),
        channel_id: Some(3),
        ..Default::default()
    };
    us.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert_eq!(state.current_channel, Some(3));
    drop(state);

    let names = emitter.event_names();
    assert!(names.contains(&"current-channel-changed".to_string()));
    assert!(names.contains(&"state-changed".to_string()));
}

#[test]
fn user_state_does_not_emit_before_sync() {
    let (ctx, emitter) = make_ctx();
    // synced is false by default
    let us = mumble_tcp::UserState {
        session: Some(10),
        name: Some("Alice".into()),
        ..Default::default()
    };
    us.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert!(state.users.contains_key(&10));
    drop(state);

    assert!(!emitter.event_names().contains(&"state-changed".to_string()));
}

#[test]
fn user_state_no_session_is_noop() {
    let (ctx, emitter) = make_ctx();
    let us = mumble_tcp::UserState::default();
    us.handle(&ctx);
    assert!(emitter.events().is_empty());
    assert!(ctx.shared.lock().unwrap().users.is_empty());
}

/// When a post-sync `UserState` arrives with `texture_hash` but no `texture`,
/// the handler must request the full blob so the avatar becomes visible.
/// (Before this fix, `texture_hash` was silently ignored for post-sync updates,
/// causing missing avatars when a user joins *after* us and sets their texture.)
#[tokio::test]
async fn user_state_texture_hash_triggers_blob_request() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.conn.synced = true;
        state.conn.own_session = Some(1);
        let _ = state.users.insert(10, make_user(10, "Alice"));
    }

    // Server sends UserState with texture_hash but no texture
    // (this is what happens when a client >= 1.2.2 receives a
    // texture update from the server).
    let us = mumble_tcp::UserState {
        session: Some(10),
        texture_hash: Some(vec![0xDE, 0xAD, 0xBE, 0xEF]),
        ..Default::default()
    };
    us.handle(&ctx);

    // The handler spawns a task to request the blob.  Give it a
    // moment to run (it will find no client_handle and exit gracefully).
    tokio::task::yield_now().await;

    // User is still present and the texture is unchanged (the blob
    // response would fill it in later).
    let state = ctx.shared.lock().unwrap();
    assert!(state.users.contains_key(&10));
    drop(state);

    // The handler should emit state-changed since we are synced.
    assert!(emitter.event_names().contains(&"state-changed".to_string()));
}

/// Same as above but for `comment_hash` without comment.
#[tokio::test]
async fn user_state_comment_hash_triggers_blob_request() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.conn.synced = true;
        state.conn.own_session = Some(1);
        let _ = state.users.insert(10, make_user(10, "Alice"));
    }

    let us = mumble_tcp::UserState {
        session: Some(10),
        comment_hash: Some(vec![0xCA, 0xFE]),
        ..Default::default()
    };
    us.handle(&ctx);

    tokio::task::yield_now().await;

    let state = ctx.shared.lock().unwrap();
    assert!(state.users.contains_key(&10));
    drop(state);

    assert!(emitter.event_names().contains(&"state-changed".to_string()));
}

/// When the full texture is included alongside the hash, no blob request
/// should be needed - the texture is applied directly.
#[test]
fn user_state_full_texture_does_not_need_blob() {
    let (ctx, _) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.conn.synced = true;
        let _ = state.users.insert(10, make_user(10, "Alice"));
    }

    let us = mumble_tcp::UserState {
        session: Some(10),
        texture: Some(vec![0xFF, 0xD8, 0xFF, 0xE0]), // JPEG magic
        texture_hash: Some(vec![0xAB, 0xCD]),
        ..Default::default()
    };
    us.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert_eq!(state.users[&10].texture, Some(vec![0xFF, 0xD8, 0xFF, 0xE0]));
}

/// Before initial sync, `texture_hash` should NOT trigger a blob request
/// (the bulk `request_user_blobs` call in `ServerSync` handles it).
#[test]
fn user_state_texture_hash_before_sync_no_blob() {
    let (ctx, emitter) = make_ctx();
    // synced = false (default)

    let us = mumble_tcp::UserState {
        session: Some(10),
        name: Some("Alice".into()),
        texture_hash: Some(vec![0xDE, 0xAD]),
        ..Default::default()
    };
    us.handle(&ctx);

    // User inserted but no events (not synced yet).
    let state = ctx.shared.lock().unwrap();
    assert!(state.users.contains_key(&10));
    drop(state);

    // No state-changed event should be emitted before sync.
    assert!(!emitter.event_names().contains(&"state-changed".to_string()));
}

// -- UserRemove ----------------------------------------------------

#[test]
fn user_remove_other_user() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.conn.own_session = Some(1);
        let _ = state.users.insert(10, make_user(10, "Alice"));
    }

    let ur = mumble_tcp::UserRemove {
        session: 10,
        ..Default::default()
    };
    ur.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert!(!state.users.contains_key(&10));
    drop(state);

    assert!(emitter.event_names().contains(&"state-changed".to_string()));
}

#[test]
fn user_remove_self_kicks_and_cleans_up() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.conn.own_session = Some(42);
        state.conn.status = ConnectionStatus::Connected;
        state.conn.synced = true;
        let _ = state.users.insert(42, make_user(42, "Me"));
    }

    let ur = mumble_tcp::UserRemove {
        session: 42,
        reason: Some("Banned".into()),
        ..Default::default()
    };
    ur.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert_eq!(state.conn.status, ConnectionStatus::Disconnected);
    assert!(state.users.is_empty());
    assert!(!state.conn.synced);
    assert_eq!(state.conn.own_session, None);
    drop(state);

    let names = emitter.event_names();
    assert!(names.contains(&"connection-rejected".to_string()));
    assert!(names.contains(&"server-disconnected".to_string()));
}

#[test]
fn user_remove_self_default_reason() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.conn.own_session = Some(42);
    }

    let ur = mumble_tcp::UserRemove {
        session: 42,
        ..Default::default()
    };
    ur.handle(&ctx);

    let events = emitter.events();
    let rejected = events.iter().find(|(n, _)| n == "connection-rejected");
    assert!(rejected.is_some());
    assert_eq!(
        rejected.unwrap().1["reason"].as_str().unwrap(),
        "Disconnected by server"
    );
}

#[test]
fn user_remove_clears_pending_key_shares() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.conn.own_session = Some(1);
        let mut alice = make_user(10, "Alice");
        alice.hash = Some("abc123".into());
        let _ = state.users.insert(10, alice);
        state.pchat_ctx.pending_key_shares.push(PendingKeyShare {
            channel_id: 5,
            peer_cert_hash: "abc123".into(),
            peer_name: "Alice".into(),
            request_id: None,
        });
        state.pchat_ctx.pending_key_shares.push(PendingKeyShare {
            channel_id: 7,
            peer_cert_hash: "other_hash".into(),
            peer_name: "Bob".into(),
            request_id: None,
        });
    }

    let ur = mumble_tcp::UserRemove {
        session: 10,
        ..Default::default()
    };
    ur.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    // Alice's pending share removed, Bob's remains.
    assert_eq!(state.pchat_ctx.pending_key_shares.len(), 1);
    assert_eq!(state.pchat_ctx.pending_key_shares[0].peer_cert_hash, "other_hash");
    drop(state);

    let names = emitter.event_names();
    assert!(names.contains(&"pchat-key-share-requests-changed".to_string()));
    assert!(names.contains(&"state-changed".to_string()));
}

// -- ChannelState --------------------------------------------------

#[tokio::test]
async fn channel_state_inserts_new_channel() {
    let (ctx, _) = make_ctx();
    let cs = mumble_tcp::ChannelState {
        channel_id: Some(1),
        parent: Some(0),
        name: Some("Root".into()),
        description: Some("Welcome".into()),
        ..Default::default()
    };
    cs.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    let ch = state.channels.get(&1).unwrap();
    assert_eq!(ch.name, "Root");
    assert_eq!(ch.description, "Welcome");
    assert_eq!(ch.parent_id, Some(0));
}

#[tokio::test]
async fn channel_state_updates_existing_channel() {
    let (ctx, _) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        let _ = state.channels.insert(
            1,
            ChannelEntry {
                id: 1,
                parent_id: Some(0),
                name: "Old".into(),
                description: String::new(),
                description_hash: None,
                user_count: 0,
                permissions: None,
                temporary: false,
                position: 0,
                max_users: 0,
                pchat_protocol: None,
                pchat_max_history: None,
                pchat_retention_days: None,
                    pchat_key_custodians: Vec::new(),            },
        );
    }

    let cs = mumble_tcp::ChannelState {
        channel_id: Some(1),
        name: Some("New".into()),
        description: Some("Updated".into()),
        ..Default::default()
    };
    cs.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    let ch = state.channels.get(&1).unwrap();
    assert_eq!(ch.name, "New");
    assert_eq!(ch.description, "Updated");
}

#[tokio::test]
async fn channel_state_emits_when_synced() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.conn.synced = true;
    }

    let cs = mumble_tcp::ChannelState {
        channel_id: Some(1),
        name: Some("Test".into()),
        ..Default::default()
    };
    cs.handle(&ctx);

    assert!(emitter.event_names().contains(&"state-changed".to_string()));
}

#[tokio::test]
async fn channel_state_no_id_is_noop() {
    let (ctx, emitter) = make_ctx();
    let cs = mumble_tcp::ChannelState::default();
    cs.handle(&ctx);
    assert!(emitter.events().is_empty());
    assert!(ctx.shared.lock().unwrap().channels.is_empty());
}

#[tokio::test]
async fn channel_state_stores_description_hash() {
    let (ctx, _) = make_ctx();
    let hash = vec![0xAB, 0xCD];
    let cs = mumble_tcp::ChannelState {
        channel_id: Some(1),
        description_hash: Some(hash.clone()),
        ..Default::default()
    };
    cs.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert_eq!(state.channels[&1].description_hash, Some(hash));
}

// -- ChannelRemove -------------------------------------------------

#[test]
fn channel_remove_clears_channel_and_messages() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        let _ = state.channels.insert(
            5,
            ChannelEntry {
                id: 5,
                parent_id: Some(0),
                name: "Test".into(),
                description: String::new(),
                description_hash: None,
                user_count: 0,
                permissions: None,
                temporary: false,
                position: 0,
                max_users: 0,
                pchat_protocol: None,
                pchat_max_history: None,
                pchat_retention_days: None,
                    pchat_key_custodians: Vec::new(),            },
        );
        let _ = state.msgs.by_channel.insert(5, vec![]);
    }

    let cr = mumble_tcp::ChannelRemove { channel_id: 5 };
    cr.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert!(!state.channels.contains_key(&5));
    assert!(!state.msgs.by_channel.contains_key(&5));
    drop(state);

    assert!(emitter.event_names().contains(&"state-changed".to_string()));
}

// -- TextMessage (channel) -----------------------------------------

#[test]
fn text_message_channel_message() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.conn.own_session = Some(1);
        let _ = state.users.insert(10, make_user(10, "Alice"));
    }

    let tm = mumble_tcp::TextMessage {
        actor: Some(10),
        channel_id: vec![5],
        message: "Hello!".into(),
        ..Default::default()
    };
    tm.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    let msgs = state.msgs.by_channel.get(&5).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].body, "Hello!");
    assert_eq!(msgs[0].sender_name, "Alice");
    assert_eq!(msgs[0].channel_id, 5);
    assert!(!msgs[0].is_own);
    assert!(msgs[0].dm_session.is_none());
    drop(state);

    let names = emitter.event_names();
    assert!(names.contains(&"new-message".to_string()));
    assert!(names.contains(&"unread-changed".to_string()));
}

/// Regression test: `new-message` and `request_user_attention` must be
/// emitted via the `DeferredEmitter` (i.e. AFTER the `SharedState` lock
/// is released).  Prior to the fix, these were emitted while the lock was
/// held, causing a deadlock when the Tauri IPC handler tried to re-acquire
/// the lock from the webview thread.
#[test]
fn channel_message_emits_attention_for_permanently_listened() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.conn.own_session = Some(1);
        let _ = state.users.insert(10, make_user(10, "Bob"));
        // Select channel 0 so channel 5 counts as "not viewed".
        state.selected_channel = Some(0);
        // Mark channel 5 as permanently listened.
        let _ = state.permanently_listened.insert(5);
    }

    let tm = mumble_tcp::TextMessage {
        actor: Some(10),
        channel_id: vec![5],
        message: "Ping!".into(),
        ..Default::default()
    };
    tm.handle(&ctx);

    let names = emitter.event_names();
    assert!(
        names.contains(&"new-message".to_string()),
        "new-message must be emitted (deferred) for channel messages"
    );
    assert!(
        emitter.attention_count() > 0,
        "request_user_attention must be called (deferred) for permanently-listened non-viewed channels"
    );
}

#[test]
fn text_message_own_message_ignored() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.conn.own_session = Some(10);
    }

    let tm = mumble_tcp::TextMessage {
        actor: Some(10),
        channel_id: vec![0],
        message: "My message".into(),
        ..Default::default()
    };
    tm.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert!(state.msgs.by_channel.is_empty());
    drop(state);
    assert!(emitter.events().is_empty());
}

#[test]
fn text_message_no_channel_defaults_to_zero() {
    let (ctx, _) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.conn.own_session = Some(1);
        let _ = state.users.insert(10, make_user(10, "Server"));
    }

    let tm = mumble_tcp::TextMessage {
        actor: Some(10),
        message: "Broadcast".into(),
        ..Default::default()
    };
    tm.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    let msgs = state.msgs.by_channel.get(&0).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].body, "Broadcast");
}

#[test]
fn text_message_multi_channel() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.conn.own_session = Some(1);
        let _ = state.users.insert(10, make_user(10, "Alice"));
    }

    let tm = mumble_tcp::TextMessage {
        actor: Some(10),
        channel_id: vec![1, 2, 3],
        message: "Multi".into(),
        ..Default::default()
    };
    tm.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert!(state.msgs.by_channel.contains_key(&1));
    assert!(state.msgs.by_channel.contains_key(&2));
    assert!(state.msgs.by_channel.contains_key(&3));
    drop(state);

    let new_msg_count = emitter
        .event_names()
        .iter()
        .filter(|n| *n == "new-message")
        .count();
    assert_eq!(new_msg_count, 3);
}

#[test]
fn text_message_unread_not_incremented_for_viewed_channel() {
    let (ctx, _) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.conn.own_session = Some(1);
        state.selected_channel = Some(5);
        let _ = state.users.insert(10, make_user(10, "Alice"));
    }

    let tm = mumble_tcp::TextMessage {
        actor: Some(10),
        channel_id: vec![5],
        message: "Hello!".into(),
        ..Default::default()
    };
    tm.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert_eq!(state.msgs.channel_unread.get(&5).copied().unwrap_or(0), 0);
}

#[test]
fn text_message_listened_channel_requests_attention() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.conn.own_session = Some(1);
        state.selected_channel = Some(0); // viewing a different channel
        let _ = state.permanently_listened.insert(5);
        let _ = state.users.insert(10, make_user(10, "Alice"));
    }

    let tm = mumble_tcp::TextMessage {
        actor: Some(10),
        channel_id: vec![5],
        message: "Hello!".into(),
        ..Default::default()
    };
    tm.handle(&ctx);

    assert!(emitter.attention_count() > 0);
}

#[test]
fn text_message_server_message_no_actor() {
    let (ctx, _) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.conn.own_session = Some(1);
    }

    let tm = mumble_tcp::TextMessage {
        actor: None,
        channel_id: vec![0],
        message: "Server notice".into(),
        ..Default::default()
    };
    tm.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    let msgs = state.msgs.by_channel.get(&0).unwrap();
    assert_eq!(msgs[0].sender_name, "Server");
}

// -- TextMessage (DM) ----------------------------------------------

#[test]
fn text_message_dm() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.conn.own_session = Some(1);
        let _ = state.users.insert(10, make_user(10, "Bob"));
    }

    let tm = mumble_tcp::TextMessage {
        actor: Some(10),
        session: vec![1], // DM target
        message: "Hey!".into(),
        ..Default::default()
    };
    tm.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    let dms = state.msgs.by_dm.get(&10).unwrap();
    assert_eq!(dms.len(), 1);
    assert_eq!(dms[0].body, "Hey!");
    assert_eq!(dms[0].dm_session, Some(10));
    assert_eq!(dms[0].sender_name, "Bob");
    drop(state);

    let names = emitter.event_names();
    assert!(names.contains(&"new-dm".to_string()));
    assert!(names.contains(&"dm-unread-changed".to_string()));
}

#[test]
fn text_message_dm_no_unread_when_viewing() {
    let (ctx, _) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.conn.own_session = Some(1);
        state.msgs.selected_dm_user = Some(10);
        let _ = state.users.insert(10, make_user(10, "Bob"));
    }

    let tm = mumble_tcp::TextMessage {
        actor: Some(10),
        session: vec![1],
        message: "Hey!".into(),
        ..Default::default()
    };
    tm.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert_eq!(state.msgs.dm_unread.get(&10).copied().unwrap_or(0), 0);
}

#[test]
fn text_message_dm_always_requests_attention() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.conn.own_session = Some(1);
        let _ = state.users.insert(10, make_user(10, "Bob"));
    }

    let tm = mumble_tcp::TextMessage {
        actor: Some(10),
        session: vec![1],
        message: "DM!".into(),
        ..Default::default()
    };
    tm.handle(&ctx);

    assert!(emitter.attention_count() > 0);
}

// -- TextMessage (group) -------------------------------------------
// Group chat support has been removed; the related tests were deleted.

// -- Reject --------------------------------------------------------

#[test]
fn reject_disconnects_and_emits() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.conn.status = ConnectionStatus::Connecting;
    }

    let r = mumble_tcp::Reject {
        reason: Some("Auth failed".into()),
        ..Default::default()
    };
    r.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert_eq!(state.conn.status, ConnectionStatus::Disconnected);
    drop(state);

    let events = emitter.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "connection-rejected");
    assert_eq!(events[0].1["reason"].as_str().unwrap(), "Auth failed");
}

#[test]
fn reject_default_reason() {
    let (ctx, emitter) = make_ctx();
    let r = mumble_tcp::Reject::default();
    r.handle(&ctx);

    let events = emitter.events();
    assert_eq!(events[0].0, "connection-rejected");
    assert_eq!(
        events[0].1["reason"].as_str().unwrap(),
        "Connection rejected by server"
    );
}

// -- ServerConfig --------------------------------------------------

#[test]
fn server_config_updates_state() {
    let (ctx, emitter) = make_ctx();
    let sc = mumble_tcp::ServerConfig {
        message_length: Some(128_000),
        image_message_length: Some(10_000_000),
        allow_html: Some(true),
        max_users: Some(100),
        ..Default::default()
    };
    sc.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert_eq!(state.server.config.max_message_length, 128_000);
    assert_eq!(state.server.config.max_image_message_length, 10_000_000);
    assert!(state.server.config.allow_html);
    assert_eq!(state.server.max_users, Some(100));
    drop(state);

    assert!(emitter.event_names().contains(&"server-config".to_string()));
}

#[test]
fn server_config_zero_image_length_keeps_default() {
    let (ctx, _) = make_ctx();
    let sc = mumble_tcp::ServerConfig {
        image_message_length: Some(0),
        ..Default::default()
    };
    sc.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert_eq!(state.server.config.max_image_message_length, 131_072); // default
}

#[test]
fn server_config_partial_update() {
    let (ctx, _) = make_ctx();
    // Only set message_length, leave others at defaults.
    let sc = mumble_tcp::ServerConfig {
        message_length: Some(99_999),
        ..Default::default()
    };
    sc.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert_eq!(state.server.config.max_message_length, 99_999);
    assert_eq!(state.server.config.max_image_message_length, 131_072); // unchanged
    assert!(state.server.config.allow_html); // unchanged default
}

#[test]
fn server_config_webrtc_sfu_available() {
    let (ctx, _) = make_ctx();

    // Default should be false.
    {
        let state = ctx.shared.lock().unwrap();
        assert!(!state.server.config.webrtc_sfu_available);
    }

    // Server reports SFU available.
    let sc = mumble_tcp::ServerConfig {
        webrtc_sfu_available: Some(true),
        ..Default::default()
    };
    sc.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert!(state.server.config.webrtc_sfu_available);
}

#[test]
fn server_config_fancy_rest_api_url_set_and_cleared() {
    let (ctx, _) = make_ctx();

    // Default: no override.
    assert!(ctx
        .shared
        .lock()
        .unwrap()
        .server
        .config
        .fancy_rest_api_url
        .is_none());

    // Server advertises an override URL (whitespace gets trimmed).
    let sc = mumble_tcp::ServerConfig {
        fancy_rest_api_url: Some("  https://files.example.com  ".to_owned()),
        ..Default::default()
    };
    sc.handle(&ctx);
    assert_eq!(
        ctx.shared.lock().unwrap().server.config.fancy_rest_api_url.as_deref(),
        Some("https://files.example.com")
    );

    // Empty string clears the override (admin removed the config value).
    let sc_clear = mumble_tcp::ServerConfig {
        fancy_rest_api_url: Some(String::new()),
        ..Default::default()
    };
    sc_clear.handle(&ctx);
    assert!(ctx
        .shared
        .lock()
        .unwrap()
        .server
        .config
        .fancy_rest_api_url
        .is_none());
}

// -- PermissionDenied ----------------------------------------------

#[test]
fn permission_denied_reverts_listen() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        let _ = state.permanently_listened.insert(5);
    }

    let pd = mumble_tcp::PermissionDenied {
        channel_id: Some(5),
        r#type: Some(1),
        reason: Some("No permissions".into()),
        ..Default::default()
    };
    pd.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert!(!state.permanently_listened.contains(&5));
    drop(state);

    let names = emitter.event_names();
    assert!(names.contains(&"listen-denied".to_string()));
    assert!(names.contains(&"permission-denied".to_string()));
}

#[test]
fn permission_denied_without_channel_still_emits_general() {
    let (ctx, emitter) = make_ctx();
    let pd = mumble_tcp::PermissionDenied {
        reason: Some("Generic".into()),
        ..Default::default()
    };
    pd.handle(&ctx);

    let names = emitter.event_names();
    assert!(!names.contains(&"listen-denied".to_string()));
    assert!(names.contains(&"permission-denied".to_string()));
}

#[test]
fn permission_denied_payload_contains_type_and_reason() {
    let (ctx, emitter) = make_ctx();
    let pd = mumble_tcp::PermissionDenied {
        r#type: Some(4),
        reason: Some("TextTooLong".into()),
        ..Default::default()
    };
    pd.handle(&ctx);

    let events = emitter.events();
    let denied = events
        .iter()
        .find(|(n, _)| n == "permission-denied")
        .unwrap();
    assert_eq!(denied.1["deny_type"].as_i64().unwrap(), 4);
    assert_eq!(denied.1["reason"].as_str().unwrap(), "TextTooLong");
}

// -- PluginDataTransmission ----------------------------------------

#[test]
fn plugin_data_payload_content() {
    let (ctx, emitter) = make_ctx();
    let pd = mumble_tcp::PluginDataTransmission {
        sender_session: Some(7),
        data_id: Some("test-id".into()),
        data: Some(vec![1, 2, 3]),
        ..Default::default()
    };
    pd.handle(&ctx);

    let events = emitter.events();
    let plugin = events.iter().find(|(n, _)| n == "plugin-data").unwrap();
    assert_eq!(plugin.1["sender_session"].as_u64().unwrap(), 7);
    assert_eq!(plugin.1["data_id"].as_str().unwrap(), "test-id");
    assert_eq!(plugin.1["data"].as_array().unwrap().len(), 3);
}

#[test]
fn plugin_data_no_data_emits_empty_defaults() {
    let (ctx, emitter) = make_ctx();
    let pd = mumble_tcp::PluginDataTransmission::default();
    pd.handle(&ctx);

    let events = emitter.events();
    let plugin = events.iter().find(|(n, _)| n == "plugin-data").unwrap();
    assert_eq!(plugin.1["data_id"].as_str().unwrap(), "");
    assert!(plugin.1["data"].as_array().unwrap().is_empty());
}

// -- PermissionQuery -----------------------------------------------

#[test]
fn permission_query_stores_permissions() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        let _ = state.channels.insert(
            1,
            ChannelEntry {
                id: 1,
                parent_id: Some(0),
                name: "Test".into(),
                description: String::new(),
                description_hash: None,
                user_count: 0,
                permissions: None,
                temporary: false,
                position: 0,
                max_users: 0,
                pchat_protocol: None,
                pchat_max_history: None,
                pchat_retention_days: None,
                    pchat_key_custodians: Vec::new(),            },
        );
    }

    let pq = mumble_tcp::PermissionQuery {
        channel_id: Some(1),
        permissions: Some(0xFF),
        ..Default::default()
    };
    pq.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert_eq!(state.channels[&1].permissions, Some(0xFF));
    drop(state);

    assert!(emitter.event_names().contains(&"state-changed".to_string()));
}

#[test]
fn permission_query_flush_clears_all() {
    let (ctx, _) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        let _ = state.channels.insert(
            1,
            ChannelEntry {
                id: 1,
                parent_id: None,
                name: "A".into(),
                description: String::new(),
                description_hash: None,
                user_count: 0,
                permissions: Some(0x01),
                temporary: false,
                position: 0,
                max_users: 0,
                pchat_protocol: None,
                pchat_max_history: None,
                pchat_retention_days: None,
                    pchat_key_custodians: Vec::new(),            },
        );
        let _ = state.channels.insert(
            2,
            ChannelEntry {
                id: 2,
                parent_id: None,
                name: "B".into(),
                description: String::new(),
                description_hash: None,
                user_count: 0,
                permissions: Some(0x02),
                temporary: false,
                position: 0,
                max_users: 0,
                pchat_protocol: None,
                pchat_max_history: None,
                pchat_retention_days: None,
                    pchat_key_custodians: Vec::new(),            },
        );
    }

    let pq = mumble_tcp::PermissionQuery {
        channel_id: Some(1),
        permissions: Some(0xFF),
        flush: Some(true),
    };
    pq.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert_eq!(state.channels[&1].permissions, Some(0xFF)); // re-set
    assert_eq!(state.channels[&2].permissions, None); // flushed
}

#[test]
fn permission_query_no_channel_no_event() {
    let (ctx, emitter) = make_ctx();
    let pq = mumble_tcp::PermissionQuery::default();
    pq.handle(&ctx);
    // No channel_id or permissions, so no state-changed event.
    assert!(!emitter.event_names().contains(&"state-changed".to_string()));
}

// -- PermissionQuery: push subscribe tracking ----------------------

#[test]
fn permission_query_tracks_subscribe_push_channels() {
    let (ctx, _) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        let _ = state.channels.insert(
            1,
            ChannelEntry {
                id: 1,
                parent_id: Some(0),
                name: "General".into(),
                description: String::new(),
                description_hash: None,
                user_count: 0,
                permissions: None,
                temporary: false,
                position: 0,
                max_users: 0,
                pchat_protocol: None,
                pchat_max_history: None,
                pchat_retention_days: None,
                pchat_key_custodians: Vec::new(),
            },
        );
        let _ = state.channels.insert(
            2,
            ChannelEntry {
                id: 2,
                parent_id: Some(0),
                name: "AFK".into(),
                description: String::new(),
                description_hash: None,
                user_count: 0,
                permissions: None,
                temporary: false,
                position: 0,
                max_users: 0,
                pchat_protocol: None,
                pchat_max_history: None,
                pchat_retention_days: None,
                pchat_key_custodians: Vec::new(),
            },
        );
    }

    // Channel 1 with SubscribePush permission (0x2000).
    let pq1 = mumble_tcp::PermissionQuery {
        channel_id: Some(1),
        permissions: Some(0x2000),
        ..Default::default()
    };
    pq1.handle(&ctx);

    // Channel 2 without SubscribePush permission.
    let pq2 = mumble_tcp::PermissionQuery {
        channel_id: Some(2),
        permissions: Some(0x0001),
        ..Default::default()
    };
    pq2.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert!(
        state.push_subscribed_channels.contains(&1),
        "channel 1 should be push-subscribed (has 0x2000)"
    );
    assert!(
        !state.push_subscribed_channels.contains(&2),
        "channel 2 should NOT be push-subscribed (no 0x2000)"
    );
}

#[test]
fn permission_query_removes_subscribe_push_on_revoke() {
    let (ctx, _) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        let _ = state.channels.insert(
            1,
            ChannelEntry {
                id: 1,
                parent_id: Some(0),
                name: "General".into(),
                description: String::new(),
                description_hash: None,
                user_count: 0,
                permissions: None,
                temporary: false,
                position: 0,
                max_users: 0,
                pchat_protocol: None,
                pchat_max_history: None,
                pchat_retention_days: None,
                pchat_key_custodians: Vec::new(),
            },
        );
    }

    // Grant SubscribePush.
    let grant = mumble_tcp::PermissionQuery {
        channel_id: Some(1),
        permissions: Some(0x2200),
        ..Default::default()
    };
    grant.handle(&ctx);
    assert!(ctx.shared.lock().unwrap().push_subscribed_channels.contains(&1));

    // Revoke SubscribePush (remove 0x2000 bit).
    let revoke = mumble_tcp::PermissionQuery {
        channel_id: Some(1),
        permissions: Some(0x0200),
        ..Default::default()
    };
    revoke.handle(&ctx);
    assert!(
        !ctx.shared.lock().unwrap().push_subscribed_channels.contains(&1),
        "channel should be removed from push_subscribed after permission revoked"
    );
}

#[test]
fn permission_query_flush_clears_push_subscribed() {
    let (ctx, _) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        let _ = state.push_subscribed_channels.insert(1);
        let _ = state.push_subscribed_channels.insert(2);
        let _ = state.channels.insert(
            1,
            ChannelEntry {
                id: 1,
                parent_id: None,
                name: "A".into(),
                description: String::new(),
                description_hash: None,
                user_count: 0,
                permissions: Some(0x2000),
                temporary: false,
                position: 0,
                max_users: 0,
                pchat_protocol: None,
                pchat_max_history: None,
                pchat_retention_days: None,
                pchat_key_custodians: Vec::new(),
            },
        );
    }

    let flush_pq = mumble_tcp::PermissionQuery {
        channel_id: Some(1),
        permissions: Some(0x0001),
        flush: Some(true),
    };
    flush_pq.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert!(
        state.push_subscribed_channels.is_empty() || !state.push_subscribed_channels.contains(&2),
        "flush should clear all push_subscribed_channels; channel 2 was not re-added"
    );
    assert!(
        !state.push_subscribed_channels.contains(&1),
        "channel 1 should not be push-subscribed after flush (perm 0x0001 has no 0x2000)"
    );
}

// -- CodecVersion --------------------------------------------------

#[test]
fn codec_version_sets_opus() {
    let (ctx, emitter) = make_ctx();
    let cv = mumble_tcp::CodecVersion {
        opus: Some(true),
        ..Default::default()
    };
    cv.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert!(state.server.opus);
    drop(state);

    // No events emitted by codec version handler.
    assert!(emitter.events().is_empty());
}

#[test]
fn codec_version_opus_defaults_false() {
    let (ctx, _) = make_ctx();
    let cv = mumble_tcp::CodecVersion::default();
    cv.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert!(!state.server.opus);
}

// -- Dispatch ------------------------------------------------------

#[test]
fn dispatch_routes_ping() {
    let (ctx, emitter) = make_ctx();
    let msg = ControlMessage::Ping(mumble_tcp::Ping {
        timestamp: Some(0),
        ..Default::default()
    });
    dispatch(&msg, &ctx);
    assert!(!emitter.events().is_empty());
}

#[test]
fn dispatch_unknown_variant_is_noop() {
    let (ctx, emitter) = make_ctx();
    let msg = ControlMessage::Authenticate(mumble_tcp::Authenticate::default());
    dispatch(&msg, &ctx);
    assert!(emitter.events().is_empty());
}

#[tokio::test]
async fn dispatch_routes_server_sync() {
    let (ctx, emitter) = make_ctx();
    let msg = ControlMessage::ServerSync(mumble_tcp::ServerSync {
        session: Some(1),
        ..Default::default()
    });
    dispatch(&msg, &ctx);
    assert!(emitter.event_names().contains(&"server-connected".to_string()));
}

// -- TextMessage + pchat interaction ----------------------------------

#[test]
fn text_message_skipped_for_pchat_enabled_channel() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.conn.own_session = Some(1);
        let _ = state.users.insert(10, make_e2ee_user(10, "Alice"));
        // Channel 5 has pchat enabled (PostJoin mode).
        let _ = state.channels.insert(
            5,
            ChannelEntry {
                id: 5,
                parent_id: Some(0),
                name: "pchat-room".into(),
                description: String::new(),
                description_hash: None,
                user_count: 0,
                permissions: None,
                temporary: false,
                position: 0,
                max_users: 0,
                pchat_protocol: Some(PchatProtocol::FancyV1FullArchive),
                pchat_max_history: None,
                pchat_retention_days: None,
                    pchat_key_custodians: Vec::new(),            },
        );
    }

    let tm = mumble_tcp::TextMessage {
        actor: Some(10),
        channel_id: vec![5],
        message: "Hello pchat!".into(),
        ..Default::default()
    };
    tm.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    // TextMessage should NOT be stored for pchat-enabled channels.
    assert!(
        state.msgs.by_channel.get(&5).map(Vec::is_empty).unwrap_or(true),
        "TextMessage should be skipped for pchat-enabled channel"
    );
    drop(state);

    // No new-message event should be emitted for the skipped channel.
    let names = emitter.event_names();
    assert!(
        !names.contains(&"new-message".to_string()),
        "no new-message event for pchat channel"
    );
}

#[test]
fn text_message_stored_for_non_pchat_channel() {
    let (ctx, _) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.conn.own_session = Some(1);
        let _ = state.users.insert(10, make_user(10, "Bob"));
        // Channel 3 with pchat_protocol = None (explicitly disabled).
        let _ = state.channels.insert(
            3,
            ChannelEntry {
                id: 3,
                parent_id: Some(0),
                name: "regular-room".into(),
                description: String::new(),
                description_hash: None,
                user_count: 0,
                permissions: None,
                temporary: false,
                position: 0,
                max_users: 0,
                pchat_protocol: Some(PchatProtocol::None),
                pchat_max_history: None,
                pchat_retention_days: None,
                    pchat_key_custodians: Vec::new(),            },
        );
    }

    let tm = mumble_tcp::TextMessage {
        actor: Some(10),
        channel_id: vec![3],
        message: "Regular message".into(),
        ..Default::default()
    };
    tm.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    let msgs = state.msgs.by_channel.get(&3).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].body, "Regular message");
}

#[test]
fn text_message_stored_when_pchat_protocol_absent() {
    let (ctx, _) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.conn.own_session = Some(1);
        let _ = state.users.insert(10, make_user(10, "Carol"));
        // Channel 7 without pchat_protocol (legacy channel, no pchat support).
        let _ = state.channels.insert(
            7,
            ChannelEntry {
                id: 7,
                parent_id: Some(0),
                name: "legacy-room".into(),
                description: String::new(),
                description_hash: None,
                user_count: 0,
                permissions: None,
                temporary: false,
                position: 0,
                max_users: 0,
                pchat_protocol: None,
                pchat_max_history: None,
                pchat_retention_days: None,
                    pchat_key_custodians: Vec::new(),            },
        );
    }

    let tm = mumble_tcp::TextMessage {
        actor: Some(10),
        channel_id: vec![7],
        message: "Legacy message".into(),
        ..Default::default()
    };
    tm.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    let msgs = state.msgs.by_channel.get(&7).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].body, "Legacy message");
}

#[test]
fn text_message_skipped_for_full_archive_channel() {
    let (ctx, _) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.conn.own_session = Some(1);
        let _ = state.users.insert(10, make_e2ee_user(10, "Dave"));
        // Channel 9 with FullArchive mode.
        let _ = state.channels.insert(
            9,
            ChannelEntry {
                id: 9,
                parent_id: Some(0),
                name: "archive-room".into(),
                description: String::new(),
                description_hash: None,
                user_count: 0,
                permissions: None,
                temporary: false,
                position: 0,
                max_users: 0,
                pchat_protocol: Some(PchatProtocol::FancyV1FullArchive),
                pchat_max_history: None,
                pchat_retention_days: None,
                    pchat_key_custodians: Vec::new(),            },
        );
    }

    let tm = mumble_tcp::TextMessage {
        actor: Some(10),
        channel_id: vec![9],
        message: "Archived message".into(),
        ..Default::default()
    };
    tm.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert!(
        state.msgs.by_channel.get(&9).map(Vec::is_empty).unwrap_or(true),
        "TextMessage should be skipped for FullArchive channel"
    );
}

#[test]
fn text_message_mixed_pchat_and_regular_channels() {
    let (ctx, _) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.conn.own_session = Some(1);
        let _ = state.users.insert(10, make_e2ee_user(10, "Eve"));
        // Channel 2: pchat enabled
        let _ = state.channels.insert(
            2,
            ChannelEntry {
                id: 2,
                parent_id: Some(0),
                name: "pchat".into(),
                description: String::new(),
                description_hash: None,
                user_count: 0,
                permissions: None,
                temporary: false,
                position: 0,
                max_users: 0,
                pchat_protocol: Some(PchatProtocol::FancyV1FullArchive),
                pchat_max_history: None,
                pchat_retention_days: None,
                    pchat_key_custodians: Vec::new(),            },
        );
        // Channel 4: no pchat
        let _ = state.channels.insert(
            4,
            ChannelEntry {
                id: 4,
                parent_id: Some(0),
                name: "regular".into(),
                description: String::new(),
                description_hash: None,
                user_count: 0,
                permissions: None,
                temporary: false,
                position: 0,
                max_users: 0,
                pchat_protocol: None,
                pchat_max_history: None,
                pchat_retention_days: None,
                    pchat_key_custodians: Vec::new(),            },
        );
    }

    // Message targets both channels.
    let tm = mumble_tcp::TextMessage {
        actor: Some(10),
        channel_id: vec![2, 4],
        message: "Multi-channel".into(),
        ..Default::default()
    };
    tm.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    // Channel 2 (pchat) should have no message.
    assert!(
        state.msgs.by_channel.get(&2).map(Vec::is_empty).unwrap_or(true),
        "pchat channel should not store TextMessage"
    );
    // Channel 4 (regular) should have the message.
    let msgs = state.msgs.by_channel.get(&4).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].body, "Multi-channel");
}

// -- PchatKeyHoldersList -------------------------------------------

#[test]
fn key_holders_online_user_gets_live_name() {
    let (ctx, _emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        let mut user = make_user(1, "Alice");
        user.hash = Some("aabb".into());
        let _ = state.users.insert(1, user);
    }

    let msg = mumble_tcp::PchatKeyHoldersList {
        channel_id: Some(42),
        holders: vec![mumble_tcp::pchat_key_holders_list::Entry {
            cert_hash: Some("aabb".into()),
            name: Some("OldAlice".into()),
        }],
    };
    msg.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    let holders = state.pchat_ctx.key_holders.get(&42).unwrap();
    assert_eq!(holders.len(), 1);
    assert_eq!(holders[0].name, "Alice", "should use the live online name");
    assert!(holders[0].is_online);
}

#[test]
fn key_holders_server_name_used_when_offline_and_not_hash() {
    let (ctx, _emitter) = make_ctx();

    let msg = mumble_tcp::PchatKeyHoldersList {
        channel_id: Some(42),
        holders: vec![mumble_tcp::pchat_key_holders_list::Entry {
            cert_hash: Some("ccdd".into()),
            name: Some("Bob".into()),
        }],
    };
    msg.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    let holders = state.pchat_ctx.key_holders.get(&42).unwrap();
    assert_eq!(holders[0].name, "Bob", "server-provided name should be used");
    assert!(!holders[0].is_online);
}

#[test]
fn key_holders_hash_as_name_falls_through_to_resolver() {
    let (ctx, _emitter) = make_ctx();
    let hash = "76688b569fb4519ef37da57900682ee3a55b02d2";

    // Set up resolver so it returns a fallback name.
    {
        let mut state = ctx.shared.lock().unwrap();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        state.pchat_ctx.hash_name_resolver = Some(Arc::new(
            crate::state::hash_names::DefaultHashNameResolver::new(
                tmp.path().to_path_buf(),
            ),
        ));
    }

    // Server sends the hash as the name (the bug scenario).
    let msg = mumble_tcp::PchatKeyHoldersList {
        channel_id: Some(42),
        holders: vec![mumble_tcp::pchat_key_holders_list::Entry {
            cert_hash: Some(hash.into()),
            name: Some(hash.into()),
        }],
    };
    msg.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    let holders = state.pchat_ctx.key_holders.get(&42).unwrap();
    assert_ne!(
        holders[0].name, hash,
        "hash should not be used as display name"
    );
    assert!(
        holders[0].name.contains(' '),
        "fallback name should be 'Adjective Animal', got: {}",
        holders[0].name
    );
}

#[test]
fn key_holders_resolver_returns_recorded_name() {
    let (ctx, _emitter) = make_ctx();
    let hash = "deadbeef01234567";

    {
        let mut state = ctx.shared.lock().unwrap();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let resolver = crate::state::hash_names::DefaultHashNameResolver::new(
            tmp.path().to_path_buf(),
        );
        resolver.record(hash, "Charlie");
        state.pchat_ctx.hash_name_resolver = Some(Arc::new(resolver));
    }

    let msg = mumble_tcp::PchatKeyHoldersList {
        channel_id: Some(42),
        holders: vec![mumble_tcp::pchat_key_holders_list::Entry {
            cert_hash: Some(hash.into()),
            name: None,
        }],
    };
    msg.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    let holders = state.pchat_ctx.key_holders.get(&42).unwrap();
    assert_eq!(
        holders[0].name, "Charlie",
        "resolver should return previously recorded name"
    );
}

#[test]
fn key_holders_no_resolver_falls_back_to_hash() {
    let (ctx, _emitter) = make_ctx();
    let hash = "aabbccddee";

    // No resolver set (hash_name_resolver is None).
    let msg = mumble_tcp::PchatKeyHoldersList {
        channel_id: Some(42),
        holders: vec![mumble_tcp::pchat_key_holders_list::Entry {
            cert_hash: Some(hash.into()),
            name: None,
        }],
    };
    msg.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    let holders = state.pchat_ctx.key_holders.get(&42).unwrap();
    assert_eq!(
        holders[0].name, hash,
        "without a resolver the raw hash should be used"
    );
}

#[test]
fn key_holders_empty_server_name_uses_resolver() {
    let (ctx, _emitter) = make_ctx();
    let hash = "1122334455";

    {
        let mut state = ctx.shared.lock().unwrap();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        state.pchat_ctx.hash_name_resolver = Some(Arc::new(
            crate::state::hash_names::DefaultHashNameResolver::new(
                tmp.path().to_path_buf(),
            ),
        ));
    }

    let msg = mumble_tcp::PchatKeyHoldersList {
        channel_id: Some(42),
        holders: vec![mumble_tcp::pchat_key_holders_list::Entry {
            cert_hash: Some(hash.into()),
            name: Some(String::new()),
        }],
    };
    msg.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    let holders = state.pchat_ctx.key_holders.get(&42).unwrap();
    assert_ne!(
        holders[0].name, hash,
        "empty server name should fall through to resolver"
    );
    assert!(
        holders[0].name.contains(' '),
        "fallback name should be 'Adjective Animal', got: {}",
        holders[0].name
    );
}

// -- Lint: no emit under lock (meta-test) --------------------------

// Scan all Rust source files in the `mumble-tauri` crate to ensure
// that `app.emit(` (Tauri IPC) is never called while a `SharedState`
// or `inner` mutex lock guard is alive.

// -- Server activity log -------------------------------------------

/// Helper to extract "server-log" event message strings from the emitter.
fn server_log_messages(emitter: &MockEmitter) -> Vec<String> {
    emitter
        .events()
        .iter()
        .filter(|(name, _)| name == "server-log")
        .filter_map(|(_, val)| val.get("message").and_then(|m| m.as_str()).map(String::from))
        .collect()
}

fn make_synced_ctx() -> (HandlerContext, Arc<MockEmitter>) {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.conn.synced = true;
        state.conn.own_session = Some(1);
    }
    (ctx, emitter)
}

#[test]
fn server_log_user_connected() {
    let (ctx, emitter) = make_synced_ctx();
    let us = mumble_tcp::UserState {
        session: Some(10),
        name: Some("Alice".into()),
        channel_id: Some(0),
        ..Default::default()
    };
    us.handle(&ctx);

    let logs = server_log_messages(&emitter);
    assert_eq!(logs, vec!["Alice connected"]);
}

#[test]
fn server_log_user_disconnected() {
    let (ctx, emitter) = make_synced_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        let _ = state.users.insert(10, make_user(10, "Bob"));
    }

    let ur = mumble_tcp::UserRemove {
        session: 10,
        ..Default::default()
    };
    ur.handle(&ctx);

    let logs = server_log_messages(&emitter);
    assert_eq!(logs, vec!["Bob disconnected"]);
}

#[test]
fn server_log_self_mute() {
    let (ctx, emitter) = make_synced_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        let _ = state.users.insert(10, make_user(10, "Charlie"));
    }

    let us = mumble_tcp::UserState {
        session: Some(10),
        self_mute: Some(true),
        ..Default::default()
    };
    us.handle(&ctx);

    let logs = server_log_messages(&emitter);
    assert_eq!(logs, vec!["Charlie self-muted"]);
}

#[test]
fn server_log_channel_move() {
    let (ctx, emitter) = make_synced_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        let _ = state.users.insert(10, make_user(10, "Dana"));
        let _ = state.channels.insert(
            5,
            ChannelEntry {
                id: 5,
                parent_id: Some(0),
                name: "Lobby".into(),
                description: String::new(),
                description_hash: None,
                user_count: 0,
                permissions: None,
                temporary: false,
                position: 0,
                max_users: 0,
                pchat_protocol: None,
                pchat_max_history: None,
                pchat_retention_days: None,
                pchat_key_custodians: Vec::new(),
            },
        );
    }

    let us = mumble_tcp::UserState {
        session: Some(10),
        channel_id: Some(5),
        ..Default::default()
    };
    us.handle(&ctx);

    let logs = server_log_messages(&emitter);
    assert_eq!(logs, vec!["Dana moved to Lobby"]);
}

#[test]
fn server_log_no_events_before_sync() {
    let (ctx, emitter) = make_ctx();
    // synced is false by default.
    let us = mumble_tcp::UserState {
        session: Some(10),
        name: Some("Eve".into()),
        channel_id: Some(0),
        ..Default::default()
    };
    us.handle(&ctx);

    let logs = server_log_messages(&emitter);
    assert!(logs.is_empty(), "no log events should emit before sync completes");
}

// -- Lint: no emit under lock (meta-test) --------------------------
///
/// Background: calling `app.emit()` while holding a `std::sync::Mutex`
/// causes a cross-thread deadlock - the webview dispatches the JS event
/// synchronously, and if the JS handler invokes a Tauri command that
/// re-locks the same mutex, both threads block forever.
///
/// The heuristic is intentionally conservative: it tracks brace-depth
/// from the line where `.lock()` is called and flags any `.emit(`
/// before the brace scope closes.  Known-safe patterns (e.g. emitting
/// inside `flush()` which runs after lock release) can be annotated
/// with `// lint:allow-emit-under-lock` on the same line.
#[test]
fn no_emit_under_lock_in_sources() {
    let crate_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut violations = Vec::new();

    for entry in walkdir(&crate_src) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let contents = std::fs::read_to_string(path).unwrap();
        check_emit_under_lock(path, &contents, &mut violations);
    }

    assert!(
        violations.is_empty(),
        "emit-under-lock violations found (calling app.emit() while holding a mutex \
         causes a deadlock with Tauri IPC):\n{}",
        violations.join("\n")
    );
}

/// Recursively collect all file entries under `dir`.
fn walkdir(dir: &std::path::Path) -> Vec<DirEntry> {
    let mut entries = Vec::new();
    walk_recursive(dir, &mut entries);
    entries
}

struct DirEntry {
    path: std::path::PathBuf,
}

impl DirEntry {
    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

fn walk_recursive(dir: &std::path::Path, out: &mut Vec<DirEntry>) {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_recursive(&path, out);
        } else {
            out.push(DirEntry { path });
        }
    }
}

fn check_emit_under_lock(
    path: &std::path::Path,
    contents: &str,
    violations: &mut Vec<String>,
) {
    let lock_patterns = [".lock()", "shared.lock()", "inner.lock()"];

    // Track nested brace depth for each active lock scope.
    // Each entry: (line_number_of_lock, brace_depth_at_lock)
    let mut active_locks: Vec<(usize, i32)> = Vec::new();
    let mut brace_depth: i32 = 0;

    for (line_idx, line) in contents.lines().enumerate() {
        let line_num = line_idx + 1;
        let trimmed = line.trim();

        // Skip comments and test code.
        if trimmed.starts_with("//") || trimmed.starts_with("* ") || trimmed.starts_with("/*") {
            continue;
        }

        // Count braces (rough: doesn't handle strings/comments perfectly,
        // but good enough for this heuristic).
        for ch in line.chars() {
            match ch {
                '{' => brace_depth += 1,
                '}' => {
                    brace_depth -= 1;
                    // Close any lock scopes that have ended.
                    active_locks.retain(|&(_, depth)| brace_depth >= depth);
                }
                _ => {}
            }
        }

        // Detect lock acquisitions where the guard survives beyond the statement.
        // Patterns like `.lock().map(...)` or `.lock().ok()` (single-line or
        // multi-line chain) consume the guard immediately and are safe.
        //
        // The dangerous pattern stores the guard in a variable:
        //   `let mut state = shared.lock()...;`
        //   `let Ok(mut state) = shared.lock()...`
        //   `if let Ok(mut state) = shared.lock() { ... }`
        //
        // We detect these by checking for `let` before `.lock()` on the same
        // line, while filtering out consuming chains that also have .map/.ok.
        if lock_patterns.iter().any(|p| line.contains(p)) {
            let has_let_binding = trimmed.starts_with("let ") || trimmed.contains("if let ");
            let guard_consumed_immediately = line.contains(".lock().map(")
                || line.contains(".lock().ok()")
                || line.contains(".lock().unwrap().");
            if has_let_binding && !guard_consumed_immediately {
                let rel = path
                    .strip_prefix(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
                    .unwrap_or(path);
                if rel.to_string_lossy().contains("tests") {
                    continue;
                }
                active_locks.push((line_num, brace_depth));
            }
        }

        // Detect emit calls while locks are active.
        if !active_locks.is_empty()
            && (line.contains(".emit(") || line.contains("ctx.emit("))
            && !line.contains("lint:allow-emit-under-lock")
        {
            let rel = path
                .strip_prefix(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
                .unwrap_or(path);
            // Exclude test files.
            if rel.to_string_lossy().contains("tests") {
                continue;
            }
            let lock_lines: Vec<usize> = active_locks.iter().map(|(l, _)| *l).collect();
            violations.push(format!(
                "  {}:{} - .emit() called with lock held (acquired at line(s) {:?})",
                rel.display(),
                line_num,
                lock_lines,
            ));
        }
    }
}
