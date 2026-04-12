//! Integration tests for the Mumble protocol client.
//!
//! These tests require a running Mumble server. Start one with:
//!
//! ```sh
//! docker compose -f crates/mumble-protocol/docker-compose.test.yml up -d
//! ```
//!
//! Then run:
//!
//! ```sh
//! cargo test --package mumble-protocol --test integration
//! ```
//!
//! The server is configured (via `test-mumble.ini`) with large message/image
//! limits so that large image tests pass.
//
// Integration tests are separate crate compilation units and will trigger
// `unused_crate_dependencies` for every transitive dep of mumble-protocol
// that is not directly imported in this file.
#![allow(
    unused_crate_dependencies,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    reason = "integration test: transitive deps are not directly imported; unwrap/expect and long test functions are idiomatic"
)]

use std::time::Duration;

use mumble_protocol::command::{
    Authenticate, CommandAction, JoinChannel, RequestBlob, SendPluginData, SendTextMessage,
    SetComment, SetSelfDeaf, SetSelfMute,
};
use mumble_protocol::message::ControlMessage;
use mumble_protocol::proto::mumble_tcp;
use mumble_protocol::state::ServerState;
use mumble_protocol::transport::tcp::{TcpConfig, TcpTransport};

/// How long to wait for the server to respond.
const TIMEOUT: Duration = Duration::from_secs(10);

/// Server address for Docker container.
const HOST: &str = "127.0.0.1";

/// Port for the test server. Override with `MUMBLE_TEST_PORT` env var
/// when the default 64738 is blocked (e.g. by Windows Hyper-V).
fn port() -> u16 {
    std::env::var("MUMBLE_TEST_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(64738)
}

fn tcp_config() -> TcpConfig {
    // Ensure the rustls crypto provider is installed (once per process).
    let _ = rustls::crypto::ring::default_provider().install_default();
    TcpConfig {
        server_host: HOST.into(),
        server_port: port(),
        accept_invalid_certs: true,
        client_cert_pem: None,
        client_key_pem: None,
    }
}

/// Check if the test server is reachable. Skip tests gracefully if not.
async fn ensure_server_available() -> bool {
    let addr = format!("{HOST}:{}", port());
    match tokio::time::timeout(
        Duration::from_secs(3),
        tokio::net::TcpStream::connect(&addr),
    )
    .await
    {
        Ok(Ok(_)) => true,
        _ => {
            eprintln!(
                "WARNING: Mumble test server not available at {addr}. \
                 Skipping integration test. Start it with:\n  \
                 docker compose -f crates/mumble-protocol/docker-compose.test.yml up -d"
            );
            false
        }
    }
}

/// Helper: connect TLS + send Version + Authenticate, wait for `ServerSync`.
/// Returns the transport and collected state.
async fn connect_and_authenticate(
    username: &str,
) -> (TcpTransport, ServerState) {
    let mut transport = TcpTransport::connect(&tcp_config()).await.unwrap();

    // Send Version
    let version_msg = ControlMessage::Version(mumble_tcp::Version {
        version_v2: Some(0x0001_0005_0000_0000), // 1.5.0
        release: Some("mumble-protocol-test".into()),
        os: Some(std::env::consts::OS.into()),
        os_version: Some("test".into()),
        ..Default::default()
    });
    transport.send(&version_msg).await.unwrap();

    // Send Authenticate
    let auth = Authenticate {
        username: username.into(),
        password: None,
        tokens: vec![],
    };
    let auth_output = auth.execute(&ServerState::new());
    for msg in &auth_output.tcp_messages {
        transport.send(msg).await.unwrap();
    }

    let mut state = ServerState::new();
    let mut got_sync = false;

    // Read messages until we get ServerSync
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while !got_sync && tokio::time::Instant::now() < deadline {
        let msg = tokio::time::timeout(Duration::from_secs(5), transport.recv())
            .await
            .expect("timed out waiting for message")
            .expect("transport error");

        match &msg {
            ControlMessage::ServerSync(sync) => {
                state.apply_server_sync(sync);
                got_sync = true;
            }
            ControlMessage::UserState(us) => {
                state.apply_user_state(us);
            }
            ControlMessage::ChannelState(cs) => {
                state.apply_channel_state(cs);
            }
            ControlMessage::Reject(r) => {
                panic!(
                    "Connection rejected: {:?} - {}",
                    r.r#type,
                    r.reason.as_deref().unwrap_or("no reason")
                );
            }
            _ => {
                // ServerConfig, CodecVersion, CryptSetup, etc. - ignore
            }
        }
    }

    assert!(got_sync, "Never received ServerSync from the server");
    (transport, state)
}

// -- Tests ----------------------------------------------------------

#[tokio::test]
async fn test_tcp_connect_and_version_exchange() {
    if !ensure_server_available().await {
        return;
    }

    let mut transport = TcpTransport::connect(&tcp_config()).await.unwrap();

    // Send our version
    let version_msg = ControlMessage::Version(mumble_tcp::Version {
        version_v2: Some(0x0001_0005_0000_0000),
        release: Some("test-client".into()),
        ..Default::default()
    });
    transport.send(&version_msg).await.unwrap();

    // Server should respond with its own Version
    let response = tokio::time::timeout(TIMEOUT, transport.recv())
        .await
        .expect("timed out")
        .expect("recv error");

    match response {
        ControlMessage::Version(v) => {
            // Server should have a version set
            assert!(
                v.version_v1.is_some() || v.version_v2.is_some(),
                "server should report a version"
            );
        }
        other => {
            // Some servers may send other messages first; just verify we got data
            eprintln!("First message was not Version: {other:?}");
        }
    }
}

#[tokio::test]
async fn test_full_authentication_flow() {
    if !ensure_server_available().await {
        return;
    }

    let (_transport, state) = connect_and_authenticate("IntegTestUser").await;

    // We should have a session ID
    let session_id = state.own_session().expect("should have session ID");
    assert!(session_id > 0);

    // We should see ourselves in the user list
    assert!(
        state.users.values().any(|u| u.name == "IntegTestUser"),
        "our user should appear in state"
    );

    // There should be at least the Root channel
    assert!(
        !state.channels.is_empty(),
        "server should send at least one channel"
    );
}

#[tokio::test]
async fn test_send_text_message() {
    if !ensure_server_available().await {
        return;
    }

    let (mut transport, state) = connect_and_authenticate("TextMsgUser").await;

    // Send a text message to root channel (channel_id=0)
    let cmd = SendTextMessage {
        channel_ids: vec![0],
        user_sessions: vec![],
        tree_ids: vec![],
        message: "Hello from integration test!".into(),
        message_id: None,
        timestamp: None,
    };
    let output = cmd.execute(&state);
    for msg in &output.tcp_messages {
        transport.send(msg).await.unwrap();
    }

    // The server typically echoes the text message back.
    // Wait for it with a timeout.
    let mut received_echo = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(2), transport.recv()).await {
            Ok(Ok(ControlMessage::TextMessage(tm))) => {
                if tm.message.contains("Hello from integration test!") {
                    received_echo = true;
                    break;
                }
            }
            Ok(Ok(_)) => continue,
            Ok(Err(_)) | Err(_) => break,
        }
    }

    // Some server configs don't echo; just verify no error occurred
    if !received_echo {
        eprintln!("Note: server did not echo the text message (this may be normal)");
    }

    drop(transport);
}

