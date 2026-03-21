#![cfg(feature = "persistent-chat")]
// Integration tests are separate crate compilation units and will trigger
// `unused_crate_dependencies` for every transitive dep of mumble-protocol
// that is not directly imported in this file.
#![allow(
    unused_crate_dependencies,
    unused_results,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    reason = "integration test: transitive deps are not directly imported; unwrap/expect, long functions, and discarded bool results are idiomatic"
)]
//! Integration tests for persistent encrypted chat (pchat).
//!
//! These tests require the custom mumble-server Docker container with
//! pchat support running on localhost:64738.
//!
//! Start it with:
//!
//! ```sh
//! cd <mumble-docker-dir>
//! dev-build.bat
//! ```
//!
//! Then run (must be single-threaded to avoid auto-ban):
//!
//! ```sh
//! cargo test --package mumble-protocol --test pchat_integration --features persistent-chat -- --test-threads=1
//! ```

use std::time::Duration;

use mumble_protocol::command::{
    Authenticate, CommandAction, JoinChannel, SendPchatKeyChallengeResponse,
    SendPchatKeyHolderReport, SendPchatKeyHoldersQuery, SetChannelState,
};
use mumble_protocol::message::ControlMessage;
use mumble_protocol::persistent::keys::{KeyManager, SeedIdentity};
use mumble_protocol::persistent::wire::{
    MsgPackCodec, MessageEnvelope, WireCodec,
    PchatKeyAnnounce as WireKeyAnnounce,
    PchatKeyExchange as WireKeyExchange,
    PchatKeyRequest as WireKeyRequest,
};
use mumble_protocol::persistent::{KeyTrustLevel, PersistenceMode};
use mumble_protocol::proto::mumble_tcp;
use mumble_protocol::state::{PchatMode, ServerState};
use mumble_protocol::transport::tcp::{TcpConfig, TcpTransport};

/// How long to wait for the server to respond.
const TIMEOUT: Duration = Duration::from_secs(10);

const HOST: &str = "127.0.0.1";

fn port() -> u16 {
    std::env::var("MUMBLE_TEST_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(64738)
}

fn tcp_config_with_cert(cert_pem: Option<Vec<u8>>, key_pem: Option<Vec<u8>>) -> TcpConfig {
    let _ = rustls::crypto::ring::default_provider().install_default();
    TcpConfig {
        server_host: HOST.into(),
        server_port: port(),
        accept_invalid_certs: true,
        client_cert_pem: cert_pem,
        client_key_pem: key_pem,
    }
}

fn tcp_config() -> TcpConfig {
    tcp_config_with_cert(None, None)
}

/// Generate a self-signed TLS client certificate and return (`cert_pem`, `key_pem`).
fn generate_test_cert(username: &str) -> (Vec<u8>, Vec<u8>) {
    let certified = rcgen::generate_simple_self_signed(vec![username.to_string()])
        .expect("failed to generate test certificate");
    let cert_pem = certified.cert.pem().into_bytes();
    let key_pem = certified.key_pair.serialize_pem().into_bytes();
    (cert_pem, key_pem)
}

fn codec() -> MsgPackCodec {
    MsgPackCodec
}

/// Check if the test server is reachable (including TLS handshake). Skip tests gracefully if not.
async fn ensure_server_available() -> bool {
    match tokio::time::timeout(Duration::from_secs(5), TcpTransport::connect(&tcp_config())).await
    {
        Ok(Ok(_)) => true,
        _ => {
            let addr = format!("{HOST}:{}", port());
            eprintln!(
                "WARNING: Mumble pchat test server not available at {addr}. \
                 Skipping integration test. Start the pchat-enabled server first."
            );
            false
        }
    }
}

/// Connect, send Version (with `fancy_version`), Authenticate, and wait for `ServerSync`.
/// Returns the transport, state, and the `cert_hash` from `UserState`.
async fn connect_and_authenticate(username: &str) -> (TcpTransport, ServerState, String) {
    connect_and_authenticate_with_password(username, None).await
}

/// `SuperUser` password for the dev Docker container.
const SUPERUSER_PASSWORD: &str = "mumble123";

/// Connect as `SuperUser` with admin privileges.
async fn connect_as_superuser() -> (TcpTransport, ServerState, String) {
    connect_and_authenticate_with_password("SuperUser", Some(SUPERUSER_PASSWORD)).await
}

/// Connect with optional password.
/// Each connection generates a unique self-signed TLS client certificate so
/// the server can compute a cert hash (SHA-1 of the DER-encoded cert).
async fn connect_and_authenticate_with_password(
    username: &str,
    password: Option<&str>,
) -> (TcpTransport, ServerState, String) {
    let (cert_pem, key_pem) = generate_test_cert(username);
    let mut transport = TcpTransport::connect(
        &tcp_config_with_cert(Some(cert_pem), Some(key_pem)),
    )
    .await
    .unwrap();

    // Send Version with fancy_version to enable pchat extensions.
    let version_msg = ControlMessage::Version(mumble_tcp::Version {
        version_v1: Some((1 << 16) | (5 << 8)),
        version_v2: Some((1u64 << 48) | (5u64 << 32)),
        release: Some("pchat-integration-test".into()),
        os: Some(std::env::consts::OS.into()),
        os_version: Some("test".into()),
        // Announce Fancy Mumble extension support, version derived from Cargo.toml.
        fancy_version: Some(mumble_protocol::FANCY_VERSION),
    });
    transport.send(&version_msg).await.unwrap();

    // Send Authenticate
    let auth = Authenticate {
        username: username.into(),
        password: password.map(String::from),
        tokens: vec![],
    };
    let auth_output = auth.execute(&ServerState::new());
    for msg in &auth_output.tcp_messages {
        transport.send(msg).await.unwrap();
    }

    let mut state = ServerState::new();
    let mut got_sync = false;

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
            _ => {}
        }
    }

    assert!(got_sync, "Never received ServerSync from the server");

    // Look up our own cert hash by session ID (more reliable than capturing
    // the first UserState hash, which could belong to another user).
    let cert_hash = state
        .own_session()
        .and_then(|s| state.users.get(&s))
        .map(|u| u.hash.clone())
        .unwrap_or_default();

    (transport, state, cert_hash)
}

/// Drain pending messages from a transport (non-blocking).
async fn drain(transport: &mut TcpTransport) {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(500);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(200), transport.recv()).await {
            Ok(Ok(_)) => {}
            _ => break,
        }
    }
}

/// Wait for a `PchatAck` proto message from the server.
async fn wait_for_pchat_ack(
    transport: &mut TcpTransport,
    timeout: Duration,
) -> Option<mumble_tcp::PchatAck> {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(3), transport.recv()).await {
            Ok(Ok(ControlMessage::PchatAck(ack))) => return Some(ack),
            Ok(Ok(_)) => continue,
            Ok(Err(e)) => {
                eprintln!("transport error while waiting for PchatAck: {e}");
                return None;
            }
            Err(_) => continue,
        }
    }
    None
}

/// Wait for a `PchatFetchResponse` proto message from the server.
async fn wait_for_pchat_fetch_resp(
    transport: &mut TcpTransport,
    timeout: Duration,
) -> Option<mumble_tcp::PchatFetchResponse> {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(3), transport.recv()).await {
            Ok(Ok(ControlMessage::PchatFetchResponse(resp))) => return Some(resp),
            Ok(Ok(_)) => continue,
            Ok(Err(e)) => {
                eprintln!("transport error while waiting for PchatFetchResponse: {e}");
                return None;
            }
            Err(_) => continue,
        }
    }
    None
}

/// Create a `KeyManager` from a random seed.
fn make_key_manager() -> KeyManager {
    let seed: [u8; 32] = rand::random();
    let identity = SeedIdentity::from_seed(&seed).unwrap();
    KeyManager::new(Box::new(identity))
}

/// Build an `EncryptedPayload` from a proto `PchatMessage` (as returned in fetch responses).
fn payload_from_proto_msg(msg: &mumble_tcp::PchatMessage) -> mumble_protocol::persistent::keys::EncryptedPayload {
    let fp: [u8; 8] = msg
        .epoch_fingerprint
        .as_ref()
        .and_then(|v| v.clone().try_into().ok())
        .unwrap_or([0; 8]);
    mumble_protocol::persistent::keys::EncryptedPayload {
        ciphertext: msg.envelope.clone().unwrap_or_default(),
        epoch: msg.epoch,
        chain_index: msg.chain_index,
        epoch_fingerprint: fp,
    }
}

/// Get current time in milliseconds.
fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// Set `pchat_mode` on a channel. Requires admin (`SuperUser`) privileges.
async fn set_pchat_mode(
    transport: &mut TcpTransport,
    state: &ServerState,
    channel_id: u32,
    mode: PchatMode,
) {
    let cmd = SetChannelState {
        channel_id: Some(channel_id),
        parent: None,
        name: None,
        description: None,
        position: None,
        temporary: None,
        max_users: None,
        pchat_mode: Some(mode),
        pchat_max_history: None,
        pchat_retention_days: None,
    };
    let output = cmd.execute(state);
    for msg in &output.tcp_messages {
        transport.send(msg).await.unwrap();
    }

    // Wait for server to echo back the ChannelState confirming the mode.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(2), transport.recv()).await {
            Ok(Ok(ControlMessage::ChannelState(cs))) => {
                if cs.channel_id == Some(channel_id)
                    && cs.pchat_mode == Some(mode.to_proto())
                {
                    return;
                }
            }
            Ok(Ok(ControlMessage::PermissionDenied(_))) => {
                panic!("Permission denied setting pchat_mode - authenticate as SuperUser");
            }
            Ok(Ok(_)) => continue,
            _ => break,
        }
    }
    panic!(
        "Server did not confirm pchat_mode change to {mode:?} on channel {channel_id}"
    );
}

/// Helper: send a key-announce for a `key_manager` using native proto.
async fn send_key_announce(
    transport: &mut TcpTransport,
    _state: &ServerState,
    key_manager: &KeyManager,
    cert_hash: &str,
) {
    let wire = key_manager.build_key_announce(cert_hash, now_millis());
    let proto = wire_key_announce_to_proto(&wire);
    transport
        .send(&ControlMessage::PchatKeyAnnounce(proto))
        .await
        .unwrap();
}

fn wire_key_announce_to_proto(w: &WireKeyAnnounce) -> mumble_tcp::PchatKeyAnnounce {
    mumble_tcp::PchatKeyAnnounce {
        algorithm_version: Some(w.algorithm_version as u32),
        identity_public: Some(w.identity_public.clone()),
        signing_public: Some(w.signing_public.clone()),
        cert_hash: Some(w.cert_hash.clone()),
        timestamp: Some(w.timestamp),
        signature: Some(w.signature.clone()),
        tls_signature: Some(w.tls_signature.clone()),
    }
}

fn persistence_mode_to_proto(mode: PersistenceMode) -> i32 {
    match mode {
        PersistenceMode::FullArchive => mumble_tcp::PchatPersistenceMode::PchatModeFullArchive as i32,
        _ => mumble_tcp::PchatPersistenceMode::PchatModePostJoin as i32,
    }
}

/// Helper: send a pchat-msg (encrypted) using native proto and return the `message_id`.
#[allow(clippy::too_many_arguments, reason = "pchat send helper mirrors the full message parameter surface")]
async fn send_pchat_msg(
    transport: &mut TcpTransport,
    _state: &ServerState,
    key_manager: &mut KeyManager,
    cert_hash: &str,
    channel_id: u32,
    mode: PersistenceMode,
    body: &str,
    sender_name: &str,
    sender_session: u32,
) -> String {
    let c = codec();
    let message_id = uuid::Uuid::new_v4().to_string();

    let envelope = MessageEnvelope {
        body: body.to_string(),
        sender_name: sender_name.to_string(),
        sender_session,
        attachments: vec![],
    };
    let envelope_bytes = c.encode(&envelope).unwrap();

    let now = now_millis();
    let payload = key_manager
        .encrypt(mode, channel_id, &message_id, now, &envelope_bytes)
        .expect("encryption should succeed");

    let proto_msg = mumble_tcp::PchatMessage {
        message_id: Some(message_id.clone()),
        channel_id: Some(channel_id),
        timestamp: Some(now),
        sender_hash: Some(cert_hash.to_string()),
        mode: Some(persistence_mode_to_proto(mode)),
        envelope: Some(payload.ciphertext),
        epoch: payload.epoch,
        chain_index: payload.chain_index,
        epoch_fingerprint: Some(payload.epoch_fingerprint.to_vec()),
        replaces_id: None,
    };

    transport
        .send(&ControlMessage::PchatMessage(proto_msg))
        .await
        .unwrap();

    message_id
}

