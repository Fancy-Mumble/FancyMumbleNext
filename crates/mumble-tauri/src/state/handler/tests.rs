use std::sync::{Arc, Mutex};

use serde_json::Value;

use mumble_protocol::message::ControlMessage;
use mumble_protocol::proto::mumble_tcp;
use mumble_protocol::state::PchatMode;

use super::{dispatch, EventEmitter, HandleMessage, HandlerContext};
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
        texture: None,
        comment: None,
        mute: false,
        deaf: false,
        suppress: false,
        self_mute: false,
        self_deaf: false,
        priority_speaker: false,
        hash: None,
    }
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
        state.server_fancy_version,
        Some(mumble_protocol::state::fancy_version_encode(0, 1, 0))
    );
    assert_eq!(
        state.server_version_info.release.as_deref(),
        Some("Mumble 1.5")
    );
    assert_eq!(state.server_version_info.os.as_deref(), Some("Linux"));
    assert_eq!(
        state.server_version_info.os_version.as_deref(),
        Some("5.15")
    );
    assert_eq!(state.server_version_info.version_v1, Some(0x0001_0500));
    assert_eq!(state.server_version_info.version_v2, Some(42));
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
    assert_eq!(state.status, ConnectionStatus::Connected);
    assert_eq!(state.own_session, Some(42));
    assert!(state.synced);
    assert_eq!(state.max_bandwidth, Some(72000));
    assert_eq!(state.welcome_text.as_deref(), Some("Welcome!"));
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
        state.users.insert(42, user);
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
        state.users.insert(10, make_user(10, "Alice"));
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
        state.users.insert(10, user);
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
        state.users.insert(10, user);
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
        state.own_session = Some(10);
        state.synced = true;
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

// -- UserRemove ----------------------------------------------------

#[test]
fn user_remove_other_user() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.own_session = Some(1);
        state.users.insert(10, make_user(10, "Alice"));
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
        state.own_session = Some(42);
        state.status = ConnectionStatus::Connected;
        state.synced = true;
        state.users.insert(42, make_user(42, "Me"));
    }

    let ur = mumble_tcp::UserRemove {
        session: 42,
        reason: Some("Banned".into()),
        ..Default::default()
    };
    ur.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert_eq!(state.status, ConnectionStatus::Disconnected);
    assert!(state.users.is_empty());
    assert!(!state.synced);
    assert_eq!(state.own_session, None);
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
        state.own_session = Some(42);
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
        state.channels.insert(
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
                pchat_mode: None,
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
        state.synced = true;
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
        state.channels.insert(
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
                pchat_mode: None,
                pchat_max_history: None,
                pchat_retention_days: None,
                    pchat_key_custodians: Vec::new(),            },
        );
        state.messages.insert(5, vec![]);
    }

    let cr = mumble_tcp::ChannelRemove { channel_id: 5 };
    cr.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert!(!state.channels.contains_key(&5));
    assert!(!state.messages.contains_key(&5));
    drop(state);

    assert!(emitter.event_names().contains(&"state-changed".to_string()));
}

// -- TextMessage (channel) -----------------------------------------

#[test]
fn text_message_channel_message() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.own_session = Some(1);
        state.users.insert(10, make_user(10, "Alice"));
    }

    let tm = mumble_tcp::TextMessage {
        actor: Some(10),
        channel_id: vec![5],
        message: "Hello!".into(),
        ..Default::default()
    };
    tm.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    let msgs = state.messages.get(&5).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].body, "Hello!");
    assert_eq!(msgs[0].sender_name, "Alice");
    assert_eq!(msgs[0].channel_id, 5);
    assert!(!msgs[0].is_own);
    assert!(msgs[0].dm_session.is_none());
    assert!(msgs[0].group_id.is_none());
    drop(state);

    let names = emitter.event_names();
    assert!(names.contains(&"new-message".to_string()));
    assert!(names.contains(&"unread-changed".to_string()));
}

#[test]
fn text_message_own_message_ignored() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.own_session = Some(10);
    }

    let tm = mumble_tcp::TextMessage {
        actor: Some(10),
        channel_id: vec![0],
        message: "My message".into(),
        ..Default::default()
    };
    tm.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert!(state.messages.is_empty());
    drop(state);
    assert!(emitter.events().is_empty());
}

#[test]
fn text_message_no_channel_defaults_to_zero() {
    let (ctx, _) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.own_session = Some(1);
        state.users.insert(10, make_user(10, "Server"));
    }

    let tm = mumble_tcp::TextMessage {
        actor: Some(10),
        message: "Broadcast".into(),
        ..Default::default()
    };
    tm.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    let msgs = state.messages.get(&0).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].body, "Broadcast");
}