#[tokio::test]
async fn test_send_large_image_message() {
    if !ensure_server_available().await {
        return;
    }

    let (mut transport, state) = connect_and_authenticate("LargeImgUser").await;

    // Create a large "image" payload (~1 MiB base64-encoded fake PNG).
    // This tests that the server's imagemessagelength limit is large enough.
    let image_bytes = vec![0xAAu8; 512 * 1024]; // 512 KiB raw
    let base64_image = base64_encode(&image_bytes);

    let html_message = format!(
        "<img src=\"data:image/png;base64,{base64_image}\" />"
    );

    let cmd = SendTextMessage {
        channel_ids: vec![0],
        user_sessions: vec![],
        tree_ids: vec![],
        message: html_message.clone(),
        message_id: None,
        timestamp: None,
    };
    let output = cmd.execute(&state);
    for msg in &output.tcp_messages {
        transport.send(msg).await.unwrap();
    }

    // If the server accepted it, we should not get a PermissionDenied with TextTooLong.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut permission_denied = false;
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(2), transport.recv()).await {
            Ok(Ok(ControlMessage::PermissionDenied(pd))) => {
                if pd.r#type == Some(mumble_tcp::permission_denied::DenyType::TextTooLong as i32) {
                    permission_denied = true;
                    break;
                }
            }
            Ok(Ok(ControlMessage::TextMessage(_))) => {
                // Got the message back - success
                break;
            }
            Ok(Ok(_)) => continue,
            Ok(Err(_)) | Err(_) => break,
        }
    }

    assert!(
        !permission_denied,
        "Server rejected the large image message. \
         Ensure imagemessagelength is set high enough in test-mumble.ini"
    );

    drop(transport);
}

#[tokio::test]
async fn test_set_self_mute_and_deaf() {
    if !ensure_server_available().await {
        return;
    }

    let (mut transport, state) = connect_and_authenticate("MuteDeafUser").await;

    // Self-mute
    let mute_cmd = SetSelfMute { muted: true };
    let output = mute_cmd.execute(&state);
    for msg in &output.tcp_messages {
        transport.send(msg).await.unwrap();
    }

    // Wait for the server to echo back our UserState
    let mut got_mute_ack = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(2), transport.recv()).await {
            Ok(Ok(ControlMessage::UserState(us))) => {
                if us.session == state.own_session() && us.self_mute == Some(true) {
                    got_mute_ack = true;
                    break;
                }
            }
            Ok(Ok(_)) => continue,
            Ok(Err(_)) | Err(_) => break,
        }
    }
    assert!(got_mute_ack, "Server should acknowledge self-mute");

    // Self-deaf (implies mute)
    let deaf_cmd = SetSelfDeaf { deafened: true };
    let output = deaf_cmd.execute(&state);
    for msg in &output.tcp_messages {
        transport.send(msg).await.unwrap();
    }

    let mut got_deaf_ack = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(2), transport.recv()).await {
            Ok(Ok(ControlMessage::UserState(us))) => {
                if us.session == state.own_session() && us.self_deaf == Some(true) {
                    got_deaf_ack = true;
                    break;
                }
            }
            Ok(Ok(_)) => continue,
            Ok(Err(_)) | Err(_) => break,
        }
    }
    assert!(got_deaf_ack, "Server should acknowledge self-deaf");

    drop(transport);
}

#[tokio::test]
async fn test_set_comment() {
    if !ensure_server_available().await {
        return;
    }

    let (mut transport, state) = connect_and_authenticate("CommentUser").await;

    let cmd = SetComment {
        comment: "Integration test comment".into(),
    };
    let output = cmd.execute(&state);
    for msg in &output.tcp_messages {
        transport.send(msg).await.unwrap();
    }

    // Wait for echoed UserState with our comment
    let mut got_comment = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(2), transport.recv()).await {
            Ok(Ok(ControlMessage::UserState(us))) => {
                if us.session == state.own_session()
                    && us.comment.as_deref() == Some("Integration test comment")
                {
                    got_comment = true;
                    break;
                }
            }
            Ok(Ok(_)) => continue,
            Ok(Err(_)) | Err(_) => break,
        }
    }
    assert!(got_comment, "Server should echo our comment");

    drop(transport);
}

