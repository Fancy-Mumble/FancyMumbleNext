//! Connection lifecycle: `connect()` and `disconnect()`.

use std::time::Duration;

use tauri::{Emitter, Manager};
use tracing::info;

use mumble_protocol::client::ClientConfig;
use mumble_protocol::command;
use mumble_protocol::transport::tcp::TcpConfig;
use mumble_protocol::transport::udp::UdpConfig;

use super::event_handler::TauriEventHandler;
use super::types::*;
use super::AppState;

impl AppState {
    #[allow(clippy::too_many_lines, reason = "connection setup spans auth, TLS, state init, and event loop spawn")]
    pub async fn connect(
        &self,
        host: String,
        port: u16,
        username: String,
        cert_label: Option<String>,
        password: Option<String>,
    ) -> Result<(), String> {
        let inner = self.inner.clone();
        let app_handle = self.app_handle().ok_or("App not initialized")?;

        // Abort any stale event loop and clear state before starting a new
        // connection. Dropping a JoinHandle only detaches the task - it keeps
        // running.  We must `.abort()` it explicitly so its `on_disconnected`
        // callback cannot fire and clobber the new connection's state.
        {
            let mut state = inner.lock().map_err(|e| e.to_string())?;

            // Abort the old event-loop task (if any).
            if let Some(handle) = state.event_loop_handle.take() {
                handle.abort();
            }
            // Abort any stale connecting-phase task (in case a previous
            // connect() was cancelled before the handshake completed).
            if let Some(task) = state.connect_task_handle.take() {
                task.abort();
            }

            state.connection_epoch += 1;
            state.status = ConnectionStatus::Connecting;
            state.own_name = username.clone();
            state.connected_host = host.clone();
            state.connected_port = port;
            state.users.clear();
            state.channels.clear();
            state.messages.clear();
            state.own_session = None;
            state.client_handle = None;
            state.synced = false;
            state.permanently_listened.clear();
            state.selected_channel = None;
            state.current_channel = None;
            state.unread_counts.clear();
            state.server_config = ServerConfig::default();
            state.voice_state = VoiceState::Inactive;
            state.root_permissions = None;
            state.pchat = None;
            state.pchat_seed = None;
            state.pchat_identity_dir = None;
            state.pending_key_shares.clear();
            state.tauri_app_handle = Some(app_handle.clone());
        }

        // Initialise the cert-hash-to-username resolver (persisted across sessions).
        if let Ok(data_dir) = app_handle.path().app_data_dir() {
            let hash_names_path = data_dir.join("hash_names.json");
            let resolver = super::hash_names::DefaultHashNameResolver::new(hash_names_path);
            if let Ok(mut state) = inner.lock() {
                state.hash_name_resolver =
                    Some(std::sync::Arc::new(resolver));
            }
        }

        // Migrate legacy storage layout (certs/ + pchat/) to per-identity
        // folders on first connect after the update.  Idempotent.
        if let Ok(data_dir) = app_handle.path().app_data_dir() {
            super::pchat::migrate_legacy_storage(&data_dir);
        }

        // Load (or generate) the persistent chat identity seed
        // scoped to this certificate / identity label.
        let identity_label = cert_label.clone().unwrap_or_else(|| "default".to_string());
        if let Ok(data_dir) = app_handle.path().app_data_dir() {
            match super::pchat::load_or_generate_seed(&data_dir, &identity_label) {
                Ok(seed) => {
                    if let Ok(mut state) = inner.lock() {
                        state.pchat_seed = Some(seed);
                        state.pchat_identity_dir =
                            Some(super::pchat::identity_dir(&data_dir, &identity_label));
                    }
                }
                Err(e) => {
                    tracing::warn!("failed to load pchat seed: {e}");
                }
            }
        }

        // Emit status change so the frontend can show a loading screen immediately.
        let _ = app_handle.emit("status-changed", "connecting");

        // Spawn the actual connection work in the background so we don't
        // block the Tauri command (which freezes the webview).
        let connect_task = tokio::spawn(async move {
            // Load client certificate from the per-identity folder.
            let (client_cert_pem, client_key_pem) = if let Some(ref label) = cert_label {
                app_handle
                    .path()
                    .app_data_dir()
                    .ok()
                    .map(|d| super::pchat::load_identity_cert(&d, label))
                    .unwrap_or((None, None))
            } else {
                (None, None)
            };

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
                ..ClientConfig::default()
            };

            let epoch = inner.lock().map(|s| s.connection_epoch).unwrap_or(0);
            let handler = TauriEventHandler {
                shared: inner.clone(),
                app: app_handle.clone(),
                epoch,
            };

            let result = mumble_protocol::client::run(config, handler).await;

            match result {
                Ok((handle, join)) => {
                    // Store the client handle and event-loop JoinHandle for later
                    // commands and for awaiting a clean shutdown on disconnect.
                    // Clear connect_task_handle - the connecting phase is done.
                    if let Ok(mut state) = inner.lock() {
                        state.client_handle = Some(handle.clone());
                        state.event_loop_handle = Some(join);
                        state.connect_task_handle = None;
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
                        if let Ok(mut state) = inner.lock() {
                            state.status = ConnectionStatus::Disconnected;
                            state.client_handle = None;
                            state.event_loop_handle = None;
                            state.connect_task_handle = None;
                        }
                        let _ = app_handle.emit(
                            "connection-rejected",
                            RejectedPayload {
                                reason: format!("Failed to authenticate: {e}"),
                                reject_type: None,
                            },
                        );
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
                    if let Ok(mut state) = inner.lock() {
                        state.status = ConnectionStatus::Disconnected;
                        state.client_handle = None;
                        state.event_loop_handle = None;
                        state.connect_task_handle = None;
                    }
                    let _ = app_handle.emit(
                        "connection-rejected",
                        RejectedPayload {
                            reason: format!("Connection failed: {e}"),
                            reject_type: None,
                        },
                    );
                }
            }
        });

        // Store the task handle so disconnect() can abort it if the user
        // cancels before the TCP handshake completes.
        if let Ok(mut state) = self.inner.lock() {
            state.connect_task_handle = Some(connect_task);
        }

        Ok(())
    }