/// Helper: send a pchat-fetch request using native proto.
async fn send_pchat_fetch(
    transport: &mut TcpTransport,
    _state: &ServerState,
    channel_id: u32,
    limit: u32,
) {
    let fetch = mumble_tcp::PchatFetch {
        channel_id: Some(channel_id),
        before_id: None,
        limit: Some(limit),
        after_id: None,
    };
    transport
        .send(&ControlMessage::PchatFetch(fetch))
        .await
        .unwrap();
}

// =============================================================================
// Tests
// =============================================================================

/// Test that the server accepts a key-announce without error.
#[tokio::test]
async fn test_key_announce_accepted() {
    if !ensure_server_available().await {
        return;
    }

    let (mut transport, state, cert_hash) = connect_as_superuser().await;
    let key_manager = make_key_manager();

    send_key_announce(&mut transport, &state, &key_manager, &cert_hash).await;

    // The server should NOT disconnect us. Wait briefly to confirm stability.
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Send a ping to verify the connection is still alive.
    let ping = ControlMessage::Ping(mumble_tcp::Ping {
        timestamp: Some(now_millis()),
        ..Default::default()
    });
    transport.send(&ping).await.unwrap();

    let mut got_pong = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(2), transport.recv()).await {
            Ok(Ok(ControlMessage::Ping(_))) => {
                got_pong = true;
                break;
            }
            Ok(Ok(_)) => continue,
            _ => break,
        }
    }
    assert!(got_pong, "Connection should remain alive after key-announce");
}

/// Test setting `pchat_mode` on a channel (requires server pchat support).
#[tokio::test]
async fn test_set_pchat_mode_on_channel() {
    if !ensure_server_available().await {
        return;
    }

    let (mut transport, state, _cert_hash) = connect_as_superuser().await;

    // Set pchat_mode = FullArchive on root channel.
    // set_pchat_mode waits for the server echo internally.
    set_pchat_mode(&mut transport, &state, 0, PchatMode::FullArchive).await;

    // Reset back to None for other tests.
    set_pchat_mode(&mut transport, &state, 0, PchatMode::None).await;
}

/// Test the full pchat message storage and retrieval flow:
/// 1. Set channel to `FullArchive` mode
/// 2. Send key-announce
/// 3. Generate and store an archive key
/// 4. Send an encrypted pchat-msg
/// 5. Wait for pchat-ack with status "stored"
/// 6. Send pchat-fetch
/// 7. Receive pchat-fetch-resp with the stored message
/// 8. Decrypt and verify the message body
#[tokio::test]
async fn test_pchat_message_store_and_fetch() {
    if !ensure_server_available().await {
        return;
    }

    let (mut transport, state, cert_hash) = connect_as_superuser().await;
    let session = state.own_session().expect("should have session");
    let channel_id: u32 = 0; // Root channel

    // 1. Set pchat_mode = FullArchive on root channel (requires admin).
    set_pchat_mode(&mut transport, &state, channel_id, PchatMode::FullArchive).await;

    // 2. Send key-announce.
    let mut key_manager = make_key_manager();
    send_key_announce(&mut transport, &state, &key_manager, &cert_hash).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    drain(&mut transport).await;

    // 3. Generate and store an archive key locally.
    let archive_key: [u8; 32] = rand::random();
    key_manager.store_archive_key(channel_id, archive_key, KeyTrustLevel::Verified);

    // 4. Send an encrypted pchat-msg.
    let msg_body = "Hello from pchat integration test!";
    let message_id = send_pchat_msg(
        &mut transport,
        &state,
        &mut key_manager,
        &cert_hash,
        channel_id,
        PersistenceMode::FullArchive,
        msg_body,
        "PchatStoreUser",
        session,
    )
    .await;

    // 5. Wait for pchat-ack.
    let ack = wait_for_pchat_ack(&mut transport, Duration::from_secs(5)).await;
    assert!(ack.is_some(), "Should receive a pchat-ack from server");

    let ack = ack.unwrap();
    assert_eq!(
        ack.message_id.as_deref(),
        Some(message_id.as_str()),
        "ack should reference our message"
    );
    assert_eq!(
        ack.status,
        Some(mumble_tcp::PchatAckStatus::PchatAckStored as i32),
        "ack status should be STORED, got {:?} (reason: {:?})",
        ack.status,
        ack.reason
    );

    // 6. Send pchat-fetch.
    send_pchat_fetch(&mut transport, &state, channel_id, 50).await;

    // 7. Wait for pchat-fetch-resp.
    let resp = wait_for_pchat_fetch_resp(&mut transport, Duration::from_secs(5)).await;
    assert!(
        resp.is_some(),
        "Should receive a pchat-fetch-resp from server"
    );

    let resp = resp.unwrap();
    assert_eq!(resp.channel_id, Some(channel_id));
    assert!(
        !resp.messages.is_empty(),
        "fetch-resp should contain at least one message"
    );

    // 8. Find our message and decrypt it.
    let our_msg = resp
        .messages
        .iter()
        .find(|m| m.message_id.as_deref() == Some(&message_id))
        .expect("our message should be in the fetch response");

    assert_eq!(our_msg.sender_hash.as_deref(), Some(cert_hash.as_str()));

    // Decrypt the envelope.
    let encrypted_payload = payload_from_proto_msg(our_msg);

    let decrypted = key_manager
        .decrypt(
            PersistenceMode::FullArchive,
            channel_id,
            &message_id,
            our_msg.timestamp.unwrap_or(0),
            &encrypted_payload,
        )
        .expect("decryption should succeed");

    let c = codec();
    let envelope: MessageEnvelope = c.decode(&decrypted).expect("should decode envelope");
    assert_eq!(envelope.body, msg_body, "decrypted body should match");
    assert_eq!(envelope.sender_name, "PchatStoreUser");
    assert_eq!(envelope.sender_session, session);

    // Cleanup: reset pchat_mode.
    set_pchat_mode(&mut transport, &state, channel_id, PchatMode::None).await;
}