#[test]
fn text_message_multi_channel() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.own_session = Some(1);
        state.users.insert(10, make_user(10, "Alice"));
    }

    let tm = mumble_tcp::TextMessage {
        actor: Some(10),
        channel_id: vec![1, 2, 3],
        message: "Multi".into(),
        ..Default::default()
    };
    tm.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert!(state.messages.contains_key(&1));
    assert!(state.messages.contains_key(&2));
    assert!(state.messages.contains_key(&3));
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
        state.own_session = Some(1);
        state.selected_channel = Some(5);
        state.users.insert(10, make_user(10, "Alice"));
    }

    let tm = mumble_tcp::TextMessage {
        actor: Some(10),
        channel_id: vec![5],
        message: "Hello!".into(),
        ..Default::default()
    };
    tm.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert_eq!(state.unread_counts.get(&5).copied().unwrap_or(0), 0);
}

#[test]
fn text_message_listened_channel_requests_attention() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.own_session = Some(1);
        state.selected_channel = Some(0); // viewing a different channel
        state.permanently_listened.insert(5);
        state.users.insert(10, make_user(10, "Alice"));
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
        state.own_session = Some(1);
    }

    let tm = mumble_tcp::TextMessage {
        actor: None,
        channel_id: vec![0],
        message: "Server notice".into(),
        ..Default::default()
    };
    tm.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    let msgs = state.messages.get(&0).unwrap();
    assert_eq!(msgs[0].sender_name, "Server");
}

// -- TextMessage (DM) ----------------------------------------------

#[test]
fn text_message_dm() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.own_session = Some(1);
        state.users.insert(10, make_user(10, "Bob"));
    }

    let tm = mumble_tcp::TextMessage {
        actor: Some(10),
        session: vec![1], // DM target
        message: "Hey!".into(),
        ..Default::default()
    };
    tm.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    let dms = state.dm_messages.get(&10).unwrap();
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
        state.own_session = Some(1);
        state.selected_dm_user = Some(10);
        state.users.insert(10, make_user(10, "Bob"));
    }

    let tm = mumble_tcp::TextMessage {
        actor: Some(10),
        session: vec![1],
        message: "Hey!".into(),
        ..Default::default()
    };
    tm.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert_eq!(state.dm_unread_counts.get(&10).copied().unwrap_or(0), 0);
}

#[test]
fn text_message_dm_always_requests_attention() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.own_session = Some(1);
        state.users.insert(10, make_user(10, "Bob"));
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

#[test]
fn text_message_group() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.own_session = Some(1);
        state.users.insert(10, make_user(10, "Charlie"));
        state.group_chats.insert(
            "g1".into(),
            GroupChat {
                id: "g1".into(),
                name: "Test Group".into(),
                members: vec![1, 10],
                creator: 10,
            },
        );
    }

    let tm = mumble_tcp::TextMessage {
        actor: Some(10),
        session: vec![1], // targets sessions
        message: "<!-- FANCY_GROUP:g1 -->Group hello!".into(),
        ..Default::default()
    };
    tm.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    let msgs = state.group_messages.get("g1").unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].body, "Group hello!");
    assert_eq!(msgs[0].group_id, Some("g1".to_string()));
    assert_eq!(msgs[0].sender_name, "Charlie");
    drop(state);

    let names = emitter.event_names();
    assert!(names.contains(&"new-group-message".to_string()));
    assert!(names.contains(&"group-unread-changed".to_string()));
}

#[test]
fn text_message_group_no_unread_when_viewing() {
    let (ctx, _) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.own_session = Some(1);
        state.selected_group = Some("g1".into());
        state.users.insert(10, make_user(10, "Charlie"));
        state.group_chats.insert(
            "g1".into(),
            GroupChat {
                id: "g1".into(),
                name: "Test Group".into(),
                members: vec![1, 10],
                creator: 10,
            },
        );
    }

    let tm = mumble_tcp::TextMessage {
        actor: Some(10),
        session: vec![1],
        message: "<!-- FANCY_GROUP:g1 -->Hello!".into(),
        ..Default::default()
    };
    tm.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert_eq!(
        state
            .group_unread_counts
            .get("g1")
            .copied()
            .unwrap_or(0),
        0
    );
}

#[test]
fn text_message_group_unknown_group_ignored() {
    let (ctx, _) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.own_session = Some(1);
        state.users.insert(10, make_user(10, "Charlie"));
        // no group_chats entry for "unknown"
    }

    let tm = mumble_tcp::TextMessage {
        actor: Some(10),
        session: vec![1],
        message: "<!-- FANCY_GROUP:unknown -->Body".into(),
        ..Default::default()
    };
    tm.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert!(state.group_messages.is_empty());
}

#[test]
fn text_message_group_requests_attention() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.own_session = Some(1);
        state.users.insert(10, make_user(10, "Charlie"));
        state.group_chats.insert(
            "g1".into(),
            GroupChat {
                id: "g1".into(),
                name: "Test Group".into(),
                members: vec![1, 10],
                creator: 10,
            },
        );
    }

    let tm = mumble_tcp::TextMessage {
        actor: Some(10),
        session: vec![1],
        message: "<!-- FANCY_GROUP:g1 -->Hey".into(),
        ..Default::default()
    };
    tm.handle(&ctx);

    assert!(emitter.attention_count() > 0);
}

// -- Reject --------------------------------------------------------