    pub async fn disconnect(&self) -> Result<(), String> {
        // Stop audio before disconnecting.
        self.stop_audio();

        // Stop Android foreground service before tearing down the connection.
        #[cfg(target_os = "android")]
        {
            use tauri::Manager;
            if let Some(app_handle) = self.app_handle() {
                if let Some(handle) =
                    app_handle.try_state::<crate::connection_service::ConnectionServiceHandle>()
                {
                    crate::connection_service::stop_service(&handle);
                }
            }
        }

        let (handle, join, connect_task) = {
            let mut guard = self.inner.lock().map_err(|e| e.to_string())?;
            guard.user_initiated_disconnect = true;
            (
                guard.client_handle.take(),
                guard.event_loop_handle.take(),
                guard.connect_task_handle.take(),
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

        if let Ok(mut state) = self.inner.lock() {
            state.status = ConnectionStatus::Disconnected;
            state.client_handle = None;
            state.connect_task_handle = None;
            state.event_loop_handle = None;
            state.users.clear();
            state.channels.clear();
            state.messages.clear();
            state.own_session = None;
            state.synced = false;
            state.permanently_listened.clear();
            state.selected_channel = None;
            state.current_channel = None;
            state.unread_counts.clear();
            state.server_config = ServerConfig::default();
            state.voice_state = VoiceState::Inactive;
            state.root_permissions = None;
            state.pchat = None;
            state.pchat_seed = None;
            state.pchat_identity_dir = None;
            state.pending_key_shares.clear();
        }

        Ok(())
    }
}