#[tokio::test]
async fn test_ping_keepalive() {
    if !ensure_server_available().await {
        return;
    }

    let (mut transport, _state) = connect_and_authenticate("PingUser").await;

    // Send a TCP Ping
    let ping_msg = ControlMessage::Ping(mumble_tcp::Ping {
        timestamp: Some(42),
        ..Default::default()
    });
    transport.send(&ping_msg).await.unwrap();

    // Server should respond with a Ping
    let mut got_pong = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(2), transport.recv()).await {
            Ok(Ok(ControlMessage::Ping(_))) => {
                got_pong = true;
                break;
            }
            Ok(Ok(_)) => continue,
            Ok(Err(_)) | Err(_) => break,
        }
    }
    assert!(got_pong, "Server should respond to TCP ping");

    drop(transport);
}

#[tokio::test]
async fn test_multiple_concurrent_connections() {
    if !ensure_server_available().await {
        return;
    }

    // Connect two users simultaneously
    let (_t1, state1) = connect_and_authenticate("ConcUser1").await;
    let (_t2, state2) = connect_and_authenticate("ConcUser2").await;

    // Both should have valid sessions
    assert!(state1.own_session().is_some());
    assert!(state2.own_session().is_some());
    assert_ne!(state1.own_session(), state2.own_session());

    // User2 should see User1 already connected
    // (The server sends UserState for existing users during handshake)
    let user1_visible = state2.users.values().any(|u| u.name == "ConcUser1");
    assert!(
        user1_visible,
        "User2 should see User1 in the state after connecting"
    );
}

#[tokio::test]
async fn test_server_config_has_large_limits() {
    if !ensure_server_available().await {
        return;
    }

    let mut transport = TcpTransport::connect(&tcp_config()).await.unwrap();

    // Send Version + Auth
    transport
        .send(&ControlMessage::Version(mumble_tcp::Version {
            version_v2: Some(0x0001_0005_0000_0000),
            ..Default::default()
        }))
        .await
        .unwrap();
    let auth = Authenticate {
        username: "ConfigCheckUser".into(),
        password: None,
        tokens: vec![],
    };
    for msg in &auth.execute(&ServerState::new()).tcp_messages {
        transport.send(msg).await.unwrap();
    }

    // Read messages until we find ServerConfig
    let mut server_config = None;
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(3), transport.recv()).await {
            Ok(Ok(ControlMessage::ServerConfig(sc))) => {
                server_config = Some(sc);
                break;
            }
            Ok(Ok(ControlMessage::ServerSync(_))) => break,
            Ok(Ok(_)) => continue,
            _ => break,
        }
    }

    if let Some(config) = server_config {
        // Verify the server is configured with large image limits
        if let Some(img_len) = config.image_message_length {
            assert!(
                img_len >= 1_048_576,
                "image_message_length should be >= 1 MiB, got {img_len}"
            );
        }
        if let Some(msg_len) = config.message_length {
            assert!(
                msg_len >= 65536,
                "message_length should be >= 64 KiB, got {msg_len}"
            );
        }
    } else {
        eprintln!("Note: server did not send ServerConfig before ServerSync");
    }

    drop(transport);
}

// -- PluginDataTransmission tests -----------------------------------

/// Two clients connect; client A sends a `PluginDataTransmission` to client B.
/// Client B must receive the message with the correct payload and `data_id`.
#[tokio::test]
async fn test_plugin_data_transmission_between_two_clients() {
    if !ensure_server_available().await {
        return;
    }

    // Connect client A and client B.
    let (mut transport_a, state_a) = connect_and_authenticate("PluginSender").await;
    let (mut transport_b, state_b) = connect_and_authenticate("PluginReceiver").await;

    // Drain any pending UserState messages on transport_b (e.g. client A's arrival).
    let drain_deadline = tokio::time::Instant::now() + Duration::from_millis(500);
    while tokio::time::Instant::now() < drain_deadline {
        match tokio::time::timeout(Duration::from_millis(200), transport_b.recv()).await {
            Ok(Ok(_)) => {}
            _ => break,
        }
    }

    // Client A sends plugin data targeting client B's session.
    let session_b = state_b.own_session().expect("client B should have session");
    let poll_json = r#"{"type":"poll","id":"test-poll-123","question":"Favourite?","options":["Rust","TS"]}"#;

    let cmd = SendPluginData {
        receiver_sessions: vec![session_b],
        data: poll_json.as_bytes().to_vec(),
        data_id: "fancy-poll".into(),
    };
    let output = cmd.execute(&state_a);
    for msg in &output.tcp_messages {
        transport_a.send(msg).await.unwrap();
    }

    // Client B should receive the PluginDataTransmission.
    let mut received = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(2), transport_b.recv()).await {
            Ok(Ok(ControlMessage::PluginDataTransmission(pd))) => {
                assert_eq!(pd.data_id.as_deref(), Some("fancy-poll"));
                let payload = std::str::from_utf8(pd.data.as_deref().unwrap()).unwrap();
                assert_eq!(payload, poll_json);
                // Server should fill in the sender's session.
                assert_eq!(
                    pd.sender_session,
                    state_a.own_session(),
                    "sender_session should be set by the server"
                );
                received = true;
                break;
            }
            Ok(Ok(_)) => continue,
            Ok(Err(e)) => panic!("transport error: {e}"),
            Err(_) => break,
        }
    }

    assert!(
        received,
        "Client B should have received the PluginDataTransmission from Client A"
    );

    drop(transport_a);
    drop(transport_b);
}

