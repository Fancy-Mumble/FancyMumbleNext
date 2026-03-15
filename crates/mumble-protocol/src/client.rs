//! Client orchestrator - the async event loop that ties everything together.
//!
//! Spawns independent tasks for TCP reading, UDP reading, and a periodic
//! ping timer, all feeding into the priority work queue. The main loop
//! drains the queue and dispatches each item.

use std::time::Duration;

use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::command::{BoxedCommand, CommandAction};
use crate::error::{Error, Result};
use crate::event::EventHandler;
use crate::message::{ControlMessage, ServerMessage, UdpMessage};
use crate::proto::mumble_tcp;
use crate::state::ServerState;
use crate::transport::tcp::{TcpConfig, TcpTransport};
use crate::transport::udp::{PlaintextCryptState, UdpConfig, UdpTransport};
use crate::work_queue::{self, WorkItem, WorkQueueSender};

/// Top-level configuration for the Mumble client.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub tcp: TcpConfig,
    pub udp: UdpConfig,
    /// Interval between keep-alive TCP pings.
    pub ping_interval: Duration,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            tcp: TcpConfig::default(),
            udp: UdpConfig::default(),
            ping_interval: Duration::from_secs(15),
        }
    }
}

/// Handle returned to callers for submitting commands into the running client.
#[derive(Clone)]
pub struct ClientHandle {
    cmd_tx: mpsc::Sender<BoxedCommand>,
}

impl ClientHandle {
    /// Submit a command to the running client.
    ///
    /// The command carries its own data and will produce the right protocol
    /// messages when processed by the event loop.
    pub async fn send<C: CommandAction>(&self, cmd: C) -> Result<()> {
        self.cmd_tx
            .send(Box::new(cmd))
            .await
            .map_err(|_| Error::QueueClosed)
    }
}

/// Build and run the Mumble client.
///
/// Returns a [`ClientHandle`] for submitting commands and a `JoinHandle`
/// for the main event loop task.
pub async fn run<H: EventHandler>(
    config: ClientConfig,
    handler: H,
) -> Result<(ClientHandle, tokio::task::JoinHandle<()>)> {
    // 1. Connect TCP - retry on transient connection-reset errors (e.g. the
    //    server hasn't cleaned up a previous session yet, Windows error 10054).
    const MAX_RETRIES: u32 = 3;
    const RETRY_DELAY: Duration = Duration::from_secs(2);

    let mut tcp = None;
    let mut last_err = None;

    for attempt in 1..=MAX_RETRIES {
        match TcpTransport::connect(&config.tcp).await {
            Ok(t) => {
                tcp = Some(t);
                break;
            }
            Err(e) => {
                // Only retry on ConnectionRefused (server briefly down or
                // still starting up).  ConnectionReset / ConnectionAborted
                // at this early stage almost always means the server
                // deliberately closed the connection (e.g. IP ban, version
                // mismatch) - retrying would just spam the server.
                let is_retryable = matches!(&e, Error::Io(io)
                    if io.kind() == std::io::ErrorKind::ConnectionRefused
                );
                if is_retryable && attempt < MAX_RETRIES {
                    warn!(
                        "TCP connection attempt {attempt}/{MAX_RETRIES} failed ({e}), \
                         retrying in {}s…",
                        RETRY_DELAY.as_secs()
                    );
                    tokio::time::sleep(RETRY_DELAY).await;
                    last_err = Some(e);
                } else {
                    return Err(e);
                }
            }
        }
    }

    let mut tcp = tcp.ok_or_else(|| last_err.unwrap())?;
    info!("TCP connected to {}:{}", config.tcp.server_host, config.tcp.server_port);

    // 2. Send the Version message FIRST - before anything else touches the
    //    stream.  The server requires version >= 1.4 for channel listen.
    let version_msg = ControlMessage::Version(mumble_tcp::Version {
        // Legacy v1 encoding: (major << 16) | (minor << 8) | patch
        version_v1: Some((1 << 16) | (5 << 8)),
        // v2 encoding: (major << 48) | (minor << 32) | (patch << 16)
        version_v2: Some((1u64 << 48) | (5u64 << 32)),
        release: Some("FancyMumble 0.1.0".into()),
        os: Some(std::env::consts::OS.into()),
        os_version: None,
        // Announce Fancy Mumble extension support (version 1).
        // The server responds with its own fancy_version if it supports them.
        fancy_version: Some(1),
    });
    tcp.send(&version_msg).await?;
    info!("Version 1.5.0 sent");

    let (tcp_reader, tcp_writer) = tcp.split();

    // 2. Create work queue
    let (wq_sender, wq_receiver) = work_queue::create();

    // 3. Build client handle (for external command submission)
    let (ext_cmd_tx, ext_cmd_rx) = mpsc::channel::<BoxedCommand>(32);
    let client_handle = ClientHandle {
        cmd_tx: ext_cmd_tx,
    };

    // 4. Spawn the main event loop
    let ping_interval = config.ping_interval;
    let udp_config = config.udp;

    let join = tokio::spawn(async move {
        if let Err(e) = event_loop(
            handler,
            tcp_reader,
            tcp_writer,
            udp_config,
            wq_sender,
            wq_receiver,
            ext_cmd_rx,
            ping_interval,
        )
        .await
        {
            error!("client event loop error: {e}");
        }
    });

    Ok((client_handle, join))
}