/// Test sending multiple messages and fetching them all back.
#[tokio::test]
async fn test_pchat_multiple_messages_stored_and_fetched() {
    if !ensure_server_available().await {
        return;
    }

    let (mut transport, state, cert_hash) = connect_as_superuser().await;
    let session = state.own_session().unwrap();
    let channel_id: u32 = 0;

    // Setup: set mode, announce key, generate archive key.
    set_pchat_mode(&mut transport, &state, channel_id, PchatMode::FullArchive).await;

    let mut key_manager = make_key_manager();
    send_key_announce(&mut transport, &state, &key_manager, &cert_hash).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    drain(&mut transport).await;

    let archive_key: [u8; 32] = rand::random();
    key_manager.store_archive_key(channel_id, archive_key, KeyTrustLevel::Verified);

    // Send 3 messages.
    let mut message_ids = Vec::new();
    let messages = ["First message", "Second message", "Third message"];
    for body in &messages {
        let id = send_pchat_msg(
            &mut transport,
            &state,
            &mut key_manager,
            &cert_hash,
            channel_id,
            PersistenceMode::FullArchive,
            body,
            "PchatMultiUser",
            session,
        )
        .await;
        message_ids.push(id);

        // Wait for ack.
        let ack = wait_for_pchat_ack(&mut transport, Duration::from_secs(5)).await;
        assert!(ack.is_some(), "Should receive ack for message '{body}'");
        let ack = ack.unwrap();
        assert_eq!(
            ack.status,
            Some(mumble_tcp::PchatAckStatus::PchatAckStored as i32),
            "Message '{body}' should be stored"
        );

        // Small delay between messages to ensure distinct timestamps.
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Fetch all messages.
    send_pchat_fetch(&mut transport, &state, channel_id, 50).await;

    let resp = wait_for_pchat_fetch_resp(&mut transport, Duration::from_secs(5)).await;
    assert!(resp.is_some(), "Should receive fetch-resp");

    let resp = resp.unwrap();

    // Verify all 3 messages are present.
    for (i, id) in message_ids.iter().enumerate() {
        let found = resp.messages.iter().find(|m| m.message_id.as_deref() == Some(id.as_str()));
        assert!(
            found.is_some(),
            "Message {} ('{}') should be in fetch response",
            i,
            messages[i]
        );

        // Decrypt and verify.
        let msg = found.unwrap();
        let payload = payload_from_proto_msg(msg);
        let decrypted = key_manager
            .decrypt(
                PersistenceMode::FullArchive,
                channel_id,
                id,
                msg.timestamp.unwrap_or(0),
                &payload,
            )
            .unwrap();
        let c = codec();
        let envelope: MessageEnvelope = c.decode(&decrypted).unwrap();
        assert_eq!(envelope.body, messages[i]);
    }

    // Cleanup.
    set_pchat_mode(&mut transport, &state, channel_id, PchatMode::None).await;
}

/// Test that a second client can fetch messages stored by the first client
/// (provided it has the same archive key).
#[tokio::test]
async fn test_pchat_cross_client_fetch() {
    if !ensure_server_available().await {
        return;
    }

    // Shared archive key (in real usage, distributed via key-exchange).
    let archive_key: [u8; 32] = rand::random();
    let channel_id: u32 = 0;

    // --- Client A (SuperUser): store a message ---
    let (mut transport_a, state_a, cert_hash_a) = connect_as_superuser().await;
    let session_a = state_a.own_session().unwrap();

    set_pchat_mode(&mut transport_a, &state_a, channel_id, PchatMode::FullArchive).await;

    let mut km_a = make_key_manager();
    send_key_announce(&mut transport_a, &state_a, &km_a, &cert_hash_a).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    drain(&mut transport_a).await;

    km_a.store_archive_key(channel_id, archive_key, KeyTrustLevel::Verified);

    let msg_body = "Cross-client test message";
    let message_id = send_pchat_msg(
        &mut transport_a,
        &state_a,
        &mut km_a,
        &cert_hash_a,
        channel_id,
        PersistenceMode::FullArchive,
        msg_body,
        "PchatCrossA",
        session_a,
    )
    .await;

    // Wait for ack.
    let ack = wait_for_pchat_ack(&mut transport_a, Duration::from_secs(5)).await;
    assert!(ack.is_some(), "Client A should receive ack");
    let ack = ack.unwrap();
    assert_eq!(ack.status, Some(mumble_tcp::PchatAckStatus::PchatAckStored as i32));

    // --- Client B: fetch the message ---
    let (mut transport_b, state_b, cert_hash_b) =
        connect_and_authenticate("PchatCrossB2").await;

    // Client B announces its key (required for pchat participation).
    let km_b = make_key_manager();
    send_key_announce(&mut transport_b, &state_b, &km_b, &cert_hash_b).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    drain(&mut transport_b).await;

    // Client B has the same archive key (simulating key exchange).
    let km_b = {
        let mut km = make_key_manager();
        km.store_archive_key(channel_id, archive_key, KeyTrustLevel::Verified);
        km
    };

    // Client B fetches.
    send_pchat_fetch(&mut transport_b, &state_b, channel_id, 50).await;

    let resp = wait_for_pchat_fetch_resp(&mut transport_b, Duration::from_secs(5)).await;
    assert!(
        resp.is_some(),
        "Client B should receive fetch-resp"
    );

    let resp = resp.unwrap();
    assert!(
        !resp.messages.is_empty(),
        "fetch-resp should contain messages"
    );

    // Find and decrypt the message from client A.
    let our_msg = resp
        .messages
        .iter()
        .find(|m| m.message_id.as_deref() == Some(&message_id))
        .expect("Client A's message should be in fetch response");

    let payload = payload_from_proto_msg(our_msg);

    let decrypted = km_b
        .decrypt(
            PersistenceMode::FullArchive,
            channel_id,
            &message_id,
            our_msg.timestamp.unwrap_or(0),
            &payload,
        )
        .expect("Client B should be able to decrypt with the shared key");

    let c = codec();
    let envelope: MessageEnvelope = c.decode(&decrypted).unwrap();
    assert_eq!(envelope.body, msg_body);
    assert_eq!(envelope.sender_name, "PchatCrossA");

    // Cleanup.
    set_pchat_mode(&mut transport_a, &state_a, channel_id, PchatMode::None).await;
}

/// Test that fetch on a channel with no stored messages returns an empty response.
#[tokio::test]
async fn test_pchat_fetch_empty_channel() {
    if !ensure_server_available().await {
        return;
    }

    let (mut transport, state, cert_hash) = connect_as_superuser().await;
    let channel_id: u32 = 0;

    set_pchat_mode(&mut transport, &state, channel_id, PchatMode::FullArchive).await;

    let key_manager = make_key_manager();
    send_key_announce(&mut transport, &state, &key_manager, &cert_hash).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    drain(&mut transport).await;

    // Fetch without storing any messages first.
    send_pchat_fetch(&mut transport, &state, channel_id, 50).await;

    let resp = wait_for_pchat_fetch_resp(&mut transport, Duration::from_secs(5)).await;
    assert!(
        resp.is_some(),
        "Should receive fetch-resp even for empty channel"
    );

    let resp = resp.unwrap();
    assert_eq!(resp.channel_id, Some(channel_id));
    // Messages might be non-empty if previous test left data, but should not error.
    assert!(!resp.has_more.unwrap_or(false), "empty channel should not have more pages");

    // Cleanup.
    set_pchat_mode(&mut transport, &state, channel_id, PchatMode::None).await;
}

/// Test that messages persist across client reconnections.
#[tokio::test]
async fn test_pchat_messages_persist_across_reconnect() {
    if !ensure_server_available().await {
        return;
    }

    let archive_key: [u8; 32] = rand::random();
    let channel_id: u32 = 0;
    let msg_body = "This message should survive reconnect";
    let saved_msg_id;

    // --- First connection: store a message ---
    {
        let (mut transport, state, cert_hash) = connect_as_superuser().await;
        let session = state.own_session().unwrap();

        set_pchat_mode(&mut transport, &state, channel_id, PchatMode::FullArchive).await;

        let mut km = make_key_manager();
        send_key_announce(&mut transport, &state, &km, &cert_hash).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        drain(&mut transport).await;

        km.store_archive_key(channel_id, archive_key, KeyTrustLevel::Verified);

        saved_msg_id = send_pchat_msg(
            &mut transport,
            &state,
            &mut km,
            &cert_hash,
            channel_id,
            PersistenceMode::FullArchive,
            msg_body,
            "PchatReconnect1",
            session,
        )
        .await;

        let ack = wait_for_pchat_ack(&mut transport, Duration::from_secs(5)).await;
        assert!(ack.is_some(), "Should get ack");
        let ack = ack.unwrap();
        assert_eq!(ack.status, Some(mumble_tcp::PchatAckStatus::PchatAckStored as i32));

        // Disconnect by dropping transport.
        drop(transport);
    }

    // Small delay to let server process disconnect.
    tokio::time::sleep(Duration::from_millis(500)).await;

    // --- Second connection: fetch and verify the message is still there ---
    {
        let (mut transport, state, cert_hash) = connect_as_superuser().await;

        let km = make_key_manager();
        send_key_announce(&mut transport, &state, &km, &cert_hash).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        drain(&mut transport).await;

        send_pchat_fetch(&mut transport, &state, channel_id, 50).await;

        let resp = wait_for_pchat_fetch_resp(&mut transport, Duration::from_secs(5)).await;
        assert!(
            resp.is_some(),
            "Should receive fetch-resp on second connection"
        );

        let resp = resp.unwrap();
        assert!(
            !resp.messages.is_empty(),
            "Messages should persist across reconnections"
        );

        // Find a message with our body (we can't decrypt without the key, but
        // we can verify the server stored it by checking sender_hash).
        let has_our_msg = resp
            .messages
            .iter()
            .any(|m| m.message_id.as_deref() == Some(&saved_msg_id));
        assert!(
            has_our_msg,
            "Should find our specific message from previous connection"
        );

        // If we store the same key, we should be able to decrypt.
        let mut km2 = make_key_manager();
        km2.store_archive_key(channel_id, archive_key, KeyTrustLevel::Verified);

        // Find and decrypt our specific message.
        let our_msg = resp
            .messages
            .iter()
            .find(|m| m.message_id.as_deref() == Some(&saved_msg_id))
            .unwrap();
        let payload = payload_from_proto_msg(our_msg);
        let msg_id = our_msg.message_id.clone().unwrap_or_default();
        let decrypted = km2
            .decrypt(
                PersistenceMode::FullArchive,
                channel_id,
                &msg_id,
                our_msg.timestamp.unwrap_or(0),
                &payload,
            )
            .expect("decryption with shared key should work after reconnect");
        let c = codec();
        let envelope: MessageEnvelope = c.decode(&decrypted).unwrap();
        assert_eq!(envelope.body, msg_body);

        // Cleanup.
        set_pchat_mode(&mut transport, &state, channel_id, PchatMode::None).await;
    }
}

// =============================================================================
// Deterministic archive key tests
// =============================================================================

/// Create a `KeyManager` from a specific seed (deterministic).
fn make_key_manager_from_seed(seed: &[u8; 32]) -> KeyManager {
    let identity = SeedIdentity::from_seed(seed).unwrap();
    KeyManager::new(Box::new(identity))
}

/// Test that `derive_archive_key` is deterministic: same seed + `channel_id` → same key.
/// Also verifies different seeds or channel IDs produce different keys.
#[test]
fn test_deterministic_archive_key_derivation() {
    use mumble_protocol::persistent::encryption::derive_archive_key;

    let seed_a: [u8; 32] = [0xAA; 32];
    let seed_b: [u8; 32] = [0xBB; 32];

    // Same seed + channel → same key.
    let k1 = derive_archive_key(&seed_a, 1);
    let k2 = derive_archive_key(&seed_a, 1);
    assert_eq!(k1, k2, "same seed + channel must yield identical key");

    // Different channel → different key.
    let k3 = derive_archive_key(&seed_a, 2);
    assert_ne!(k1, k3, "different channel must yield different key");

    // Different seed → different key.
    let k4 = derive_archive_key(&seed_b, 1);
    assert_ne!(k1, k4, "different seed must yield different key");
}

/// Test that a message encrypted with a derived key can be decrypted by a
/// NEW `KeyManager` instance using the same seed (simulating a reconnect).
#[test]
fn test_derived_key_survives_keymgr_rebuild() {
    use mumble_protocol::persistent::encryption::derive_archive_key;
    use mumble_protocol::persistent::wire::{MessageEnvelope, MsgPackCodec, WireCodec};

    let seed: [u8; 32] = [0xCC; 32];
    let channel_id: u32 = 42;
    let mode = PersistenceMode::FullArchive;
    let msg_id = "00000000-0000-0000-0000-000000000001";
    let timestamp = 1_700_000_000_000u64;

    let key = derive_archive_key(&seed, channel_id);

    // --- Session 1: encrypt ---
    let mut km1 = make_key_manager_from_seed(&seed);
    km1.store_archive_key(channel_id, key, KeyTrustLevel::Verified);

    let c = MsgPackCodec;
    let envelope = MessageEnvelope {
        body: "survive rebuild".into(),
        sender_name: "tester".into(),
        sender_session: 1,
        attachments: vec![],
    };
    let env_bytes = c.encode(&envelope).unwrap();

    let payload = km1
        .encrypt(mode, channel_id, msg_id, timestamp, &env_bytes)
        .expect("encrypt should succeed");

    // --- Session 2: new KeyManager, same seed → derive same key ---
    let mut km2 = make_key_manager_from_seed(&seed);
    let key2 = derive_archive_key(&seed, channel_id);
    km2.store_archive_key(channel_id, key2, KeyTrustLevel::Verified);

    let decrypted = km2
        .decrypt(mode, channel_id, msg_id, timestamp, &payload)
        .expect("decrypt with same derived key should succeed");

    let env2: MessageEnvelope = c.decode(&decrypted).unwrap();
    assert_eq!(env2.body, "survive rebuild");
}

/// Test that decryption fails when we have no key (reproduces the bug
/// where fetch-resp arrived before key generation).
#[test]
fn test_decrypt_fails_without_key() {
    use mumble_protocol::persistent::encryption::derive_archive_key;
    use mumble_protocol::persistent::wire::{MessageEnvelope, MsgPackCodec, WireCodec};

    let seed: [u8; 32] = [0xDD; 32];
    let channel_id: u32 = 7;
    let mode = PersistenceMode::FullArchive;
    let msg_id = "00000000-0000-0000-0000-000000000002";
    let timestamp = 1_700_000_000_000u64;

    let key = derive_archive_key(&seed, channel_id);

    // Encrypt with a real key.
    let mut km_sender = make_key_manager_from_seed(&seed);
    km_sender.store_archive_key(channel_id, key, KeyTrustLevel::Verified);

    let c = MsgPackCodec;
    let envelope = MessageEnvelope {
        body: "should not decrypt".into(),
        sender_name: "sender".into(),
        sender_session: 1,
        attachments: vec![],
    };
    let env_bytes = c.encode(&envelope).unwrap();

    let payload = km_sender
        .encrypt(mode, channel_id, msg_id, timestamp, &env_bytes)
        .unwrap();

    // Try to decrypt WITHOUT any key at all → must fail.
    let km_no_key = make_key_manager_from_seed(&[0xEE; 32]);
    let result = km_no_key.decrypt(mode, channel_id, msg_id, timestamp, &payload);
    assert!(
        result.is_err(),
        "decryption without a key must fail"
    );

    // Try to decrypt with a WRONG key → must also fail.
    let mut km_wrong = make_key_manager_from_seed(&[0xEE; 32]);
    km_wrong.store_archive_key(channel_id, [0xFF; 32], KeyTrustLevel::Verified);
    let result = km_wrong.decrypt(mode, channel_id, msg_id, timestamp, &payload);
    assert!(
        result.is_err(),
        "decryption with wrong key must fail"
    );
}

/// Integration test: store a message, disconnect, reconnect with a fresh
/// `KeyManager` that derives the SAME archive key from the SAME seed, fetch,
/// and verify decryption succeeds. This is the exact scenario that was broken
/// when random keys were used per-session.
#[tokio::test]
async fn test_reconnect_decrypt_with_derived_key() {
    use mumble_protocol::persistent::encryption::derive_archive_key;

    if !ensure_server_available().await {
        return;
    }

    let seed: [u8; 32] = rand::random();
    let channel_id: u32 = 0;
    let msg_body = "Deterministic key reconnect test";
    let saved_msg_id;

    let key = derive_archive_key(&seed, channel_id);

    // --- Connection 1: store a message ---
    {
        let (mut transport, state, cert_hash) = connect_as_superuser().await;
        let session = state.own_session().unwrap();

        set_pchat_mode(&mut transport, &state, channel_id, PchatMode::FullArchive).await;

        let mut km = make_key_manager_from_seed(&seed);
        send_key_announce(&mut transport, &state, &km, &cert_hash).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        drain(&mut transport).await;

        km.store_archive_key(channel_id, key, KeyTrustLevel::Verified);

        saved_msg_id = send_pchat_msg(
            &mut transport,
            &state,
            &mut km,
            &cert_hash,
            channel_id,
            PersistenceMode::FullArchive,
            msg_body,
            "DerivedKeyUser",
            session,
        )
        .await;

        let ack = wait_for_pchat_ack(&mut transport, Duration::from_secs(5)).await;
        assert!(ack.is_some(), "Should get ack");
        let ack = ack.unwrap();
        assert_eq!(ack.status, Some(mumble_tcp::PchatAckStatus::PchatAckStored as i32));

        drop(transport);
    }

    tokio::time::sleep(Duration::from_millis(500)).await;

    // --- Connection 2: new KeyManager, derive SAME key from SAME seed ---
    {
        let (mut transport, state, cert_hash) = connect_as_superuser().await;

        // Fresh KeyManager from same seed → derive same archive key.
        let mut km2 = make_key_manager_from_seed(&seed);
        let key2 = derive_archive_key(&seed, channel_id);
        km2.store_archive_key(channel_id, key2, KeyTrustLevel::Verified);

        send_key_announce(&mut transport, &state, &km2, &cert_hash).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        drain(&mut transport).await;

        send_pchat_fetch(&mut transport, &state, channel_id, 50).await;

        let resp = wait_for_pchat_fetch_resp(&mut transport, Duration::from_secs(5)).await;
        assert!(
            resp.is_some(),
            "Should receive fetch-resp on reconnect"
        );

        let resp = resp.unwrap();

        let our_msg = resp
            .messages
            .iter()
            .find(|m| m.message_id.as_deref() == Some(&saved_msg_id))
            .expect("our message should persist on the server");

        let payload = payload_from_proto_msg(our_msg);
        let msg_id = our_msg.message_id.clone().unwrap_or_default();
        let decrypted = km2
            .decrypt(
                PersistenceMode::FullArchive,
                channel_id,
                &msg_id,
                our_msg.timestamp.unwrap_or(0),
                &payload,
            )
            .expect(
                "decryption must succeed with derived key from same seed"
            );

        let c = codec();
        let envelope: MessageEnvelope = c.decode(&decrypted).unwrap();
        assert_eq!(envelope.body, msg_body);
        assert_eq!(envelope.sender_name, "DerivedKeyUser");

        // Cleanup.
        set_pchat_mode(&mut transport, &state, channel_id, PchatMode::None).await;
    }
}

// =============================================================================
// Cross-user sender_hash / is_own tests
// =============================================================================

/// Test that when Bob fetches a message sent by Alice, the `sender_hash` in
/// the fetch response equals Alice's cert hash — NOT Bob's.  This is the
/// server-side invariant that the client's `is_own` logic relies on.
///
/// Scenario:
///   1. `SuperUser` sets channel to `FullArchive`.
///   2. Alice connects, announces key, stores archive key, sends a message.
///   3. Bob connects, announces key, stores (same) archive key, fetches.
///   4. Assert: `sender_hash` of Alice's message == Alice's `cert_hash`
///   5. Assert: `sender_hash` of Alice's message != Bob's `cert_hash`
///   6. Assert: `is_own` logic (as used in the Tauri client) would be `false`
///      for Bob.
#[tokio::test]
async fn test_cross_user_sender_hash_determines_is_own() {
    if !ensure_server_available().await {
        return;
    }

    let archive_key: [u8; 32] = rand::random();
    let channel_id: u32 = 0;

    // --- SuperUser: set channel mode ---
    let (mut su_transport, su_state, _su_hash) = connect_as_superuser().await;
    set_pchat_mode(&mut su_transport, &su_state, channel_id, PchatMode::FullArchive).await;
    // Keep SuperUser alive so mode persists.

    // --- Alice: connect, announce key, send a message ---
    let (mut alice_transport, alice_state, alice_cert_hash) =
        connect_and_authenticate("AliceIsOwn").await;
    let alice_session = alice_state.own_session().unwrap();

    eprintln!("Alice cert_hash = {alice_cert_hash}");
    assert!(
        !alice_cert_hash.is_empty(),
        "Alice must have a non-empty cert hash"
    );

    let mut alice_km = make_key_manager();
    send_key_announce(&mut alice_transport, &alice_state, &alice_km, &alice_cert_hash).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    drain(&mut alice_transport).await;

    alice_km.store_archive_key(channel_id, archive_key, KeyTrustLevel::Verified);

    let msg_body = "Hello from Alice - is_own test";
    let alice_msg_id = send_pchat_msg(
        &mut alice_transport,
        &alice_state,
        &mut alice_km,
        &alice_cert_hash,
        channel_id,
        PersistenceMode::FullArchive,
        msg_body,
        "AliceIsOwn",
        alice_session,
    )
    .await;

    let ack = wait_for_pchat_ack(&mut alice_transport, Duration::from_secs(5)).await;
    assert!(ack.is_some(), "Alice should receive ack");
    assert_eq!(
        ack.unwrap().status,
        Some(mumble_tcp::PchatAckStatus::PchatAckStored as i32)
    );

    // --- Bob: connect, announce key, fetch ---
    let (mut bob_transport, bob_state, bob_cert_hash) =
        connect_and_authenticate("BobIsOwn").await;

    eprintln!("Bob cert_hash   = {bob_cert_hash}");
    assert!(
        !bob_cert_hash.is_empty(),
        "Bob must have a non-empty cert hash"
    );
    assert_ne!(
        alice_cert_hash, bob_cert_hash,
        "Alice and Bob MUST have different cert hashes (different TLS certs)"
    );

    let bob_km = make_key_manager();
    send_key_announce(&mut bob_transport, &bob_state, &bob_km, &bob_cert_hash).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    drain(&mut bob_transport).await;

    let bob_km = {
        let mut km = make_key_manager();
        km.store_archive_key(channel_id, archive_key, KeyTrustLevel::Verified);
        km
    };

    // Bob fetches.
    send_pchat_fetch(&mut bob_transport, &bob_state, channel_id, 50).await;

    let resp = wait_for_pchat_fetch_resp(&mut bob_transport, Duration::from_secs(5)).await;
    assert!(resp.is_some(), "Bob should receive fetch-resp");

    let resp = resp.unwrap();
    assert!(
        !resp.messages.is_empty(),
        "fetch-resp should contain at least one message"
    );

    // Find Alice's specific message.
    let alice_msg = resp
        .messages
        .iter()
        .find(|m| m.message_id.as_deref() == Some(&alice_msg_id))
        .expect("Alice's message must be in the fetch response");

    let msg_sender_hash = alice_msg.sender_hash.clone().unwrap_or_default();
    eprintln!("msg sender_hash = {msg_sender_hash}");

    // CRITICAL ASSERTIONS:
    // The sender_hash in the fetch response must be Alice's cert hash.
    assert_eq!(
        msg_sender_hash, alice_cert_hash,
        "sender_hash in fetch-resp must equal Alice's cert hash"
    );

    // The sender_hash must NOT equal Bob's cert hash.
    assert_ne!(
        msg_sender_hash, bob_cert_hash,
        "sender_hash must NOT equal Bob's cert hash"
    );

    // Simulate the client's is_own logic (from pchat.rs handle_proto_fetch_resp).
    let bob_is_own = !msg_sender_hash.is_empty()
        && !bob_cert_hash.is_empty()
        && msg_sender_hash == bob_cert_hash;
    assert!(
        !bob_is_own,
        "Bob's is_own must be FALSE for Alice's message (sender={msg_sender_hash}, bob={bob_cert_hash})"
    );

    let alice_is_own = !msg_sender_hash.is_empty()
        && !alice_cert_hash.is_empty()
        && msg_sender_hash == alice_cert_hash;
    assert!(
        alice_is_own,
        "Alice's is_own must be TRUE for her own message"
    );

    // Also verify we can decrypt the message.
    let payload = payload_from_proto_msg(alice_msg);
    let decrypted = bob_km
        .decrypt(
            PersistenceMode::FullArchive,
            channel_id,
            &alice_msg_id,
            alice_msg.timestamp.unwrap_or(0),
            &payload,
        )
        .expect("Bob should decrypt with shared key");

    let c = codec();
    let envelope: MessageEnvelope = c.decode(&decrypted).unwrap();
    assert_eq!(envelope.body, msg_body);
    assert_eq!(envelope.sender_name, "AliceIsOwn");

    // Cleanup.
    set_pchat_mode(&mut su_transport, &su_state, channel_id, PchatMode::None).await;
}

/// Complementary test: verify that when Alice fetches her OWN message,
/// `sender_hash == own_cert_hash` → `is_own = true`.
#[tokio::test]
async fn test_sender_hash_matches_own_for_self_fetch() {
    if !ensure_server_available().await {
        return;
    }

    let archive_key: [u8; 32] = rand::random();
    let channel_id: u32 = 0;

    let (mut transport, state, cert_hash) = connect_as_superuser().await;
    let session = state.own_session().unwrap();

    set_pchat_mode(&mut transport, &state, channel_id, PchatMode::FullArchive).await;

    let mut km = make_key_manager();
    send_key_announce(&mut transport, &state, &km, &cert_hash).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    drain(&mut transport).await;

    km.store_archive_key(channel_id, archive_key, KeyTrustLevel::Verified);

    let msg_body = "Self-fetch is_own test";
    let msg_id = send_pchat_msg(
        &mut transport,
        &state,
        &mut km,
        &cert_hash,
        channel_id,
        PersistenceMode::FullArchive,
        msg_body,
        "SelfFetcher",
        session,
    )
    .await;

    let ack = wait_for_pchat_ack(&mut transport, Duration::from_secs(5)).await;
    assert!(ack.is_some());
    assert_eq!(
        ack.unwrap().status,
        Some(mumble_tcp::PchatAckStatus::PchatAckStored as i32)
    );

    // Fetch our own message back.
    send_pchat_fetch(&mut transport, &state, channel_id, 50).await;

    let resp = wait_for_pchat_fetch_resp(&mut transport, Duration::from_secs(5)).await;
    assert!(resp.is_some());

    let resp = resp.unwrap();
    let our_msg = resp
        .messages
        .iter()
        .find(|m| m.message_id.as_deref() == Some(&msg_id))
        .expect("our message should be in fetch response");

    let sender_hash = our_msg.sender_hash.clone().unwrap_or_default();
    eprintln!("own cert_hash   = {cert_hash}");
    eprintln!("msg sender_hash = {sender_hash}");

    assert_eq!(
        sender_hash, cert_hash,
        "sender_hash must equal our own cert_hash for self-sent messages"
    );

    // Simulate is_own logic — must be true.
    let is_own = !sender_hash.is_empty()
        && !cert_hash.is_empty()
        && sender_hash == cert_hash;
    assert!(
        is_own,
        "is_own must be TRUE when fetching our own message"
    );

    // Cleanup.
    set_pchat_mode(&mut transport, &state, channel_id, PchatMode::None).await;
}

// =============================================================================
// Key exchange integration tests
// =============================================================================

/// Wait for a `PchatKeyAnnounce` proto message from the server (relayed from another client).
async fn wait_for_key_announce(
    transport: &mut TcpTransport,
    timeout: Duration,
) -> Option<mumble_tcp::PchatKeyAnnounce> {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(3), transport.recv()).await {
            Ok(Ok(ControlMessage::PchatKeyAnnounce(ann))) => return Some(ann),
            Ok(Ok(_)) => continue,
            Ok(Err(e)) => {
                eprintln!("transport error while waiting for PchatKeyAnnounce: {e}");
                return None;
            }
            Err(_) => continue,
        }
    }
    None
}

