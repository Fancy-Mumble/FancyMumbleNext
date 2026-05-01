//! Connection lifecycle: `connect()` and `disconnect()`.

use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use tracing::info;

use mumble_protocol::client::ClientConfig;
use mumble_protocol::command;
use mumble_protocol::transport::tcp::TcpConfig;
use mumble_protocol::transport::udp::UdpConfig;

use super::event_handler::TauriEventHandler;
use super::sessions::ServerId;
use super::types::*;
use super::{AppState, SharedState};

impl AppState {
    pub async fn connect(
        &self,
        host: String,
        port: u16,
        username: String,
        cert_label: Option<String>,
        password: Option<String>,
    ) -> Result<(), String> {
        let app_handle = self.app_handle().ok_or("App not initialized")?;

        // Allocate a fresh `SharedState` for this session and register
        // it.  Existing sessions stay alive on their own `Arc`s; we
        // simply swap the `inner` handle to point at the new one so it
        // becomes the active session.
        let server_id = ServerId::new();
        let inner = self.fresh_session_state();
        let _ = self
            .registry
            .register_active(server_id, std::sync::Arc::clone(&inner));
        let _ = self.inner.swap(std::sync::Arc::clone(&inner));

        reset_state_for_connect(
            &inner,
            &username,
            &host,
            port,
            cert_label.as_deref(),
            server_id,
            &app_handle,
        )?;
        init_identity(&inner, &app_handle, &cert_label);

        // Emit status change so the frontend can show a loading screen immediately.
        let _ = app_handle.emit("status-changed", "connecting");

        // Spawn the actual connection work in the background so we don't
        // block the Tauri command (which freezes the webview).
        let registry = self.registry.clone();
        let active_handle = self.inner.clone();
        let connect_task = tokio::spawn(async move {
            // Load client certificate from the per-identity folder.
            let (client_cert_pem, client_key_pem) = if let Some(ref label) = cert_label {
                app_handle
                    .path()
                    .app_data_dir()
                    .ok()
                    .map(|d| super::pchat::IdentityStore::new(d).load_cert(label))
                    .unwrap_or((None, None))
            } else {
                (None, None)
            };

            // Read force_tcp_audio from current audio settings.
            let force_tcp = inner
                .lock()
                .map(|s| s.audio.settings.force_tcp_audio)
                .unwrap_or(false);

            let config = ClientConfig {
                tcp: TcpConfig {
                    server_host: host.clone(),
                    server_port: port,
                    accept_invalid_certs: true,
                    client_cert_pem,
                    client_key_pem,
                },
                udp: UdpConfig {
                    server_host: host,
                    server_port: port,
                },
                force_tcp,
                ..ClientConfig::default()
            };

            let epoch = inner.lock().map(|s| s.conn.epoch).unwrap_or(0);
            let handler = TauriEventHandler {
                shared: inner.clone(),
                app: app_handle.clone(),
                epoch,
                server_id,
                inbound_audio_count: 0,
            };

            let result = mumble_protocol::client::run(config, handler).await;
            handle_connect_result(
                result,
                ConnectResultCtx {
                    inner: &inner,
                    app_handle: &app_handle,
                    username,
                    password,
                    registry: &registry,
                    server_id,
                    active_handle: &active_handle,
                },
            )
            .await;
        });

        // Store the task handle so disconnect() can abort it if the user
        // cancels before the TCP handshake completes.
        if let Ok(mut state) = self.inner.snapshot().lock() {
            state.conn.connect_task_handle = Some(connect_task);
        }

        Ok(())
    }

    pub async fn disconnect(&self) -> Result<(), String> {
        match self.registry.active_id() {
            Some(id) => self.disconnect_session(id).await,
            None => Ok(()),
        }
    }