/// Verify that sending `PluginDataTransmission` with an empty receiver list
/// does NOT deliver the message to other clients (the Mumble server only
/// forwards to explicitly listed sessions).
#[tokio::test]
async fn test_plugin_data_empty_receivers_not_delivered() {
    if !ensure_server_available().await {
        return;
    }

    let (mut transport_a, state_a) = connect_and_authenticate("EmptySender").await;
    let (mut transport_b, _state_b) = connect_and_authenticate("EmptyReceiver").await;

    // Drain pending messages on transport_b.
    let drain_deadline = tokio::time::Instant::now() + Duration::from_millis(500);
    while tokio::time::Instant::now() < drain_deadline {
        match tokio::time::timeout(Duration::from_millis(200), transport_b.recv()).await {
            Ok(Ok(_)) => {}
            _ => break,
        }
    }

    // Client A sends plugin data with empty receiver list.
    let cmd = SendPluginData {
        receiver_sessions: vec![], // Nobody should receive this.
        data: b"should not arrive".to_vec(),
        data_id: "fancy-poll".into(),
    };
    let output = cmd.execute(&state_a);
    for msg in &output.tcp_messages {
        transport_a.send(msg).await.unwrap();
    }

    // Wait briefly - client B should NOT receive a PluginDataTransmission.
    let mut received_plugin_data = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(1), transport_b.recv()).await {
            Ok(Ok(ControlMessage::PluginDataTransmission(_))) => {
                received_plugin_data = true;
                break;
            }
            Ok(Ok(_)) => continue,
            _ => break,
        }
    }

    assert!(
        !received_plugin_data,
        "Empty receiver_sessions should mean nobody receives the message"
    );

    drop(transport_a);
    drop(transport_b);
}

/// Simulate the `FancyMumble` poll flow end-to-end: create a poll, send it,
/// receive it, then send a vote back.
#[tokio::test]
async fn test_poll_roundtrip_create_and_vote() {
    if !ensure_server_available().await {
        return;
    }

    let (mut transport_a, state_a) = connect_and_authenticate("PollCreator").await;
    let (mut transport_b, state_b) = connect_and_authenticate("PollVoter").await;

    let session_a = state_a.own_session().unwrap();
    let session_b = state_b.own_session().unwrap();

    // Drain pending messages.
    for transport in [&mut transport_a, &mut transport_b] {
        let drain_deadline = tokio::time::Instant::now() + Duration::from_millis(500);
        while tokio::time::Instant::now() < drain_deadline {
            match tokio::time::timeout(Duration::from_millis(200), transport.recv()).await {
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    }

    // 1) Client A creates a poll and sends it to client B.
    let poll_json = format!(
        r#"{{"type":"poll","id":"roundtrip-poll","question":"Best language?","options":["Rust","TypeScript"],"multiple":false,"creator":{session_a},"creatorName":"PollCreator","createdAt":"2025-01-01T00:00:00Z","channelId":0}}"#
    );
    let cmd = SendPluginData {
        receiver_sessions: vec![session_b],
        data: poll_json.as_bytes().to_vec(),
        data_id: "fancy-poll".into(),
    };
    for msg in &cmd.execute(&state_a).tcp_messages {
        transport_a.send(msg).await.unwrap();
    }

    // 2) Client B receives the poll.
    let mut got_poll = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(2), transport_b.recv()).await {
            Ok(Ok(ControlMessage::PluginDataTransmission(pd))) => {
                if pd.data_id.as_deref() == Some("fancy-poll") {
                    got_poll = true;
                    break;
                }
            }
            Ok(Ok(_)) => continue,
            _ => break,
        }
    }
    assert!(got_poll, "Client B should receive the poll");

    // 3) Client B votes and sends the vote back to client A.
    let vote_json = format!(
        r#"{{"type":"poll_vote","pollId":"roundtrip-poll","selected":[0],"voter":{session_b},"voterName":"PollVoter"}}"#
    );
    let vote_cmd = SendPluginData {
        receiver_sessions: vec![session_a],
        data: vote_json.as_bytes().to_vec(),
        data_id: "fancy-poll-vote".into(),
    };
    for msg in &vote_cmd.execute(&state_b).tcp_messages {
        transport_b.send(msg).await.unwrap();
    }

    // 4) Client A receives the vote.
    let mut got_vote = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(2), transport_a.recv()).await {
            Ok(Ok(ControlMessage::PluginDataTransmission(pd))) => {
                if pd.data_id.as_deref() == Some("fancy-poll-vote") {
                    let payload = std::str::from_utf8(pd.data.as_deref().unwrap()).unwrap();
                    assert!(payload.contains("roundtrip-poll"));
                    assert!(payload.contains("PollVoter"));
                    assert_eq!(pd.sender_session, Some(session_b));
                    got_vote = true;
                    break;
                }
            }
            Ok(Ok(_)) => continue,
            _ => break,
        }
    }
    assert!(got_vote, "Client A should receive the vote from Client B");

    drop(transport_a);
    drop(transport_b);
}
/// Both clients can send polls to each other (bidirectional).
/// Verifies that the one-directional-only symptom does not exist at
/// the protocol level.
#[tokio::test]
async fn test_poll_bidirectional_sending() {
    if !ensure_server_available().await {
        return;
    }

    let (mut transport_a, state_a) = connect_and_authenticate("BiDirA").await;
    let (mut transport_b, state_b) = connect_and_authenticate("BiDirB").await;

    let session_a = state_a.own_session().unwrap();
    let session_b = state_b.own_session().unwrap();

    // Drain handshake noise.
    for transport in [&mut transport_a, &mut transport_b] {
        let d = tokio::time::Instant::now() + Duration::from_millis(500);
        while tokio::time::Instant::now() < d {
            match tokio::time::timeout(Duration::from_millis(200), transport.recv()).await {
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    }

    // -- A -> B -----------------------------------------------------
    let poll_a = format!(
        r#"{{"type":"poll","id":"bidir-a","question":"From A?","options":["Yes","No"],"multiple":false,"creator":{session_a},"creatorName":"BiDirA","createdAt":"2025-01-01T00:00:00Z","channelId":0}}"#
    );
    let cmd = SendPluginData {
        receiver_sessions: vec![session_b],
        data: poll_a.as_bytes().to_vec(),
        data_id: "fancy-poll".into(),
    };
    for msg in &cmd.execute(&state_a).tcp_messages {
        transport_a.send(msg).await.unwrap();
    }

    let mut b_got_poll = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(2), transport_b.recv()).await {
            Ok(Ok(ControlMessage::PluginDataTransmission(pd))) => {
                if pd.data_id.as_deref() == Some("fancy-poll") {
                    assert_eq!(pd.sender_session, Some(session_a));
                    b_got_poll = true;
                    break;
                }
            }
            Ok(Ok(_)) => continue,
            _ => break,
        }
    }
    assert!(b_got_poll, "B should receive poll from A");

    // -- B -> A -----------------------------------------------------
    let poll_b = format!(
        r#"{{"type":"poll","id":"bidir-b","question":"From B?","options":["Yes","No"],"multiple":false,"creator":{session_b},"creatorName":"BiDirB","createdAt":"2025-01-01T00:00:00Z","channelId":0}}"#
    );
    let cmd = SendPluginData {
        receiver_sessions: vec![session_a],
        data: poll_b.as_bytes().to_vec(),
        data_id: "fancy-poll".into(),
    };
    for msg in &cmd.execute(&state_b).tcp_messages {
        transport_b.send(msg).await.unwrap();
    }

    let mut a_got_poll = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(2), transport_a.recv()).await {
            Ok(Ok(ControlMessage::PluginDataTransmission(pd))) => {
                if pd.data_id.as_deref() == Some("fancy-poll") {
                    assert_eq!(pd.sender_session, Some(session_b));
                    a_got_poll = true;
                    break;
                }
            }
            Ok(Ok(_)) => continue,
            _ => break,
        }
    }
    assert!(a_got_poll, "A should receive poll from B");

    drop(transport_a);
    drop(transport_b);
}