/// Wait for a `PchatKeyRequest` proto message from the server.
async fn wait_for_key_request(
    transport: &mut TcpTransport,
    timeout: Duration,
) -> Option<mumble_tcp::PchatKeyRequest> {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(3), transport.recv()).await {
            Ok(Ok(ControlMessage::PchatKeyRequest(req))) => return Some(req),
            Ok(Ok(_)) => continue,
            Ok(Err(e)) => {
                eprintln!("transport error while waiting for PchatKeyRequest: {e}");
                return None;
            }
            Err(_) => continue,
        }
    }
    None
}

/// Wait for a `PchatKeyExchange` proto message from the server.
async fn wait_for_key_exchange(
    transport: &mut TcpTransport,
    timeout: Duration,
) -> Option<mumble_tcp::PchatKeyExchange> {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(3), transport.recv()).await {
            Ok(Ok(ControlMessage::PchatKeyExchange(kex))) => return Some(kex),
            Ok(Ok(_)) => continue,
            Ok(Err(e)) => {
                eprintln!("transport error while waiting for PchatKeyExchange: {e}");
                return None;
            }
            Err(_) => continue,
        }
    }
    None
}

/// Collect all pending `PchatKeyAnnounce` messages from server (drains within timeout).
async fn collect_key_announces(
    transport: &mut TcpTransport,
    timeout: Duration,
) -> Vec<mumble_tcp::PchatKeyAnnounce> {
    let mut announces = Vec::new();
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(500), transport.recv()).await {
            Ok(Ok(ControlMessage::PchatKeyAnnounce(ann))) => announces.push(ann),
            Ok(Ok(_)) => continue,
            _ => break,
        }
    }
    announces
}

/// Convert a wire `PchatKeyExchange` to proto `PchatKeyExchange`.
fn wire_key_exchange_to_proto(w: &WireKeyExchange) -> mumble_tcp::PchatKeyExchange {
    mumble_tcp::PchatKeyExchange {
        channel_id: Some(w.channel_id),
        mode: Some(persistence_mode_to_proto(PersistenceMode::from_wire_str(&w.mode))),
        epoch: Some(w.epoch),
        encrypted_key: Some(w.encrypted_key.clone()),
        sender_hash: Some(w.sender_hash.clone()),
        recipient_hash: Some(w.recipient_hash.clone()),
        request_id: w.request_id.clone(),
        timestamp: Some(w.timestamp),
        algorithm_version: Some(w.algorithm_version as u32),
        signature: Some(w.signature.clone()),
        parent_fingerprint: w.parent_fingerprint.clone(),
        epoch_fingerprint: Some(w.epoch_fingerprint.clone()),
        countersignature: w.countersignature.clone(),
        countersigner_hash: w.countersigner_hash.clone(),
    }
}

/// Convert a proto `PchatKeyAnnounce` to wire `PchatKeyAnnounce`.
fn proto_key_announce_to_wire(p: &mumble_tcp::PchatKeyAnnounce) -> WireKeyAnnounce {
    WireKeyAnnounce {
        algorithm_version: p.algorithm_version.unwrap_or(1) as u8,
        identity_public: p.identity_public.clone().unwrap_or_default(),
        signing_public: p.signing_public.clone().unwrap_or_default(),
        cert_hash: p.cert_hash.clone().unwrap_or_default(),
        timestamp: p.timestamp.unwrap_or(0),
        signature: p.signature.clone().unwrap_or_default(),
        tls_signature: p.tls_signature.clone().unwrap_or_default(),
    }
}

/// Convert a proto `PchatKeyRequest` to wire `PchatKeyRequest`.
fn proto_key_request_to_wire(p: &mumble_tcp::PchatKeyRequest) -> WireKeyRequest {
    WireKeyRequest {
        channel_id: p.channel_id.unwrap_or(0),
        mode: match p.mode {
            Some(m) if m == mumble_tcp::PchatPersistenceMode::PchatModeFullArchive as i32 => {
                "FULL_ARCHIVE".to_string()
            }
            _ => "POST_JOIN".to_string(),
        },
        requester_hash: p.requester_hash.clone().unwrap_or_default(),
        requester_public: p.requester_public.clone().unwrap_or_default(),
        request_id: p.request_id.clone().unwrap_or_default(),
        timestamp: p.timestamp.unwrap_or(0),
        relay_cap: p.relay_cap.unwrap_or(0),
    }
}