// ── Event loop ─────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn event_loop<H: EventHandler>(
    mut handler: H,
    mut tcp_reader: crate::transport::tcp::TcpReader,
    mut tcp_writer: crate::transport::tcp::TcpWriter,
    udp_config: UdpConfig,
    wq_sender: WorkQueueSender,
    mut wq_receiver: work_queue::WorkQueueReceiver,
    mut ext_cmd_rx: mpsc::Receiver<BoxedCommand>,
    ping_interval: Duration,
) -> Result<()> {
    let mut state = ServerState::new();

    // Create a single outbound channel that serialises all TCP writes.
    // Ping task, user commands, and the main loop all send through here.
    let (outbound_tx, mut outbound_rx) = mpsc::channel::<ControlMessage>(64);
    let tcp_writer_task = tokio::spawn(async move {
        while let Some(msg) = outbound_rx.recv().await {
            if let Err(e) = tcp_writer.send(&msg).await {
                error!("failed to send TCP message: {e}");
                break;
            }
        }
    });

    // Spawn TCP reader task
    let tcp_wq = wq_sender.clone();
    let tcp_reader_task = tokio::spawn(async move {
        loop {
            match tcp_reader.recv().await {
                Ok(msg) => {
                    if tcp_wq.send_tcp(msg).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    warn!("TCP read error: {e}");
                    break;
                }
            }
        }
    });

    // Spawn periodic ping task.
    // Pings must be written directly to the TCP stream - they are outbound
    // keep-alive messages, not inbound server messages to process.
    // Include accumulated ping statistics so the server and other clients
    // can see our connection quality.
    let ping_tx = outbound_tx.clone();
    let ping_stats = state.ping_stats.clone();
    let ping_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(ping_interval);
        loop {
            interval.tick().await;
            let stats_snapshot = ping_stats.lock().ok().map(|s| s.clone()).unwrap_or_default();
            let ping = mumble_tcp::Ping {
                timestamp: Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                ),
                tcp_packets: Some(stats_snapshot.tcp_packets),
                tcp_ping_avg: Some(stats_snapshot.tcp_ping_avg),
                tcp_ping_var: Some(stats_snapshot.tcp_ping_var),
                udp_packets: Some(stats_snapshot.udp_packets),
                udp_ping_avg: Some(stats_snapshot.udp_ping_avg),
                udp_ping_var: Some(stats_snapshot.udp_ping_var),
                ..Default::default()
            };
            if ping_tx
                .send(ControlMessage::Ping(ping))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    // Forward external commands into the work queue
    let cmd_wq = wq_sender.clone();
    let cmd_forwarder_task = tokio::spawn(async move {
        while let Some(cmd) = ext_cmd_rx.recv().await {
            if cmd_wq.send_command(cmd).await.is_err() {
                break;
            }
        }
    });

    // Optional: UDP transport (connected after CryptSetup)
    let mut _udp: Option<UdpTransport<PlaintextCryptState>> = None;

    // Main dispatch loop
    info!("entering main event loop");
    loop {
        let item = wq_receiver.recv().await;

        match item {
            WorkItem::ServerMessage(server_msg) => {
                match &server_msg {
                    ServerMessage::Control(ctrl) => {
                        // UdpTunnel carries audio (or pings) tunnelled over TCP.
                        // Try protobuf first (Mumble 1.5+), then legacy binary.
                        if let ControlMessage::UdpTunnel(ref data) = ctrl {
                            match crate::transport::audio_codec::decode_tunnel_audio(data) {
                                Ok(audio) => {
                                    handler.on_udp_message(&UdpMessage::Audio(audio));
                                }
                                Err(e) => {
                                    warn!("UdpTunnel audio decode failed ({} bytes): {e}", data.len());
                                }
                            }
                        } else {
                            handle_control_message(ctrl, &mut state, &mut handler);

                            // If we got CryptSetup, try to start UDP
                            if matches!(ctrl, ControlMessage::CryptSetup(_)) {
                                match start_udp(&udp_config, &wq_sender).await {
                                    Ok(transport) => {
                                        _udp = Some(transport);
                                        debug!("UDP transport started");
                                    }
                                    Err(e) => warn!("failed to start UDP: {e}"),
                                }
                            }

                            // A Reject means the server will close the
                            // connection - exit the loop immediately instead
                            // of lingering until the TCP stream dies.
                            if matches!(ctrl, ControlMessage::Reject(_)) {
                                info!("server rejected connection, exiting event loop");
                                handler.on_disconnected();
                                break;
                            }
                        }
                    }
                    ServerMessage::Udp(udp_msg) => {
                        handler.on_udp_message(udp_msg);
                    }
                }
            }

            WorkItem::UserCommand(cmd) => {
                let output = cmd.execute(&state);

                // Send TCP messages
                for msg in output.tcp_messages {
                    if outbound_tx.send(msg).await.is_err() {
                        error!("outbound channel closed");
                        break;
                    }
                }

                // Send UDP messages
                // (For now UDP audio falls back to TCP tunnel if no UDP)
                for udp_msg in &output.udp_messages {
                    if let UdpMessage::Audio(audio) = udp_msg {
                        // Encode as protobuf v2 format - the server negotiated
                        // this because we advertised version_v2 (1.5.0).
                        let tunnel_data =
                            crate::transport::audio_codec::encode_protobuf_audio(audio);
                        debug!(
                            frame = audio.frame_number,
                            opus_len = audio.opus_data.len(),
                            "outbound tunnel audio ({} bytes)",
                            tunnel_data.len()
                        );
                        let tunnel = ControlMessage::UdpTunnel(tunnel_data);
                        if outbound_tx.send(tunnel).await.is_err() {
                            error!("outbound channel closed");
                            break;
                        }
                    }
                }

                if output.disconnect {
                    info!("disconnect requested");
                    handler.on_disconnected();
                    break;
                }
            }

            WorkItem::Shutdown => {
                info!("shutdown signal");
                handler.on_disconnected();
                break;
            }
        }
    }

    // ── Clean up all spawned sub-tasks ─────────────────────────────
    // These tasks hold channel senders/TCP stream halves.  If we just
    // drop them (detach), the ping task keeps `ping_tx` alive which
    // keeps the TCP writer alive, so the connection never closes.
    ping_task.abort();
    cmd_forwarder_task.abort();
    tcp_reader_task.abort();
    tcp_writer_task.abort();
    debug!("all sub-tasks aborted");

    Ok(())
}