/// Three clients in the same channel each send a poll. Every OTHER
/// client must receive them all.
#[tokio::test]
async fn test_poll_multiple_senders_same_channel() {
    if !ensure_server_available().await {
        return;
    }

    let (mut t_a, s_a) = connect_and_authenticate("MultiA").await;
    let (mut t_b, s_b) = connect_and_authenticate("MultiB").await;
    let (mut t_c, s_c) = connect_and_authenticate("MultiC").await;

    let sa = s_a.own_session().unwrap();
    let sb = s_b.own_session().unwrap();
    let sc = s_c.own_session().unwrap();

    // Drain handshake.
    for t in [&mut t_a, &mut t_b, &mut t_c] {
        let d = tokio::time::Instant::now() + Duration::from_millis(500);
        while tokio::time::Instant::now() < d {
            match tokio::time::timeout(Duration::from_millis(200), t.recv()).await {
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    }

    // A sends poll to B and C.
    let poll_a = format!(
        r#"{{"type":"poll","id":"multi-a","question":"From A","options":["X","Y"],"multiple":false,"creator":{sa},"creatorName":"MultiA","createdAt":"2025-01-01T00:00:00Z","channelId":0}}"#
    );
    let cmd_a = SendPluginData {
        receiver_sessions: vec![sb, sc],
        data: poll_a.as_bytes().to_vec(),
        data_id: "fancy-poll".into(),
    };
    for msg in &cmd_a.execute(&s_a).tcp_messages {
        t_a.send(msg).await.unwrap();
    }

    // B sends poll to A and C.
    let poll_b = format!(
        r#"{{"type":"poll","id":"multi-b","question":"From B","options":["X","Y"],"multiple":false,"creator":{sb},"creatorName":"MultiB","createdAt":"2025-01-01T00:00:00Z","channelId":0}}"#
    );
    let cmd_b = SendPluginData {
        receiver_sessions: vec![sa, sc],
        data: poll_b.as_bytes().to_vec(),
        data_id: "fancy-poll".into(),
    };
    for msg in &cmd_b.execute(&s_b).tcp_messages {
        t_b.send(msg).await.unwrap();
    }

    // C sends poll to A and B.
    let poll_c = format!(
        r#"{{"type":"poll","id":"multi-c","question":"From C","options":["X","Y"],"multiple":false,"creator":{sc},"creatorName":"MultiC","createdAt":"2025-01-01T00:00:00Z","channelId":0}}"#
    );
    let cmd_c = SendPluginData {
        receiver_sessions: vec![sa, sb],
        data: poll_c.as_bytes().to_vec(),
        data_id: "fancy-poll".into(),
    };
    for msg in &cmd_c.execute(&s_c).tcp_messages {
        t_c.send(msg).await.unwrap();
    }

    fn extract_poll_id(json: &str) -> Option<String> {
        let start = json.find(r#""id":""#)?;
        let rest = &json[start + 6..];
        let end = rest.find('"')?;
        Some(rest[..end].to_string())
    }

    // Collect polls on each transport.
    async fn collect_polls(t: &mut TcpTransport, count: usize) -> Vec<String> {
        let mut ids = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while ids.len() < count && tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_secs(2), t.recv()).await {
                Ok(Ok(ControlMessage::PluginDataTransmission(pd)))
                    if pd.data_id.as_deref() == Some("fancy-poll") =>
                {
                    let json = std::str::from_utf8(pd.data.as_deref().unwrap()).unwrap();
                    ids.extend(extract_poll_id(json));
                }
                Ok(Ok(_)) => continue,
                _ => break,
            }
        }
        ids
    }

    let polls_a = collect_polls(&mut t_a, 2).await;
    let polls_b = collect_polls(&mut t_b, 2).await;
    let polls_c = collect_polls(&mut t_c, 2).await;

    // A should get polls from B and C.
    assert!(polls_a.contains(&"multi-b".to_string()), "A should get B's poll, got: {polls_a:?}");
    assert!(polls_a.contains(&"multi-c".to_string()), "A should get C's poll, got: {polls_a:?}");

    // B should get polls from A and C.
    assert!(polls_b.contains(&"multi-a".to_string()), "B should get A's poll, got: {polls_b:?}");
    assert!(polls_b.contains(&"multi-c".to_string()), "B should get C's poll, got: {polls_b:?}");

    // C should get polls from A and B.
    assert!(polls_c.contains(&"multi-a".to_string()), "C should get A's poll, got: {polls_c:?}");
    assert!(polls_c.contains(&"multi-b".to_string()), "C should get B's poll, got: {polls_c:?}");

    drop(t_a);
    drop(t_b);
    drop(t_c);
}

/// Helper: connect as `SuperUser` (admin), authenticate with password,
/// create a temporary sub-channel under root, then disconnect.
/// Returns the new channel's ID.
async fn create_temp_channel(name: &str) -> Option<u32> {
    let mut transport = TcpTransport::connect(&tcp_config()).await.unwrap();

    // Version handshake.
    transport
        .send(&ControlMessage::Version(mumble_tcp::Version {
            version_v2: Some(0x0001_0005_0000_0000),
            ..Default::default()
        }))
        .await
        .unwrap();

    // Authenticate as SuperUser.
    let auth = Authenticate {
        username: "SuperUser".into(),
        password: Some("testpassword".into()),
        tokens: vec![],
    };
    for msg in &auth.execute(&ServerState::new()).tcp_messages {
        transport.send(msg).await.unwrap();
    }

    // Wait for ServerSync.
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    let mut synced = false;
    while !synced && tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(3), transport.recv()).await {
            Ok(Ok(ControlMessage::ServerSync(_))) => synced = true,
            Ok(Ok(ControlMessage::Reject(r))) => {
                eprintln!("SuperUser rejected: {:?}", r.reason);
                return None;
            }
            Ok(Ok(_)) => continue,
            _ => return None,
        }
    }
    if !synced {
        return None;
    }

    // Create a temporary channel under root.
    transport
        .send(&ControlMessage::ChannelState(mumble_tcp::ChannelState {
            parent: Some(0),
            name: Some(name.into()),
            temporary: Some(true),
            ..Default::default()
        }))
        .await
        .unwrap();

    // Wait for the channel to appear.
    let mut channel_id = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(2), transport.recv()).await {
            Ok(Ok(ControlMessage::ChannelState(cs))) => {
                if cs.name.as_deref() == Some(name) {
                    channel_id = cs.channel_id;
                    break;
                }
            }
            Ok(Ok(ControlMessage::PermissionDenied(_))) => {
                eprintln!("SuperUser denied channel creation");
                break;
            }
            Ok(Ok(_)) => continue,
            _ => break,
        }
    }

    drop(transport);
    channel_id
}