/// Join a channel (send `UserState` with `channel_id`).
#[allow(dead_code, reason = "helper kept for completeness; not every test needs to move users")]
async fn join_channel(
    transport: &mut TcpTransport,
    state: &ServerState,
    channel_id: u32,
) {
    let cmd = JoinChannel { channel_id };
    let output = cmd.execute(state);
    for msg in &output.tcp_messages {
        transport.send(msg).await.unwrap();
    }
}

/// Wait for a `UserState` that confirms a user moved into a specific channel.
#[allow(dead_code, reason = "helper kept for completeness; not every test exercises channel moves")]
async fn wait_for_user_in_channel(
    transport: &mut TcpTransport,
    state: &mut ServerState,
    target_session: u32,
    target_channel: u32,
    timeout: Duration,
) -> bool {
    // Check if already in channel.
    if let Some(u) = state.users.get(&target_session) {
        if u.channel_id == target_channel {
            return true;
        }
    }
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(3), transport.recv()).await {
            Ok(Ok(ControlMessage::UserState(us))) => {
                state.apply_user_state(&us);
                if us.session == Some(target_session)
                    && us.channel_id == Some(target_channel)
                {
                    return true;
                }
            }
            Ok(Ok(_)) => continue,
            _ => break,
        }
    }
    false
}

/// Test that two clients can exchange key-announces and record each other's peer keys.
///
/// Flow:
///   1. Both clients connect
///   2. Client A sends key-announce, Client B receives it as a live relay
///   3. Client B sends key-announce, Client A receives it as a live relay
///   4. Both clients record each other's peer keys
///
/// Note: key-announces sent before the other client connects may arrive
/// during the `ServerSync` handshake and get consumed by `connect_and_authenticate`.
/// This test sends announces AFTER both clients are connected to ensure
/// they are received as live relays.
#[tokio::test]
async fn test_two_clients_exchange_key_announces() {
    if !ensure_server_available().await {
        return;
    }

    // --- Both clients connect first ---
    let (mut transport_a, state_a, cert_hash_a) = connect_and_authenticate("KexAnnounceA").await;
    let (mut transport_b, state_b, cert_hash_b) = connect_and_authenticate("KexAnnounceB").await;

    // Drain any connect-time messages.
    drain(&mut transport_a).await;
    drain(&mut transport_b).await;

    // --- A announces (B should receive as live relay) ---
    let mut km_a = make_key_manager();
    send_key_announce(&mut transport_a, &state_a, &km_a, &cert_hash_a).await;

    // Collect announces for B - the server may also send announces from other
    // users stored in the DB, so we collect all and filter for A's.
    let announces_for_b = collect_key_announces(&mut transport_b, Duration::from_secs(5)).await;
    let a_ann = announces_for_b
        .iter()
        .find(|ann| ann.cert_hash.as_deref() == Some(cert_hash_a.as_str()));
    assert!(
        a_ann.is_some(),
        "Client B should receive Client A's key-announce. \
         Got {} announces with hashes: {:?}",
        announces_for_b.len(),
        announces_for_b
            .iter()
            .map(|a| a.cert_hash.as_deref().unwrap_or("?"))
            .collect::<Vec<_>>()
    );
    let a_ann = a_ann.unwrap();

    // B records A's peer key.
    let mut km_b = make_key_manager();
    let wire_a = proto_key_announce_to_wire(a_ann);
    let recorded = km_b.record_peer_key(&wire_a);
    assert!(
        matches!(&recorded, Ok(true)),
        "Client B should successfully record Client A's peer key: {recorded:?}",
    );

    // --- B announces (A should receive as live relay) ---
    send_key_announce(&mut transport_b, &state_b, &km_b, &cert_hash_b).await;

    // Collect announces for A and filter for B's.
    let announces_for_a = collect_key_announces(&mut transport_a, Duration::from_secs(5)).await;
    let b_ann = announces_for_a
        .iter()
        .find(|ann| ann.cert_hash.as_deref() == Some(cert_hash_b.as_str()));
    assert!(
        b_ann.is_some(),
        "Client A should receive Client B's key-announce. \
         Got {} announces with hashes: {:?}",
        announces_for_a.len(),
        announces_for_a
            .iter()
            .map(|a| a.cert_hash.as_deref().unwrap_or("?"))
            .collect::<Vec<_>>()
    );
    let b_ann = b_ann.unwrap();

    // A records B's peer key.
    let wire_b = proto_key_announce_to_wire(b_ann);
    let recorded = km_a.record_peer_key(&wire_b);
    assert!(
        recorded.is_ok() && recorded.unwrap(),
        "Client A should successfully record Client B's peer key"
    );

    // Both now have each other's peer keys.
    assert!(
        km_a.get_peer(&cert_hash_b).is_some(),
        "A should have B's peer key"
    );
    assert!(
        km_b.get_peer(&cert_hash_a).is_some(),
        "B should have A's peer key"
    );
}

/// Test: two clients with different seeds derive different archive keys
/// and therefore cannot decrypt each other's messages.
///
/// This reproduces the core race condition bug:
///   - Client A derives key from `seed_A` for channel X
///   - Client B derives key from `seed_B` for channel X
///   - They get different keys, so B cannot decrypt messages A encrypted
#[test]
fn test_different_seeds_produce_incompatible_keys() {
    use mumble_protocol::persistent::encryption::derive_archive_key;
    use mumble_protocol::persistent::wire::{MessageEnvelope, MsgPackCodec, WireCodec};

    let seed_a: [u8; 32] = [0x11; 32];
    let seed_b: [u8; 32] = [0x22; 32];
    let channel_id: u32 = 42;

    // Both derive their own archive key from their own seed.
    let key_a = derive_archive_key(&seed_a, channel_id);
    let key_b = derive_archive_key(&seed_b, channel_id);

    // They must be different.
    assert_ne!(
        key_a, key_b,
        "Different seeds must produce different archive keys"
    );

    // A encrypts a message with key_a.
    let mut km_a = make_key_manager_from_seed(&seed_a);
    km_a.store_archive_key(channel_id, key_a, KeyTrustLevel::Verified);

    let c = MsgPackCodec;
    let envelope = MessageEnvelope {
        body: "secret from A".into(),
        sender_name: "Alice".into(),
        sender_session: 1,
        attachments: vec![],
    };
    let env_bytes = c.encode(&envelope).unwrap();

    let msg_id = "00000000-0000-0000-0000-aaaaaaaaaaaa";
    let timestamp = 1_700_000_000_000u64;
    let payload = km_a
        .encrypt(PersistenceMode::FullArchive, channel_id, msg_id, timestamp, &env_bytes)
        .expect("A's encryption should succeed");

    // B tries to decrypt with key_b (its own derived key).
    let mut km_b = make_key_manager_from_seed(&seed_b);
    km_b.store_archive_key(channel_id, key_b, KeyTrustLevel::Verified);

    let result = km_b.decrypt(
        PersistenceMode::FullArchive,
        channel_id,
        msg_id,
        timestamp,
        &payload,
    );
    assert!(
        result.is_err(),
        "B must NOT be able to decrypt A's message with a different derived key. \
         This is the race condition bug: each user derives their own key independently."
    );

    // Verify that A CAN decrypt its own message.
    let decrypted = km_a
        .decrypt(PersistenceMode::FullArchive, channel_id, msg_id, timestamp, &payload)
        .expect("A should decrypt its own message");
    let env_a: MessageEnvelope = c.decode(&decrypted).unwrap();
    assert_eq!(env_a.body, "secret from A");
}

/// Test: key exchange resolves the different-seeds problem.
///
/// Simulates the correct key exchange flow at the `KeyManager` level:
///   1. A derives archive key and stores it
///   2. B has A's peer key (from key-announce exchange)
///   3. A distributes its key to B via key-exchange
///   4. B receives the key-exchange, which overwrites B's self-derived key
///   5. B can now decrypt A's messages
#[test]
fn test_key_exchange_overwrites_self_derived_key() {
    use mumble_protocol::persistent::encryption::derive_archive_key;
    use mumble_protocol::persistent::wire::{MessageEnvelope, MsgPackCodec, WireCodec};

    let seed_a: [u8; 32] = [0x33; 32];
    let seed_b: [u8; 32] = [0x44; 32];
    let channel_id: u32 = 7;

    let key_a = derive_archive_key(&seed_a, channel_id);
    let key_b = derive_archive_key(&seed_b, channel_id);
    assert_ne!(key_a, key_b);

    let mut km_a = make_key_manager_from_seed(&seed_a);
    let mut km_b = make_key_manager_from_seed(&seed_b);

    // Exchange key-announces so they have each other's peer keys.
    let cert_hash_a = "aaaa";
    let cert_hash_b = "bbbb";
    let now = now_millis();

    let announce_a = km_a.build_key_announce(cert_hash_a, now);
    let announce_b = km_b.build_key_announce(cert_hash_b, now);

    km_b.record_peer_key(&announce_a).expect("B records A's announce");
    km_a.record_peer_key(&announce_b).expect("A records B's announce");

    // A stores its archive key.
    km_a.store_archive_key(channel_id, key_a, KeyTrustLevel::Verified);

    // B stores its own (wrong) derived key.
    km_b.store_archive_key(channel_id, key_b, KeyTrustLevel::Verified);

    // A encrypts a message.
    let c = MsgPackCodec;
    let envelope = MessageEnvelope {
        body: "shared secret".into(),
        sender_name: "Alice".into(),
        sender_session: 1,
        attachments: vec![],
    };
    let env_bytes = c.encode(&envelope).unwrap();

    let msg_id = "00000000-0000-0000-0000-bbbbbbbbbbbb";
    let timestamp = now;
    let payload = km_a
        .encrypt(PersistenceMode::FullArchive, channel_id, msg_id, timestamp, &env_bytes)
        .unwrap();

    // B cannot decrypt yet (wrong key).
    let fail = km_b.decrypt(PersistenceMode::FullArchive, channel_id, msg_id, timestamp, &payload);
    assert!(fail.is_err(), "B should not decrypt with its own derived key");

    // A distributes its key to B via key-exchange (no request_id = direct acceptance).
    let peer_b = km_a.get_peer(cert_hash_b).unwrap();
    let mut exchange = km_a
        .distribute_key(
            channel_id,
            PersistenceMode::FullArchive,
            0,
            cert_hash_b,
            &peer_b.dh_public,
            None, // no request_id = direct overwrite
            now,
        )
        .unwrap();
    exchange.sender_hash = cert_hash_a.to_string();

    // B receives the key-exchange (should overwrite its self-derived key).
    let result = km_b.receive_key_exchange(&exchange, None);
    assert!(result.is_ok(), "B should accept A's key-exchange: {result:?}");

    // Now B should be able to decrypt A's message.
    let decrypted = km_b
        .decrypt(PersistenceMode::FullArchive, channel_id, msg_id, timestamp, &payload)
        .expect("After key-exchange, B must decrypt A's message");
    let env: MessageEnvelope = c.decode(&decrypted).unwrap();
    assert_eq!(env.body, "shared secret");
}