    /// Tear down a specific session by id.  When `id` is the currently
    /// active session this also stops audio and rebinds `inner` to
    /// whichever session becomes active next (or the empty default if
    /// none remain).  When `id` is a non-active session the active
    /// session's audio pipeline and `inner` pointer are left untouched.
    pub async fn disconnect_session(&self, id: ServerId) -> Result<(), String> {
        let arc = self
            .registry
            .session(id)
            .ok_or_else(|| format!("unknown server id: {id}"))?;
        let is_active = self.registry.active_id() == Some(id);

        if is_active {
            // Stop audio for the session being torn down (== current inner).
            self.stop_audio_on(&arc);

            // Stop Android foreground service for the active connection.
            #[cfg(target_os = "android")]
            {
                if let Some(app_handle) = self.app_handle() {
                    if let Some(handle) = app_handle
                        .try_state::<crate::platform::android::connection_service::ConnectionServiceHandle>()
                    {
                        crate::platform::android::connection_service::stop_service(&handle);
                    }
                }
            }
        }

        let (handle, join, connect_task) = {
            let mut guard = arc.lock().map_err(|e| e.to_string())?;
            guard.conn.user_initiated_disconnect = true;
            (
                guard.conn.client_handle.take(),
                guard.conn.event_loop_handle.take(),
                guard.conn.connect_task_handle.take(),
            )
        };

        // If we are still in the connecting phase (TCP handshake not done yet)
        // abort the outer spawn so the attempt is cancelled immediately.
        if let Some(task) = connect_task {
            task.abort();
        }

        if let Some(handle) = handle {
            let _ = handle.send(command::Disconnect).await;
        }

        // Wait for the event loop to finish so the TCP stream is properly
        // closed (TLS close_notify + TCP FIN) before we return.  This
        // prevents "ghost sessions" on the server that block reconnects.
        if let Some(join) = join {
            let abort_handle = join.abort_handle();
            match tokio::time::timeout(Duration::from_secs(3), join).await {
                Ok(Ok(())) => info!("event loop shut down cleanly"),
                Ok(Err(e)) => tracing::warn!("event loop task panicked: {e}"),
                Err(_) => {
                    tracing::warn!("event loop did not shut down within 3 s, aborting");
                    abort_handle.abort();
                }
            }
        }
        if let Ok(mut state) = arc.lock() {
            // Persist signal bridge sender key state before dropping pchat.
            // Note: on_disconnected may have already cleared pchat, so this
            // is a safety net for cases where disconnect() runs first.
            if let Some(ref pchat) = state.pchat_ctx.pchat {
                pchat.save_signal_state();
                pchat.save_local_cache();
            }

            state.conn.status = ConnectionStatus::Disconnected;
            state.server_id = None;
            state.cert_label = None;
            state.conn.client_handle = None;
            state.conn.connect_task_handle = None;
            state.conn.event_loop_handle = None;
            state.users.clear();
            state.channels.clear();
            state.msgs.by_channel.clear();
            state.conn.own_session = None;
            state.conn.synced = false;
            state.permanently_listened.clear();
            state.selected_channel = None;
            state.current_channel = None;
            state.msgs.channel_unread.clear();
            state.server.config = ServerConfig::default();
            state.audio.voice_state = VoiceState::Inactive;
            state.server.root_permissions = None;
            state.pchat_ctx.pchat = None;
            state.pchat_ctx.seed = None;
            state.pchat_ctx.identity_dir = None;
            state.pchat_ctx.pending_key_shares.clear();
        }

        // Drop the session from the registry so `list_servers` reflects
        // the disconnect.  If this was the active session, also rebind
        // `inner` to whichever session (if any) becomes active next.
        let _ = self.registry.remove(id);
        if is_active {
            match self.registry.active_id().and_then(|nid| self.registry.session(nid)) {
                Some(next_arc) => {
                    let _ = self.inner.swap(next_arc);
                }
                None => self.reset_to_default(),
            }
        }

        Ok(())
    }
}

// -- Helpers ----------------------------------------------------------

use std::sync::Mutex;

type SharedInner = std::sync::Arc<Mutex<SharedState>>;

/// Abort stale tasks and reset all connection-related state for a fresh
/// connection attempt.
fn reset_state_for_connect(
    inner: &SharedInner,
    username: &str,
    host: &str,
    port: u16,
    cert_label: Option<&str>,
    server_id: ServerId,
    app_handle: &AppHandle,
) -> Result<(), String> {
    let mut state = inner.lock().map_err(|e| e.to_string())?;

    // Abort the old event-loop task (if any).
    if let Some(handle) = state.conn.event_loop_handle.take() {
        handle.abort();
    }
    // Abort any stale connecting-phase task (in case a previous
    // connect() was cancelled before the handshake completed).
    if let Some(task) = state.conn.connect_task_handle.take() {
        task.abort();
    }

    state.conn.epoch += 1;
    state.server_id = Some(server_id);
    state.cert_label = cert_label.map(str::to_owned);
    state.conn.status = ConnectionStatus::Connecting;
    state.conn.own_name = username.to_owned();
    state.server.host = host.to_owned();
    state.server.port = port;
    state.users.clear();
    state.channels.clear();
    state.msgs.by_channel.clear();
    state.conn.own_session = None;
    state.conn.client_handle = None;
    state.conn.synced = false;
    state.permanently_listened.clear();
    state.selected_channel = None;
    state.current_channel = None;
    state.msgs.channel_unread.clear();
    state.server.config = ServerConfig::default();
    state.audio.voice_state = VoiceState::Inactive;
    state.server.root_permissions = None;
    // Save signal state before dropping pchat (connect-time reset).
    if let Some(ref pchat) = state.pchat_ctx.pchat {
        pchat.save_signal_state();
        pchat.save_local_cache();
    }
    state.pchat_ctx.pchat = None;
    state.pchat_ctx.seed = None;
    state.pchat_ctx.identity_dir = None;
    state.pchat_ctx.pending_key_shares.clear();
    state.conn.tauri_app_handle = Some(app_handle.clone());

    Ok(())
}