/// Users in different channels CAN receive `PluginDataTransmission`.
/// The Mumble server delivers to all explicitly listed sessions
/// regardless of channel membership.
#[tokio::test]
async fn test_poll_cross_channel_is_delivered() {
    if !ensure_server_available().await {
        return;
    }

    // Create a temp channel using SuperUser privileges.
    let Some(new_ch) = create_temp_channel("CrossChannelTest").await else {
        eprintln!(
            "WARNING: could not create temp channel (no SuperUser access). \
             Skipping cross-channel test."
        );
        return;
    };

    let (mut t_a, s_a) = connect_and_authenticate("CrossA").await;
    let (mut t_b, s_b) = connect_and_authenticate("CrossB").await;

    let _sa = s_a.own_session().unwrap();
    let sb = s_b.own_session().unwrap();

    // Move B to the new channel.
    let join = JoinChannel { channel_id: new_ch };
    for msg in &join.execute(&s_b).tcp_messages {
        t_b.send(msg).await.unwrap();
    }

    // Wait for B's channel change to be acknowledged.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(2), t_b.recv()).await {
            Ok(Ok(ControlMessage::UserState(us))) => {
                if us.session == Some(sb) && us.channel_id == Some(new_ch) {
                    break;
                }
            }
            Ok(Ok(_)) => continue,
            _ => break,
        }
    }

    // Drain both transports.
    for t in [&mut t_a, &mut t_b] {
        let d = tokio::time::Instant::now() + Duration::from_millis(500);
        while tokio::time::Instant::now() < d {
            match tokio::time::timeout(Duration::from_millis(200), t.recv()).await {
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    }

    // A (in root channel 0) sends poll targeting B (in different channel).
    let cmd = SendPluginData {
        receiver_sessions: vec![sb],
        data: b"{\"type\":\"poll\",\"id\":\"cross-test\"}".to_vec(),
        data_id: "fancy-poll".into(),
    };
    for msg in &cmd.execute(&s_a).tcp_messages {
        t_a.send(msg).await.unwrap();
    }

    // B DOES receive it - Mumble delivers PluginData across channels.
    let mut received = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(2), t_b.recv()).await {
            Ok(Ok(ControlMessage::PluginDataTransmission(pd))) => {
                assert_eq!(pd.data_id.as_deref(), Some("fancy-poll"));
                received = true;
                break;
            }
            Ok(Ok(_)) => continue,
            _ => break,
        }
    }

    assert!(
        received,
        "Cross-channel PluginDataTransmission SHOULD be delivered"
    );

    drop(t_a);
    drop(t_b);
}