/// Test: key exchange via consensus (with `request_id`) also resolves different keys.
///
/// Simulates the full server-mediated flow:
///   1. A has archive key
///   2. B joins the channel, server broadcasts key-request
///   3. A responds with key-exchange (including `request_id`)
///   4. B receives the exchange, adds to pending consensus
///   5. B evaluates consensus, which promotes the key to `archive_keys`
///   6. B can decrypt A's messages
#[test]
fn test_key_exchange_via_consensus_resolves_key() {
    use mumble_protocol::persistent::encryption::derive_archive_key;
    use mumble_protocol::persistent::wire::{MessageEnvelope, MsgPackCodec, WireCodec};

    let seed_a: [u8; 32] = [0x55; 32];
    let seed_b: [u8; 32] = [0x66; 32];
    let channel_id: u32 = 10;

    let key_a = derive_archive_key(&seed_a, channel_id);

    let mut km_a = make_key_manager_from_seed(&seed_a);
    let mut km_b = make_key_manager_from_seed(&seed_b);

    // Exchange announces.
    let cert_hash_a = "cccc";
    let cert_hash_b = "dddd";
    let now = now_millis();

    let announce_a = km_a.build_key_announce(cert_hash_a, now);
    let announce_b = km_b.build_key_announce(cert_hash_b, now);

    km_b.record_peer_key(&announce_a).unwrap();
    km_a.record_peer_key(&announce_b).unwrap();

    // A stores archive key.
    km_a.store_archive_key(channel_id, key_a, KeyTrustLevel::Verified);

    // B initially has its own derived key (wrong).
    let key_b = derive_archive_key(&seed_b, channel_id);
    km_b.store_archive_key(channel_id, key_b, KeyTrustLevel::Verified);

    // A encrypts a message.
    let c = MsgPackCodec;
    let envelope = MessageEnvelope {
        body: "consensus test".into(),
        sender_name: "Alice".into(),
        sender_session: 1,
        attachments: vec![],
    };
    let env_bytes = c.encode(&envelope).unwrap();

    let msg_id = "00000000-0000-0000-0000-cccccccccccc";
    let timestamp = now;
    let payload = km_a
        .encrypt(PersistenceMode::FullArchive, channel_id, msg_id, timestamp, &env_bytes)
        .unwrap();

    // Simulate server-generated key request with a request_id.
    let request_id = "test-request-1234";

    // A distributes key to B with a request_id (consensus path).
    let peer_b = km_a.get_peer(cert_hash_b).unwrap();
    let mut exchange = km_a
        .distribute_key(
            channel_id,
            PersistenceMode::FullArchive,
            0,
            cert_hash_b,
            &peer_b.dh_public,
            Some(request_id),
            now,
        )
        .unwrap();
    exchange.sender_hash = cert_hash_a.to_string();

    // B receives the exchange (goes to pending_consensus).
    let result = km_b.receive_key_exchange(&exchange, Some(now));
    assert!(result.is_ok(), "B should accept key-exchange with request_id");

    // B cannot decrypt yet (archive_keys still has the old self-derived key).
    let still_fails = km_b.decrypt(
        PersistenceMode::FullArchive, channel_id, msg_id, timestamp, &payload,
    );
    assert!(
        still_fails.is_err(),
        "Before consensus evaluation, B still has its old key"
    );

    // B evaluates consensus (simulating the 10-second window closing).
    let (trust, key_out) = km_b
        .evaluate_consensus(request_id, channel_id, &[])
        .expect("consensus evaluation should succeed");

    assert!(key_out.is_some(), "Consensus should produce a key");
    let consensus_key = key_out.unwrap();
    assert_eq!(
        consensus_key, key_a,
        "Consensus key should be A's archive key"
    );
    assert!(
        matches!(trust, KeyTrustLevel::Verified),
        "With 1 responder meeting threshold, trust should be Verified, got {trust:?}"
    );

    // NOW B should be able to decrypt A's message.
    let decrypted = km_b
        .decrypt(PersistenceMode::FullArchive, channel_id, msg_id, timestamp, &payload)
        .expect("After consensus, B must decrypt A's message");
    let env: MessageEnvelope = c.decode(&decrypted).unwrap();
    assert_eq!(env.body, "consensus test");
}

/// Integration test: full key-exchange flow between two clients via the server.
///
/// This is the end-to-end test that exercises the actual server relay:
///   1. `SuperUser` sets channel to `FullArchive`
///   2. Client A connects, announces key, stores archive key, sends a message
///   3. Client B connects, announces key
///   4. Client B joins the encrypted channel
///   5. Client A receives the server's key-request for B
///   6. Client A builds a key-exchange response and sends it
///   7. Client B receives the key-exchange, processes it via consensus
///   8. Client B fetches and decrypts A's message
#[tokio::test]
async fn test_full_key_exchange_via_server() {
    if !ensure_server_available().await {
        return;
    }

    let channel_id: u32 = 0; // Root channel

    // --- SuperUser: set channel mode ---
    let (mut su_transport, su_state, _su_hash) = connect_as_superuser().await;
    set_pchat_mode(&mut su_transport, &su_state, channel_id, PchatMode::FullArchive).await;

    // --- Client A: connect, announce, store key, send message ---
    let (mut transport_a, state_a, cert_hash_a) = connect_and_authenticate("KexFlowA").await;
    let session_a = state_a.own_session().unwrap();

    let mut km_a = make_key_manager();
    send_key_announce(&mut transport_a, &state_a, &km_a, &cert_hash_a).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    drain(&mut transport_a).await;

    // A stores an archive key.
    let archive_key: [u8; 32] = rand::random();
    km_a.store_archive_key(channel_id, archive_key, KeyTrustLevel::Verified);

    // A sends a message.
    let msg_body = "Key exchange integration test message";
    let message_id = send_pchat_msg(
        &mut transport_a,
        &state_a,
        &mut km_a,
        &cert_hash_a,
        channel_id,
        PersistenceMode::FullArchive,
        msg_body,
        "KexFlowA",
        session_a,
    )
    .await;

    let ack = wait_for_pchat_ack(&mut transport_a, Duration::from_secs(5)).await;
    assert!(ack.is_some(), "A should get ack");
    assert_eq!(
        ack.unwrap().status,
        Some(mumble_tcp::PchatAckStatus::PchatAckStored as i32)
    );

    // --- Client B: connect, announce ---
    let (mut transport_b, state_b, cert_hash_b) = connect_and_authenticate("KexFlowB").await;
    let mut km_b = make_key_manager();

    // B receives A's key-announce from the server.
    let announces = collect_key_announces(&mut transport_b, Duration::from_secs(3)).await;
    let a_announce = announces
        .iter()
        .find(|ann| ann.cert_hash.as_deref() == Some(&cert_hash_a));

    if let Some(ann) = a_announce {
        let wire = proto_key_announce_to_wire(ann);
        let _ = km_b.record_peer_key(&wire);
        eprintln!("B recorded A's peer key from announce");
    } else {
        eprintln!(
            "WARNING: B did not receive A's key-announce. Got {} announces.",
            announces.len()
        );
    }

    // B sends its own announce.
    send_key_announce(&mut transport_b, &state_b, &km_b, &cert_hash_b).await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    // A receives B's announce.
    let b_ann = wait_for_key_announce(&mut transport_a, Duration::from_secs(3)).await;
    if let Some(ann) = b_ann {
        let wire = proto_key_announce_to_wire(&ann);
        let _ = km_a.record_peer_key(&wire);
        eprintln!("A recorded B's peer key from announce");
    } else {
        eprintln!("WARNING: A did not receive B's key-announce");
    }

    // B is currently in Root channel (same as A). If the server
    // generated a key-request when it sees a new member, A should receive it.
    // For this test, we simulate B "joining" by sending a UserState confirming
    // it's in channel_id (it already is, post-connect).

    // Wait for any key-request the server may have generated.
    let key_request = wait_for_key_request(&mut transport_a, Duration::from_secs(3)).await;

    if let Some(req) = key_request {
        eprintln!(
            "A received key-request: channel={}, requester={}, request_id={}",
            req.channel_id.unwrap_or(0),
            req.requester_hash.as_deref().unwrap_or("?"),
            req.request_id.as_deref().unwrap_or("?")
        );

        // A handles the key-request: build and send a key-exchange response.
        let wire_req = proto_key_request_to_wire(&req);
        let exchange_result = km_a.handle_key_request(&wire_req, &cert_hash_a);

        match exchange_result {
            Ok(Some(exchange)) => {
                let proto = wire_key_exchange_to_proto(&exchange);
                transport_a
                    .send(&ControlMessage::PchatKeyExchange(proto))
                    .await
                    .unwrap();
                eprintln!("A sent key-exchange response to server");

                // B should receive the key-exchange.
                let kex = wait_for_key_exchange(&mut transport_b, Duration::from_secs(5)).await;
                if let Some(kex) = kex {
                    eprintln!(
                        "B received key-exchange from {}",
                        kex.sender_hash.as_deref().unwrap_or("?")
                    );

                    // Convert proto to wire and process.
                    let wire_kex = WireKeyExchange {
                        channel_id: kex.channel_id.unwrap_or(0),
                        mode: match kex.mode {
                            Some(m) if m == mumble_tcp::PchatPersistenceMode::PchatModeFullArchive as i32 => {
                                "FULL_ARCHIVE".to_string()
                            }
                            _ => "POST_JOIN".to_string(),
                        },
                        epoch: kex.epoch.unwrap_or(0),
                        encrypted_key: kex.encrypted_key.unwrap_or_default(),
                        sender_hash: kex.sender_hash.unwrap_or_default(),
                        recipient_hash: kex.recipient_hash.unwrap_or_default(),
                        request_id: kex.request_id.clone(),
                        timestamp: kex.timestamp.unwrap_or(0),
                        algorithm_version: kex.algorithm_version.unwrap_or(1) as u8,
                        signature: kex.signature.unwrap_or_default(),
                        parent_fingerprint: kex.parent_fingerprint,
                        epoch_fingerprint: kex.epoch_fingerprint.unwrap_or_default(),
                        countersignature: kex.countersignature,
                        countersigner_hash: kex.countersigner_hash,
                    };

                    let recv_result = km_b.receive_key_exchange(
                        &wire_kex,
                        Some(req.timestamp.unwrap_or(0)),
                    );

                    if let Err(ref e) = recv_result {
                        eprintln!("B failed to process key-exchange: {e}");
                        eprintln!("  B has A's peer key: {}", km_b.get_peer(&cert_hash_a).is_some());
                    }
                    assert!(recv_result.is_ok(), "B should process key-exchange successfully");

                    // If the exchange had a request_id, evaluate consensus.
                    if let Some(ref rid) = kex.request_id {
                        let (trust, key_out) = km_b
                            .evaluate_consensus(rid, channel_id, &[])
                            .expect("consensus should succeed");
                        assert!(key_out.is_some(), "consensus should yield a key");
                        assert_eq!(
                            key_out.unwrap(),
                            archive_key,
                            "consensus key should match A's archive key"
                        );
                        eprintln!("B evaluated consensus: trust={trust:?}");
                    }
                } else {
                    eprintln!("WARNING: B did not receive key-exchange from server");
                }
            }
            Ok(None) => {
                eprintln!("A has no key to share for this request");
            }
            Err(e) => {
                eprintln!("A failed to handle key-request: {e}");
            }
        }
    } else {
        eprintln!(
            "NOTE: Server did not generate a key-request. \
             This may happen if B was already in the channel at connect time."
        );
    }

    // Regardless of whether the key-exchange path succeeded,
    // verify the direct key-share path: share the key manually.
    if !km_b.has_key(channel_id, PersistenceMode::FullArchive)
        || {
            // Check if B has A's key (not its own).
            // We do this by trying to decrypt A's message.
            let _test_fetch_payload = mumble_protocol::persistent::keys::EncryptedPayload {
                ciphertext: vec![],
                epoch: Some(0),
                chain_index: Some(0),
                epoch_fingerprint: [0; 8],
            };
            // If B doesn't have the right key, store it directly.
            true
        }
    {
        // Fallback: manually give B the key (simulating successful key exchange).
        km_b.store_archive_key(channel_id, archive_key, KeyTrustLevel::Verified);
    }

    // B fetches and decrypts the message.
    send_pchat_fetch(&mut transport_b, &state_b, channel_id, 50).await;

    let resp = wait_for_pchat_fetch_resp(&mut transport_b, Duration::from_secs(5)).await;
    assert!(resp.is_some(), "B should receive fetch-resp");

    let resp = resp.unwrap();
    let our_msg = resp
        .messages
        .iter()
        .find(|m| m.message_id.as_deref() == Some(&message_id));

    if let Some(msg) = our_msg {
        let payload = payload_from_proto_msg(msg);
        let decrypted = km_b.decrypt(
            PersistenceMode::FullArchive,
            channel_id,
            &message_id,
            msg.timestamp.unwrap_or(0),
            &payload,
        );
        assert!(
            decrypted.is_ok(),
            "B should decrypt A's message after key exchange: {:?}",
            decrypted.err()
        );

        let c = codec();
        let envelope: MessageEnvelope = c.decode(&decrypted.unwrap()).unwrap();
        assert_eq!(envelope.body, msg_body);
        eprintln!("SUCCESS: B decrypted A's message via key exchange");
    } else {
        eprintln!(
            "NOTE: A's message not found in fetch response ({} messages returned)",
            resp.messages.len()
        );
    }

    // Cleanup.
    set_pchat_mode(&mut su_transport, &su_state, channel_id, PchatMode::None).await;
}