/// Initialise the cert-hash resolver, migrate legacy storage, and load
/// the identity seed for the given certificate label.
fn init_identity(
    inner: &SharedInner,
    app_handle: &AppHandle,
    cert_label: &Option<String>,
) {
    // Cert-hash-to-username resolver (persisted across sessions).
    if let Ok(data_dir) = app_handle.path().app_data_dir() {
        let hash_names_path = data_dir.join("hash_names.json");
        let resolver = super::hash_names::DefaultHashNameResolver::new(hash_names_path);
        if let Ok(mut state) = inner.lock() {
            state.pchat_ctx.hash_name_resolver = Some(std::sync::Arc::new(resolver));
        }
    }

    // Migrate legacy storage layout (certs/ + pchat/) to per-identity
    // folders on first connect after the update.  Idempotent.
    if let Ok(data_dir) = app_handle.path().app_data_dir() {
        super::pchat::IdentityStore::new(data_dir).migrate_legacy_storage();
    }

    // Load (or generate) the persistent chat identity seed.
    let identity_label = cert_label.as_deref().unwrap_or("default");
    if let Ok(data_dir) = app_handle.path().app_data_dir() {
        let store = super::pchat::IdentityStore::new(data_dir);
        match store.load_or_generate_seed(identity_label) {
            Ok(seed) => {
                if let Ok(mut state) = inner.lock() {
                    state.pchat_ctx.seed = Some(seed);
                    state.pchat_ctx.identity_dir = Some(store.identity_dir(identity_label));
                }
            }
            Err(e) => {
                tracing::warn!("failed to load pchat seed: {e}");
            }
        }
    }
}

/// Bundle of context passed to [`handle_connect_result`] so the
/// function signature stays within Clippy's `too_many_arguments` limit.
struct ConnectResultCtx<'a> {
    inner: &'a SharedInner,
    app_handle: &'a AppHandle,
    username: String,
    password: Option<String>,
    registry: &'a super::registry::Registry,
    server_id: ServerId,
    active_handle: &'a super::shared_handle::SharedHandle,
}

/// Handle the result of `mumble_protocol::client::run()`: store handles,
/// send Authenticate, or emit rejection events on failure.
async fn handle_connect_result(
    result: Result<
        (mumble_protocol::client::ClientHandle, tokio::task::JoinHandle<()>),
        mumble_protocol::error::Error,
    >,
    ctx: ConnectResultCtx<'_>,
) {
    let ConnectResultCtx {
        inner,
        app_handle,
        username,
        password,
        registry,
        server_id,
        active_handle,
    } = ctx;
    match result {
        Ok((handle, join)) => {
            if let Ok(mut state) = inner.lock() {
                state.conn.client_handle = Some(handle.clone());
                state.conn.event_loop_handle = Some(join);
                state.conn.connect_task_handle = None;
            }

            // Send Authenticate command.
            if let Err(e) = handle
                .send(command::Authenticate {
                    username,
                    password,
                    tokens: vec![],
                })
                .await
            {
                tracing::error!("Failed to send auth: {e}");
                mark_disconnected(inner);
                let _ = registry.remove(server_id);
                rebind_active(active_handle, registry);
                let reason = format!("Failed to authenticate: {e}");
                emit_session_rejected(app_handle, server_id, reason);
                return;
            }

            info!("TCP connected, authenticate sent - waiting for ServerSync");

            // Start deaf+muted so the user does not transmit or
            // hear audio until they explicitly enable voice calling.
            if let Err(e) = handle
                .send(command::SetSelfDeaf { deafened: true })
                .await
            {
                tracing::warn!("failed to send initial self-deaf: {e}");
            }
        }
        Err(e) => {
            tracing::error!("Connection failed: {e}");
            mark_disconnected(inner);
            let _ = registry.remove(server_id);
            rebind_active(active_handle, registry);
            let reason = format!("Connection failed: {e}");
            emit_session_rejected(app_handle, server_id, reason);
        }
    }
}

/// Emit `connection-rejected` (and matching `server-disconnected`) for
/// a specific session id, ensuring the frontend can route the events
/// to the correct tab without affecting other sessions.
fn emit_session_rejected(app_handle: &AppHandle, server_id: ServerId, reason: String) {
    let id = server_id.to_string();
    let _ = app_handle.emit(
        "connection-rejected",
        serde_json::json!({
            "serverId": id.clone(),
            "reason": reason.clone(),
            "reject_type": serde_json::Value::Null,
        }),
    );
    let _ = app_handle.emit(
        "server-disconnected",
        DisconnectedPayload { server_id: Some(id), reason: Some(reason) },
    );
}

/// Clear connection handles and set status to `Disconnected`.
fn mark_disconnected(inner: &SharedInner) {
    if let Ok(mut state) = inner.lock() {
        state.conn.status = ConnectionStatus::Disconnected;
        state.server_id = None;
        state.cert_label = None;
        state.conn.client_handle = None;
        state.conn.event_loop_handle = None;
        state.conn.connect_task_handle = None;
    }
}

/// After a session is removed from the registry, point `active_handle`
/// at whichever session became active next (or leave it pointing at the
/// failed session if nothing remains; callers may also explicitly reset).
fn rebind_active(
    active_handle: &super::shared_handle::SharedHandle,
    registry: &super::registry::Registry,
) {
    if let Some(arc) = registry
        .active_id()
        .and_then(|id| registry.session(id))
    {
        let _ = active_handle.swap(arc);
    }
}