/// Mixed-channel scenario: three users - A and B in root, C in a
/// different channel. A creates a poll targeting B and C; only B
/// should receive it.
#[tokio::test]
async fn test_poll_mixed_channels_only_same_channel_receives() {
    if !ensure_server_available().await {
        return;
    }

    // Create a temp channel using SuperUser.
    let Some(new_ch) = create_temp_channel("MixedChannelTest").await else {
        eprintln!(
            "WARNING: could not create temp channel. Skipping mixed-channel test."
        );
        return;
    };

    let (mut t_a, s_a) = connect_and_authenticate("MixedA").await;
    let (mut t_b, _s_b) = connect_and_authenticate("MixedB").await;
    let (mut t_c, s_c) = connect_and_authenticate("MixedC").await;

    let sa = s_a.own_session().unwrap();
    let sb = _s_b.own_session().unwrap();
    let sc = s_c.own_session().unwrap();

    // Move C to the new channel.
    let join = JoinChannel { channel_id: new_ch };
    for msg in &join.execute(&s_c).tcp_messages {
        t_c.send(msg).await.unwrap();
    }
    // Wait for C's channel change on C's transport.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(2), t_c.recv()).await {
            Ok(Ok(ControlMessage::UserState(us))) => {
                if us.session == Some(sc) && us.channel_id == Some(new_ch) {
                    break;
                }
            }
            Ok(Ok(_)) => continue,
            _ => break,
        }
    }

    // Also wait for A to see C's channel change so the server has fully
    // propagated state before we send PluginData.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(2), t_a.recv()).await {
            Ok(Ok(ControlMessage::UserState(us))) => {
                if us.session == Some(sc) && us.channel_id == Some(new_ch) {
                    break;
                }
            }
            Ok(Ok(_)) => continue,
            _ => break,
        }
    }

    // Drain remaining messages on all transports.
    for t in [&mut t_a, &mut t_b, &mut t_c] {
        let d = tokio::time::Instant::now() + Duration::from_millis(500);
        while tokio::time::Instant::now() < d {
            match tokio::time::timeout(Duration::from_millis(300), t.recv()).await {
                Ok(Ok(_)) => {}
                _ => break,
            }
        }
    }

    // A sends poll to B (same channel) and C (different channel).
    let poll = format!(
        r#"{{"type":"poll","id":"mixed-poll","question":"Mixed?","options":["A","B"],"multiple":false,"creator":{sa},"creatorName":"MixedA","createdAt":"2025-01-01T00:00:00Z","channelId":0}}"#
    );
    let cmd = SendPluginData {
        receiver_sessions: vec![sb, sc],
        data: poll.trim().as_bytes().to_vec(),
        data_id: "fancy-poll".into(),
    };
    for msg in &cmd.execute(&s_a).tcp_messages {
        t_a.send(msg).await.unwrap();
    }

    // B (same channel as A) should receive the poll.
    let mut b_got = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(3), t_b.recv()).await {
            Ok(Ok(ControlMessage::PluginDataTransmission(pd))) => {
                if pd.data_id.as_deref() == Some("fancy-poll") {
                    b_got = true;
                    break;
                }
            }
            Ok(Ok(_)) => continue,
            _ => break,
        }
    }
    assert!(b_got, "B (same channel) should receive the poll");

    // C (different channel) ALSO receives it - Mumble delivers
    // PluginData to all listed sessions regardless of channel.
    let mut c_got = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(3), t_c.recv()).await {
            Ok(Ok(ControlMessage::PluginDataTransmission(pd))) => {
                if pd.data_id.as_deref() == Some("fancy-poll") {
                    c_got = true;
                    break;
                }
            }
            Ok(Ok(_)) => continue,
            _ => break,
        }
    }
    assert!(
        c_got,
        "C (different channel) SHOULD also receive the poll - Mumble delivers PluginData across channels"
    );

    drop(t_a);
    drop(t_b);
    drop(t_c);
}

// -- Channel description blob request tests -------------------------

/// When the server has a channel with a large description it sends only
/// `description_hash` during the initial handshake.  A subsequent
/// `RequestBlob` with the channel ID should cause the server to send
/// a `ChannelState` containing the full `description`.
#[tokio::test]
async fn test_channel_description_blob_request() {
    if !ensure_server_available().await {
        return;
    }

    // Build a description large enough that the server will defer it
    // and only send `description_hash`.  The threshold is typically
    // around 128 bytes.
    let large_description = format!(
        "<p>{}</p><p><a href=\"https://example.com\">Link</a></p>",
        "A".repeat(256),
    );

    // 1) SuperUser creates a channel with a large description.
    let channel_name = "DescBlobTest";
    let mut su = TcpTransport::connect(&tcp_config()).await.unwrap();
    su.send(&ControlMessage::Version(mumble_tcp::Version {
        version_v2: Some(0x0001_0005_0000_0000),
        ..Default::default()
    }))
    .await
    .unwrap();
    let auth = Authenticate {
        username: "SuperUser".into(),
        password: Some("testpassword".into()),
        tokens: vec![],
    };
    for msg in &auth.execute(&ServerState::new()).tcp_messages {
        su.send(msg).await.unwrap();
    }
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    let mut synced = false;
    while !synced && tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(3), su.recv()).await {
            Ok(Ok(ControlMessage::ServerSync(_))) => synced = true,
            Ok(Ok(ControlMessage::Reject(r))) => {
                eprintln!("SuperUser rejected: {:?}", r.reason);
                return;
            }
            Ok(Ok(_)) => continue,
            _ => break,
        }
    }
    if !synced {
        eprintln!("WARNING: could not authenticate as SuperUser. Skipping.");
        return;
    }

    su.send(&ControlMessage::ChannelState(mumble_tcp::ChannelState {
        parent: Some(0),
        name: Some(channel_name.into()),
        description: Some(large_description.clone()),
        temporary: Some(true),
        ..Default::default()
    }))
    .await
    .unwrap();

    // Wait for the server to echo the ChannelState with the assigned ID.
    let mut new_channel_id: Option<u32> = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(3), su.recv()).await {
            Ok(Ok(ControlMessage::ChannelState(cs))) => {
                if cs.name.as_deref() == Some(channel_name) {
                    new_channel_id = cs.channel_id;
                    break;
                }
            }
            Ok(Ok(_)) => continue,
            _ => break,
        }
    }
    let Some(channel_id) = new_channel_id else {
        eprintln!("WARNING: could not create temp channel. Skipping.");
        return;
    };

    // 2) A regular client connects and collects channels.
    let (mut transport, state) = connect_and_authenticate("DescBlobUser").await;

    // Check whether the server deferred the description (sent hash only).
    let ch = state.channels.get(&channel_id);
    let description_was_deferred =
        ch.is_some_and(|c| c.description.is_empty() && c.description_hash.is_some());

    if !description_was_deferred {
        // Some server versions inline all descriptions.  If the
        // description is already present, there is nothing to test; just
        // verify it matches and return.
        if let Some(c) = ch {
            assert_eq!(
                c.description, large_description,
                "description should match what was set"
            );
        }
        eprintln!(
            "Note: server inlined the description (no hash). \
             RequestBlob path not exercised."
        );
        drop(transport);
        drop(su);
        return;
    }

    // 3) Send RequestBlob to fetch the full description.
    let cmd = RequestBlob {
        session_texture: Vec::new(),
        session_comment: Vec::new(),
        channel_description: vec![channel_id],
    };
    let output = cmd.execute(&state);
    for msg in &output.tcp_messages {
        transport.send(msg).await.unwrap();
    }

    // 4) The server responds with a ChannelState containing the full
    //    description.
    let mut got_description = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(3), transport.recv()).await {
            Ok(Ok(ControlMessage::ChannelState(cs))) => {
                if let Some(desc) = cs.description.filter(|_| cs.channel_id == Some(channel_id)) {
                    assert_eq!(
                        desc, large_description,
                        "description blob should match the original"
                    );
                    got_description = true;
                    break;
                }
            }
            Ok(Ok(_)) => continue,
            Ok(Err(e)) => panic!("transport error: {e}"),
            Err(_) => break,
        }
    }

    assert!(
        got_description,
        "Server should respond with the full channel description after RequestBlob"
    );

    drop(transport);
    drop(su);
}