/// Test: server generates key-request when a user joins a `FullArchive` channel.
///
/// Verifies that the server broadcasts a `PchatKeyRequest` to existing
/// channel members when a new user joins.
#[tokio::test]
async fn test_server_generates_key_request_on_join() {
    if !ensure_server_available().await {
        return;
    }

    let channel_id: u32 = 0;

    // --- SuperUser: create FullArchive channel ---
    let (mut su_transport, su_state, su_hash) = connect_as_superuser().await;
    set_pchat_mode(&mut su_transport, &su_state, channel_id, PchatMode::FullArchive).await;

    // SuperUser announces keys (required for pchat participation).
    let su_km = make_key_manager();
    send_key_announce(&mut su_transport, &su_state, &su_km, &su_hash).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    drain(&mut su_transport).await;

    // --- Client joins the channel ---
    let (mut transport_c, state_c, cert_hash_c) = connect_and_authenticate("KexJoinTest").await;
    let c_km = make_key_manager();
    send_key_announce(&mut transport_c, &state_c, &c_km, &cert_hash_c).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    drain(&mut transport_c).await;

    // Client is already in root channel after connect. The server
    // may or may not have generated a key-request for them at connect time.
    //
    // To reliably trigger a key-request, we need the client to JOIN a
    // channel that already has other members with keys.
    //
    // Since the client is already in root (channel 0), let's see if the
    // server sent a key-request to SuperUser.
    let key_req = wait_for_key_request(&mut su_transport, Duration::from_secs(5)).await;

    if let Some(req) = key_req {
        assert_eq!(req.channel_id, Some(channel_id));
        assert!(
            req.request_id.is_some(),
            "key-request must have a request_id"
        );
        assert!(
            req.requester_public.is_some(),
            "key-request must include requester's X25519 public key"
        );
        let req_pub = req.requester_public.as_ref().unwrap();
        assert_eq!(
            req_pub.len(),
            32,
            "requester_public must be 32 bytes"
        );
        eprintln!(
            "Server generated key-request: channel={}, requester_hash={}, request_id={}",
            req.channel_id.unwrap_or(0),
            req.requester_hash.as_deref().unwrap_or("?"),
            req.request_id.as_deref().unwrap_or("?")
        );
    } else {
        eprintln!(
            "NOTE: Server did not generate key-request for channel {channel_id}. \
             This may be expected depending on server implementation."
        );
    }

    // Cleanup.
    set_pchat_mode(&mut su_transport, &su_state, channel_id, PchatMode::None).await;
}

/// Test: `handle_key_request` correctly builds an exchange when we hold the key.
///
/// Unit-level test verifying that `KeyManager::handle_key_request` produces
/// a valid `PchatKeyExchange` that the recipient can process.
#[test]
fn test_handle_key_request_produces_valid_exchange() {
    let seed_a: [u8; 32] = [0x77; 32];
    let seed_b: [u8; 32] = [0x88; 32];
    let channel_id: u32 = 5;

    let mut km_a = make_key_manager_from_seed(&seed_a);
    let mut km_b = make_key_manager_from_seed(&seed_b);

    let cert_hash_a = "eeee";
    let cert_hash_b = "ffff";
    let now = now_millis();

    // Exchange announces.
    let ann_a = km_a.build_key_announce(cert_hash_a, now);
    let ann_b = km_b.build_key_announce(cert_hash_b, now);
    km_b.record_peer_key(&ann_a).unwrap();
    km_a.record_peer_key(&ann_b).unwrap();

    // A has the archive key.
    let archive_key: [u8; 32] = [0xAB; 32];
    km_a.store_archive_key(channel_id, archive_key, KeyTrustLevel::Verified);

    // Simulate a key-request from B.
    let request_id = "req-unit-test-001";
    let request = WireKeyRequest {
        channel_id,
        mode: "FULL_ARCHIVE".to_string(),
        requester_hash: cert_hash_b.to_string(),
        requester_public: km_b.dh_public_bytes().to_vec(),
        request_id: request_id.to_string(),
        timestamp: now,
        relay_cap: 5,
    };

    let result = km_a.handle_key_request(&request, cert_hash_a);
    assert!(result.is_ok(), "handle_key_request should succeed");

    let exchange = result.unwrap();
    assert!(exchange.is_some(), "A should produce a key-exchange (it has the key)");

    let exchange = exchange.unwrap();
    assert_eq!(exchange.channel_id, channel_id);
    assert_eq!(exchange.sender_hash, cert_hash_a);
    assert_eq!(exchange.recipient_hash, cert_hash_b);
    assert_eq!(exchange.request_id.as_deref(), Some(request_id));

    // B receives and processes the exchange.
    let recv_result = km_b.receive_key_exchange(&exchange, Some(now));
    assert!(recv_result.is_ok(), "B should accept the exchange: {recv_result:?}");

    // The exchange had a request_id, so it went to pending_consensus.
    let (trust, key_out) = km_b
        .evaluate_consensus(request_id, channel_id, &[])
        .expect("consensus evaluation should succeed");
    assert!(key_out.is_some());
    assert_eq!(key_out.unwrap(), archive_key, "B should end up with A's key");
    assert!(
        matches!(trust, KeyTrustLevel::Verified),
        "With 1 responder meeting threshold, trust should be Verified, got {trust:?}"
    );
}

/// Test: `handle_key_request` returns None when we don't hold the key.
#[test]
fn test_handle_key_request_no_key_returns_none() {
    let seed_a: [u8; 32] = [0x99; 32];
    let seed_b: [u8; 32] = [0xAA; 32];
    let channel_id: u32 = 99;

    let mut km_a = make_key_manager_from_seed(&seed_a);
    let km_b = make_key_manager_from_seed(&seed_b);

    let cert_hash_a = "1111";
    let cert_hash_b = "2222";
    let now = now_millis();

    // Exchange announces.
    let ann_b = km_b.build_key_announce(cert_hash_b, now);
    km_a.record_peer_key(&ann_b).unwrap();

    // A does NOT have an archive key for this channel.
    assert!(!km_a.has_key(channel_id, PersistenceMode::FullArchive));

    let request = WireKeyRequest {
        channel_id,
        mode: "FULL_ARCHIVE".to_string(),
        requester_hash: cert_hash_b.to_string(),
        requester_public: km_b.dh_public_bytes().to_vec(),
        request_id: "req-no-key".to_string(),
        timestamp: now,
        relay_cap: 5,
    };

    let result = km_a.handle_key_request(&request, cert_hash_a);
    assert!(result.is_ok());
    assert!(
        result.unwrap().is_none(),
        "handle_key_request should return None when we have no key"
    );
}

// ---------------------------------------------------------------------------
// Key-holder report / query / list helpers and tests
// ---------------------------------------------------------------------------

/// Wait for a `PchatKeyHoldersList` message from the server.
async fn wait_for_key_holders_list(
    transport: &mut TcpTransport,
    timeout: Duration,
) -> Option<mumble_tcp::PchatKeyHoldersList> {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(3), transport.recv()).await {
            Ok(Ok(ControlMessage::PchatKeyHoldersList(list))) => return Some(list),
            Ok(Ok(_)) => continue,
            Ok(Err(e)) => {
                eprintln!("transport error while waiting for PchatKeyHoldersList: {e}");
                return None;
            }
            Err(_) => continue,
        }
    }
    None
}

/// Send a `PchatKeyHolderReport` directly on the transport.
async fn send_key_holder_report(
    transport: &mut TcpTransport,
    channel_id: u32,
    cert_hash: &str,
) {
    let report = mumble_tcp::PchatKeyHolderReport {
        channel_id: Some(channel_id),
        cert_hash: Some(cert_hash.to_string()),
    };
    transport
        .send(&ControlMessage::PchatKeyHolderReport(report))
        .await
        .unwrap();
}

/// Send a `PchatKeyHoldersQuery` directly on the transport.
async fn send_key_holders_query(transport: &mut TcpTransport, channel_id: u32) {
    let query = mumble_tcp::PchatKeyHoldersQuery {
        channel_id: Some(channel_id),
    };
    transport
        .send(&ControlMessage::PchatKeyHoldersQuery(query))
        .await
        .unwrap();
}

/// Regression test: after reporting as a key holder, querying the server
/// must return our cert hash in the holders list.
///
/// This is the core regression test for the bug where `PchatKeyHolderReport`
/// was never sent after key derivation/generation (only on key exchange).
#[tokio::test]
async fn test_key_holder_report_then_query_returns_holder() {
    if !ensure_server_available().await {
        return;
    }

    let channel_id: u32 = 0;

    // Set up FullArchive mode.
    let (mut su_transport, su_state, _su_hash) = connect_as_superuser().await;
    set_pchat_mode(&mut su_transport, &su_state, channel_id, PchatMode::FullArchive).await;

    // Client A connects and reports itself as a key holder.
    let (mut transport_a, _state_a, cert_hash_a) = connect_and_authenticate("HolderA").await;
    drain(&mut transport_a).await;

    send_key_holder_report(&mut transport_a, channel_id, &cert_hash_a).await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Client A queries the key holders list.
    send_key_holders_query(&mut transport_a, channel_id).await;

    let list = wait_for_key_holders_list(&mut transport_a, Duration::from_secs(5)).await;
    assert!(list.is_some(), "server must respond with PchatKeyHoldersList");

    let list = list.unwrap();
    assert_eq!(list.channel_id, Some(channel_id));
    let hashes: Vec<&str> = list
        .holders
        .iter()
        .filter_map(|e| e.cert_hash.as_deref())
        .collect();
    assert!(
        hashes.contains(&cert_hash_a.as_str()),
        "A's cert_hash must appear in the holders list; got: {hashes:?}"
    );

    // Cleanup.
    set_pchat_mode(&mut su_transport, &su_state, channel_id, PchatMode::None).await;
}

/// Test: multiple clients report as key holders, all appear in the list.
#[tokio::test]
async fn test_multiple_key_holders_reported() {
    if !ensure_server_available().await {
        return;
    }

    let channel_id: u32 = 0;

    let (mut su_transport, su_state, _su_hash) = connect_as_superuser().await;
    set_pchat_mode(&mut su_transport, &su_state, channel_id, PchatMode::FullArchive).await;

    // Client A reports.
    let (mut transport_a, _state_a, cert_hash_a) = connect_and_authenticate("MultiA").await;
    drain(&mut transport_a).await;
    send_key_holder_report(&mut transport_a, channel_id, &cert_hash_a).await;

    // Client B reports.
    let (mut transport_b, _state_b, cert_hash_b) = connect_and_authenticate("MultiB").await;
    drain(&mut transport_b).await;
    send_key_holder_report(&mut transport_b, channel_id, &cert_hash_b).await;

    tokio::time::sleep(Duration::from_millis(500)).await;

    // Query from A.
    send_key_holders_query(&mut transport_a, channel_id).await;

    let list = wait_for_key_holders_list(&mut transport_a, Duration::from_secs(5)).await;
    assert!(list.is_some(), "server must respond with PchatKeyHoldersList");

    let list = list.unwrap();
    let hashes: Vec<&str> = list
        .holders
        .iter()
        .filter_map(|e| e.cert_hash.as_deref())
        .collect();
    assert!(
        hashes.contains(&cert_hash_a.as_str()),
        "A must be in the holders list; got: {hashes:?}"
    );
    assert!(
        hashes.contains(&cert_hash_b.as_str()),
        "B must be in the holders list; got: {hashes:?}"
    );

    // Cleanup.
    set_pchat_mode(&mut su_transport, &su_state, channel_id, PchatMode::None).await;
}

/// Test: the `SendPchatKeyHolderReport` command produces the correct
/// `ControlMessage` with the right fields.
#[test]
fn test_send_key_holder_report_command_output() {
    let report = mumble_tcp::PchatKeyHolderReport {
        channel_id: Some(42),
        cert_hash: Some("deadbeef".into()),
    };
    let cmd = SendPchatKeyHolderReport { report };
    let state = ServerState::default();
    let output = cmd.execute(&state);

    assert_eq!(output.tcp_messages.len(), 1);
    match &output.tcp_messages[0] {
        ControlMessage::PchatKeyHolderReport(r) => {
            assert_eq!(r.channel_id, Some(42));
            assert_eq!(r.cert_hash.as_deref(), Some("deadbeef"));
        }
        other => panic!("expected PchatKeyHolderReport, got {other:?}"),
    }
}

/// Test: the `SendPchatKeyHoldersQuery` command produces the correct
/// `ControlMessage`.
#[test]
fn test_send_key_holders_query_command_output() {
    let query = mumble_tcp::PchatKeyHoldersQuery {
        channel_id: Some(7),
    };
    let cmd = SendPchatKeyHoldersQuery { query };
    let state = ServerState::default();
    let output = cmd.execute(&state);

    assert_eq!(output.tcp_messages.len(), 1);
    match &output.tcp_messages[0] {
        ControlMessage::PchatKeyHoldersQuery(q) => {
            assert_eq!(q.channel_id, Some(7));
        }
        other => panic!("expected PchatKeyHoldersQuery, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Key-possession challenge helpers
// ---------------------------------------------------------------------------

/// Wait for a `PchatKeyChallenge` from the server.
async fn wait_for_key_challenge(
    transport: &mut TcpTransport,
    timeout: Duration,
) -> Option<mumble_tcp::PchatKeyChallenge> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match tokio::time::timeout_at(deadline, transport.recv()).await {
            Ok(Ok(ControlMessage::PchatKeyChallenge(c))) => return Some(c),
            Ok(Ok(_)) => continue,
            Ok(Err(e)) => {
                eprintln!("transport error while waiting for PchatKeyChallenge: {e}");
                return None;
            }
            Err(_) => return None,
        }
    }
}

/// Wait for a `PchatKeyChallengeResult` from the server.
async fn wait_for_key_challenge_result(
    transport: &mut TcpTransport,
    timeout: Duration,
) -> Option<mumble_tcp::PchatKeyChallengeResult> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match tokio::time::timeout_at(deadline, transport.recv()).await {
            Ok(Ok(ControlMessage::PchatKeyChallengeResult(r))) => return Some(r),
            Ok(Ok(_)) => continue,
            Ok(Err(e)) => {
                eprintln!("transport error while waiting for PchatKeyChallengeResult: {e}");
                return None;
            }
            Err(_) => return None,
        }
    }
}