// ── Helpers ────────────────────────────────────────────────────────

fn handle_control_message<H: EventHandler>(
    msg: &ControlMessage,
    state: &mut ServerState,
    handler: &mut H,
) {
    // Update internal state
    match msg {
        ControlMessage::Version(v) => {
            state.apply_version(v);
            if let Some(fv) = v.fancy_version {
                info!(fancy_version = fv, "server supports Fancy Mumble extensions");
            }
        }
        ControlMessage::Ping(p) => {
            if let Some(ts) = p.timestamp {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                let rtt_ms = now.saturating_sub(ts) as f32;
                state.record_tcp_ping(rtt_ms);
            }
        }
        ControlMessage::ServerSync(sync) => {
            state.apply_server_sync(sync);
            info!(
                session = state.own_session(),
                "server sync complete"
            );
            handler.on_connected();
        }
        ControlMessage::UserState(us) => state.apply_user_state(us),
        ControlMessage::UserRemove(ur) => state.remove_user(ur.session),
        ControlMessage::ChannelState(cs) => state.apply_channel_state(cs),
        ControlMessage::ChannelRemove(cr) => state.remove_channel(cr.channel_id),
        ControlMessage::Reject(r) => {
            warn!(reason = ?r.reason, "connection rejected");
        }
        ControlMessage::PermissionDenied(pd) => {
            warn!(reason = ?pd.reason, r#type = ?pd.r#type, "permission denied");
        }
        ControlMessage::PermissionQuery(pq) => {
            state.apply_permission_query(pq);
        }
        _ => {}
    }

    // Notify the event handler
    handler.on_control_message(msg);
}

async fn start_udp(
    config: &UdpConfig,
    _wq_sender: &WorkQueueSender,
) -> Result<UdpTransport<PlaintextCryptState>> {
    let transport =
        UdpTransport::connect(config, PlaintextCryptState).await?;

    // Spawn UDP reader task
    // We need a second transport for reading - for now, return the one we have
    // and note that a production implementation would split or use Arc.
    // This is a placeholder; real OCB2 encryption + split is needed.
    debug!("UDP transport connected (plaintext mode)");
    Ok(transport)
}