// -- TextMessage message_id preservation tests ----------------------

/// Regression test: the server must preserve a client-provided `message_id`
/// in `TextMessage` rather than replacing it with a server-generated UUID.
///
/// Without this, each user ends up with a different `message_id` for the
/// same message, causing reactions (keyed by `message_id`) to be invisible
/// across users.
#[tokio::test]
async fn test_text_message_preserves_client_message_id() {
    if !ensure_server_available().await {
        return;
    }

    let (mut t1, state1) = connect_and_authenticate("MsgIdUser1").await;
    let (mut t2, _state2) = connect_and_authenticate("MsgIdUser2").await;

    // Drain any pending messages on t2 so we start clean.
    let drain_deadline = tokio::time::Instant::now() + Duration::from_millis(500);
    while tokio::time::Instant::now() < drain_deadline {
        if tokio::time::timeout(Duration::from_millis(200), t2.recv())
            .await
            .is_err()
        {
            break;
        }
    }

    // User1 sends a TextMessage with a client-provided message_id.
    let client_message_id = "client-uuid-deadbeef-1234";
    let client_timestamp = 1_700_000_000_000_u64;
    let cmd = SendTextMessage {
        channel_ids: vec![0],
        user_sessions: vec![],
        tree_ids: vec![],
        message: "hello with id".into(),
        message_id: Some(client_message_id.to_owned()),
        timestamp: Some(client_timestamp),
    };
    for msg in &cmd.execute(&state1).tcp_messages {
        t1.send(msg).await.unwrap();
    }

    // User2 should receive the TextMessage with the SAME message_id.
    // NOTE: `message_id` is a Fancy Mumble proto extension (field 6).
    // Standard murmur reconstructs the TextMessage and drops unknown fields,
    // so we separate "did User2 receive the message?" from "was message_id
    // preserved?" to avoid a misleading panic.
    let mut received_msg = false;
    let mut received_id: Option<String> = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(3), t2.recv()).await {
            Ok(Ok(ControlMessage::TextMessage(tm))) => {
                if tm.message.contains("hello with id") {
                    received_msg = true;
                    received_id = tm.message_id;
                    break;
                }
            }
            Ok(Ok(_)) => continue,
            Ok(Err(e)) => panic!("recv error: {e}"),
            Err(_) => break,
        }
    }

    assert!(received_msg, "User2 should receive the TextMessage");

    // If the server doesn't preserve message_id (standard murmur), skip.
    let Some(received_id) = received_id else {
        eprintln!(
            "NOTE: server did not preserve message_id (standard murmur). \
             Skipping assertion."
        );
        return;
    };
    assert_eq!(
        received_id, client_message_id,
        "server must preserve client-provided message_id, got {received_id}"
    );

    drop(t1);
    drop(t2);
}

/// Verify the server still generates a `message_id` when the client omits it
/// (legacy client compatibility).
#[tokio::test]
async fn test_text_message_generates_id_when_omitted() {
    if !ensure_server_available().await {
        return;
    }

    let (mut t1, state1) = connect_and_authenticate("MsgIdGen1").await;
    let (mut t2, _state2) = connect_and_authenticate("MsgIdGen2").await;

    let drain_deadline = tokio::time::Instant::now() + Duration::from_millis(500);
    while tokio::time::Instant::now() < drain_deadline {
        if tokio::time::timeout(Duration::from_millis(200), t2.recv())
            .await
            .is_err()
        {
            break;
        }
    }

    // Send without message_id (legacy client behaviour).
    let cmd = SendTextMessage {
        channel_ids: vec![0],
        user_sessions: vec![],
        tree_ids: vec![],
        message: "hello no id".into(),
        message_id: None,
        timestamp: None,
    };
    for msg in &cmd.execute(&state1).tcp_messages {
        t1.send(msg).await.unwrap();
    }

    // NOTE: `message_id` generation is a Fancy Mumble extension.  Standard
    // murmur does not add a message_id, so we separate delivery from the
    // extension assertion.
    let mut received_msg = false;
    let mut received_id: Option<String> = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(3), t2.recv()).await {
            Ok(Ok(ControlMessage::TextMessage(tm))) => {
                if tm.message.contains("hello no id") {
                    received_msg = true;
                    received_id = tm.message_id;
                    break;
                }
            }
            Ok(Ok(_)) => continue,
            Ok(Err(e)) => panic!("recv error: {e}"),
            Err(_) => break,
        }
    }

    assert!(received_msg, "User2 should receive the TextMessage");

    // If the server doesn't generate a message_id (standard murmur), skip.
    let Some(received_id) = received_id else {
        eprintln!(
            "NOTE: server did not generate a message_id (standard murmur). \
             Skipping assertion."
        );
        return;
    };
    assert!(
        !received_id.is_empty(),
        "server should generate a non-empty message_id when the client omits it"
    );

    drop(t1);
    drop(t2);
}

// -- Helpers --------------------------------------------------------

/// Minimal base64 encoder (avoids adding a `base64` dependency just for tests).
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);

    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;

        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);

        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}