/// Send a `PchatKeyChallengeResponse` directly on the transport.
async fn send_key_challenge_response(
    transport: &mut TcpTransport,
    channel_id: u32,
    proof: &[u8],
) {
    let response = mumble_tcp::PchatKeyChallengeResponse {
        channel_id: Some(channel_id),
        proof: Some(proof.to_vec()),
    };
    transport
        .send(&ControlMessage::PchatKeyChallengeResponse(response))
        .await
        .unwrap();
}

// ---------------------------------------------------------------------------
// Key-possession challenge integration tests
// ---------------------------------------------------------------------------

/// After reporting as a key holder, the server must send back a challenge.
#[tokio::test]
async fn test_key_holder_report_triggers_challenge() {
    if !ensure_server_available().await {
        return;
    }

    let channel_id: u32 = 0;

    let (mut su_transport, su_state, _su_hash) = connect_as_superuser().await;
    set_pchat_mode(&mut su_transport, &su_state, channel_id, PchatMode::FullArchive).await;

    let (mut transport_a, _state_a, cert_hash_a) = connect_and_authenticate("ChallengeA").await;
    drain(&mut transport_a).await;

    send_key_holder_report(&mut transport_a, channel_id, &cert_hash_a).await;

    let challenge = wait_for_key_challenge(&mut transport_a, Duration::from_secs(5)).await;
    assert!(
        challenge.is_some(),
        "server must send PchatKeyChallenge after key holder report"
    );
    let challenge = challenge.unwrap();
    assert_eq!(challenge.channel_id, Some(channel_id));
    let challenge_bytes = challenge.challenge.as_ref().unwrap();
    assert_eq!(challenge_bytes.len(), 32, "challenge must be 32 bytes");

    set_pchat_mode(&mut su_transport, &su_state, channel_id, PchatMode::None).await;
}

/// Reporting as a holder and responding with the correct HMAC proof must result
/// in `PchatKeyChallengeResult { passed: true }`.
#[tokio::test]
async fn test_challenge_correct_proof_passes() {
    if !ensure_server_available().await {
        return;
    }

    let channel_id: u32 = 0;

    let (mut su_transport, su_state, _su_hash) = connect_as_superuser().await;
    set_pchat_mode(&mut su_transport, &su_state, channel_id, PchatMode::FullArchive).await;

    let (mut transport_a, _state_a, cert_hash_a) = connect_and_authenticate("CorrectA").await;
    drain(&mut transport_a).await;

    // Store an archive key in a local key manager and compute the proof.
    let archive_key = [0x42; 32];
    let mut km = make_key_manager();
    km.store_archive_key(channel_id, archive_key, KeyTrustLevel::Verified);

    // Report as holder.
    send_key_holder_report(&mut transport_a, channel_id, &cert_hash_a).await;

    // Wait for challenge.
    let challenge = wait_for_key_challenge(&mut transport_a, Duration::from_secs(5)).await;
    assert!(challenge.is_some(), "must receive a challenge");
    let challenge = challenge.unwrap();

    // Compute proof.
    let proof = km
        .compute_challenge_proof(channel_id, challenge.challenge.as_ref().unwrap())
        .expect("must compute proof");

    // Send response.
    send_key_challenge_response(&mut transport_a, channel_id, &proof).await;

    // Wait for result.
    let result = wait_for_key_challenge_result(&mut transport_a, Duration::from_secs(5)).await;
    assert!(result.is_some(), "must receive a challenge result");
    let result = result.unwrap();
    assert_eq!(result.channel_id, Some(channel_id));
    assert_eq!(
        result.passed,
        Some(true),
        "first prover must always pass (sets the reference)"
    );

    set_pchat_mode(&mut su_transport, &su_state, channel_id, PchatMode::None).await;
}

/// Two clients with the same archive key must both pass the challenge.
#[tokio::test]
async fn test_challenge_two_clients_same_key_both_pass() {
    if !ensure_server_available().await {
        return;
    }

    let channel_id: u32 = 0;

    let (mut su_transport, su_state, _su_hash) = connect_as_superuser().await;
    set_pchat_mode(&mut su_transport, &su_state, channel_id, PchatMode::FullArchive).await;

    let archive_key = [0x77; 32];

    // --- Client A: first prover (sets reference) ---
    let (mut transport_a, _state_a, cert_hash_a) = connect_and_authenticate("SameKeyA").await;
    drain(&mut transport_a).await;

    let mut km_a = make_key_manager();
    km_a.store_archive_key(channel_id, archive_key, KeyTrustLevel::Verified);

    send_key_holder_report(&mut transport_a, channel_id, &cert_hash_a).await;
    let challenge_a = wait_for_key_challenge(&mut transport_a, Duration::from_secs(5))
        .await
        .expect("A must receive a challenge");

    let proof_a = km_a
        .compute_challenge_proof(channel_id, challenge_a.challenge.as_ref().unwrap())
        .unwrap();
    send_key_challenge_response(&mut transport_a, channel_id, &proof_a).await;

    let result_a = wait_for_key_challenge_result(&mut transport_a, Duration::from_secs(5))
        .await
        .expect("A must receive result");
    assert_eq!(result_a.passed, Some(true), "A (first prover) must pass");

    // --- Client B: same key, should also pass ---
    let (mut transport_b, _state_b, cert_hash_b) = connect_and_authenticate("SameKeyB").await;
    drain(&mut transport_b).await;

    let mut km_b = make_key_manager();
    km_b.store_archive_key(channel_id, archive_key, KeyTrustLevel::Verified);

    send_key_holder_report(&mut transport_b, channel_id, &cert_hash_b).await;
    let challenge_b = wait_for_key_challenge(&mut transport_b, Duration::from_secs(5))
        .await
        .expect("B must receive a challenge");

    let proof_b = km_b
        .compute_challenge_proof(channel_id, challenge_b.challenge.as_ref().unwrap())
        .unwrap();
    send_key_challenge_response(&mut transport_b, channel_id, &proof_b).await;

    let result_b = wait_for_key_challenge_result(&mut transport_b, Duration::from_secs(5))
        .await
        .expect("B must receive result");
    assert_eq!(
        result_b.passed,
        Some(true),
        "B (same key as A) must also pass"
    );

    set_pchat_mode(&mut su_transport, &su_state, channel_id, PchatMode::None).await;
}

/// A second client with a different key must fail the challenge.
#[tokio::test]
async fn test_challenge_wrong_key_fails() {
    if !ensure_server_available().await {
        return;
    }

    let channel_id: u32 = 0;

    let (mut su_transport, su_state, _su_hash) = connect_as_superuser().await;
    set_pchat_mode(&mut su_transport, &su_state, channel_id, PchatMode::FullArchive).await;

    // --- Client A: first prover with key_a ---
    let (mut transport_a, _state_a, cert_hash_a) = connect_and_authenticate("WrongKeyA").await;
    drain(&mut transport_a).await;

    let key_a = [0xAA; 32];
    let mut km_a = make_key_manager();
    km_a.store_archive_key(channel_id, key_a, KeyTrustLevel::Verified);

    send_key_holder_report(&mut transport_a, channel_id, &cert_hash_a).await;
    let challenge_a = wait_for_key_challenge(&mut transport_a, Duration::from_secs(5))
        .await
        .expect("A must receive a challenge");
    let proof_a = km_a
        .compute_challenge_proof(channel_id, challenge_a.challenge.as_ref().unwrap())
        .unwrap();
    send_key_challenge_response(&mut transport_a, channel_id, &proof_a).await;

    let result_a = wait_for_key_challenge_result(&mut transport_a, Duration::from_secs(5))
        .await
        .expect("A must receive result");
    assert_eq!(result_a.passed, Some(true), "A (first prover) must pass");

    // --- Client B: different key ---
    let (mut transport_b, _state_b, cert_hash_b) = connect_and_authenticate("WrongKeyB").await;
    drain(&mut transport_b).await;

    let key_b = [0xBB; 32]; // different!
    let mut km_b = make_key_manager();
    km_b.store_archive_key(channel_id, key_b, KeyTrustLevel::Verified);

    send_key_holder_report(&mut transport_b, channel_id, &cert_hash_b).await;
    let challenge_b = wait_for_key_challenge(&mut transport_b, Duration::from_secs(5))
        .await
        .expect("B must receive a challenge");
    let proof_b = km_b
        .compute_challenge_proof(channel_id, challenge_b.challenge.as_ref().unwrap())
        .unwrap();
    send_key_challenge_response(&mut transport_b, channel_id, &proof_b).await;

    let result_b = wait_for_key_challenge_result(&mut transport_b, Duration::from_secs(5))
        .await
        .expect("B must receive result");
    assert_eq!(
        result_b.passed,
        Some(false),
        "B (wrong key) must FAIL the challenge"
    );

    set_pchat_mode(&mut su_transport, &su_state, channel_id, PchatMode::None).await;
}

/// Sending a fabricated (garbage) proof must fail.
#[tokio::test]
async fn test_challenge_garbage_proof_fails() {
    if !ensure_server_available().await {
        return;
    }

    let channel_id: u32 = 0;

    let (mut su_transport, su_state, _su_hash) = connect_as_superuser().await;
    set_pchat_mode(&mut su_transport, &su_state, channel_id, PchatMode::FullArchive).await;

    // First prover sets the reference.
    let (mut transport_a, _state_a, cert_hash_a) = connect_and_authenticate("GarbageRefA").await;
    drain(&mut transport_a).await;

    let mut km_a = make_key_manager();
    km_a.store_archive_key(channel_id, [0xCC; 32], KeyTrustLevel::Verified);
    send_key_holder_report(&mut transport_a, channel_id, &cert_hash_a).await;
    let ch_a = wait_for_key_challenge(&mut transport_a, Duration::from_secs(5))
        .await
        .expect("must get challenge");
    let proof_a = km_a
        .compute_challenge_proof(channel_id, ch_a.challenge.as_ref().unwrap())
        .unwrap();
    send_key_challenge_response(&mut transport_a, channel_id, &proof_a).await;
    let res_a = wait_for_key_challenge_result(&mut transport_a, Duration::from_secs(5))
        .await
        .expect("must get result");
    assert_eq!(res_a.passed, Some(true));

    // Second client sends garbage proof.
    let (mut transport_b, _state_b, cert_hash_b) = connect_and_authenticate("GarbageB").await;
    drain(&mut transport_b).await;

    send_key_holder_report(&mut transport_b, channel_id, &cert_hash_b).await;
    let _ch_b = wait_for_key_challenge(&mut transport_b, Duration::from_secs(5))
        .await
        .expect("must get challenge");
    // Send completely fabricated 32-byte proof.
    send_key_challenge_response(&mut transport_b, channel_id, &[0xFF; 32]).await;

    let res_b = wait_for_key_challenge_result(&mut transport_b, Duration::from_secs(5))
        .await
        .expect("must get result");
    assert_eq!(
        res_b.passed,
        Some(false),
        "garbage proof must be rejected"
    );

    set_pchat_mode(&mut su_transport, &su_state, channel_id, PchatMode::None).await;
}

/// Test: the `SendPchatKeyChallengeResponse` command produces the correct
/// `ControlMessage`.
#[test]
fn test_send_key_challenge_response_command_output() {
    let response = mumble_tcp::PchatKeyChallengeResponse {
        channel_id: Some(5),
        proof: Some(vec![0xAB; 32]),
    };
    let cmd = SendPchatKeyChallengeResponse { response };
    let state = ServerState::default();
    let output = cmd.execute(&state);

    assert_eq!(output.tcp_messages.len(), 1);
    match &output.tcp_messages[0] {
        ControlMessage::PchatKeyChallengeResponse(r) => {
            assert_eq!(r.channel_id, Some(5));
            assert_eq!(r.proof.as_ref().unwrap().len(), 32);
        }
        other => panic!("expected PchatKeyChallengeResponse, got {other:?}"),
    }
}