#[test]
fn reject_disconnects_and_emits() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.status = ConnectionStatus::Connecting;
    }

    let r = mumble_tcp::Reject {
        reason: Some("Auth failed".into()),
        ..Default::default()
    };
    r.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert_eq!(state.status, ConnectionStatus::Disconnected);
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
    assert_eq!(state.server_config.max_message_length, 128_000);
    assert_eq!(state.server_config.max_image_message_length, 10_000_000);
    assert!(state.server_config.allow_html);
    assert_eq!(state.max_users, Some(100));
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
    assert_eq!(state.server_config.max_image_message_length, 131_072); // default
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
    assert_eq!(state.server_config.max_message_length, 99_999);
    assert_eq!(state.server_config.max_image_message_length, 131_072); // unchanged
    assert!(state.server_config.allow_html); // unchanged default
}

// -- PermissionDenied ----------------------------------------------

#[test]
fn permission_denied_reverts_listen() {
    let (ctx, emitter) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.permanently_listened.insert(5);
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
fn plugin_data_creates_group_chat() {
    let (ctx, emitter) = make_ctx();
    let group = serde_json::json!({
        "action": "create",
        "group": {
            "id": "g42",
            "name": "Gamers",
            "members": [1, 2],
            "creator": 1
        }
    });
    let pd = mumble_tcp::PluginDataTransmission {
        sender_session: Some(1),
        data_id: Some("fancy-group".into()),
        data: Some(serde_json::to_vec(&group).unwrap()),
        ..Default::default()
    };
    pd.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert!(state.group_chats.contains_key("g42"));
    assert_eq!(state.group_chats["g42"].name, "Gamers");
    assert_eq!(state.group_chats["g42"].members, vec![1, 2]);
    assert_eq!(state.group_chats["g42"].creator, 1);
    drop(state);

    let names = emitter.event_names();
    assert!(names.contains(&"group-created".to_string()));
    assert!(names.contains(&"plugin-data".to_string()));
}

#[test]
fn plugin_data_non_group_just_emits() {
    let (ctx, emitter) = make_ctx();
    let pd = mumble_tcp::PluginDataTransmission {
        sender_session: Some(1),
        data_id: Some("poll".into()),
        data: Some(b"poll data".to_vec()),
        ..Default::default()
    };
    pd.handle(&ctx);

    let state = ctx.shared.lock().unwrap();
    assert!(state.group_chats.is_empty());
    drop(state);

    let names = emitter.event_names();
    assert_eq!(names, vec!["plugin-data"]);
}

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
        state.channels.insert(
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
                pchat_mode: None,
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
        state.channels.insert(
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
                pchat_mode: None,
                pchat_max_history: None,
                pchat_retention_days: None,
                    pchat_key_custodians: Vec::new(),            },
        );
        state.channels.insert(
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
                pchat_mode: None,
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
    assert!(state.opus);
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
    assert!(!state.opus);
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
        state.own_session = Some(1);
        state.users.insert(10, make_user(10, "Alice"));
        // Channel 5 has pchat enabled (PostJoin mode).
        state.channels.insert(
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
                pchat_mode: Some(PchatMode::PostJoin),
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
        state.messages.get(&5).map(|m| m.is_empty()).unwrap_or(true),
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
        state.own_session = Some(1);
        state.users.insert(10, make_user(10, "Bob"));
        // Channel 3 with pchat_mode = None (explicitly disabled).
        state.channels.insert(
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
                pchat_mode: Some(PchatMode::None),
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
    let msgs = state.messages.get(&3).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].body, "Regular message");
}

#[test]
fn text_message_stored_when_pchat_mode_absent() {
    let (ctx, _) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.own_session = Some(1);
        state.users.insert(10, make_user(10, "Carol"));
        // Channel 7 without pchat_mode (legacy channel, no pchat support).
        state.channels.insert(
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
                pchat_mode: None,
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
    let msgs = state.messages.get(&7).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].body, "Legacy message");
}

#[test]
fn text_message_skipped_for_full_archive_channel() {
    let (ctx, _) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.own_session = Some(1);
        state.users.insert(10, make_user(10, "Dave"));
        // Channel 9 with FullArchive mode.
        state.channels.insert(
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
                pchat_mode: Some(PchatMode::FullArchive),
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
        state.messages.get(&9).map(|m| m.is_empty()).unwrap_or(true),
        "TextMessage should be skipped for FullArchive channel"
    );
}

#[test]
fn text_message_mixed_pchat_and_regular_channels() {
    let (ctx, _) = make_ctx();
    {
        let mut state = ctx.shared.lock().unwrap();
        state.own_session = Some(1);
        state.users.insert(10, make_user(10, "Eve"));
        // Channel 2: pchat enabled
        state.channels.insert(
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
                pchat_mode: Some(PchatMode::PostJoin),
                pchat_max_history: None,
                pchat_retention_days: None,
                    pchat_key_custodians: Vec::new(),            },
        );
        // Channel 4: no pchat
        state.channels.insert(
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
                pchat_mode: None,
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
        state.messages.get(&2).map(|m| m.is_empty()).unwrap_or(true),
        "pchat channel should not store TextMessage"
    );
    // Channel 4 (regular) should have the message.
    let msgs = state.messages.get(&4).unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].body, "Multi-channel");
}
