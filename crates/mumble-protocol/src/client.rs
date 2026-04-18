//! Client orchestrator - the async event loop that ties everything together.
//!
//! Spawns independent tasks for TCP reading, UDP reading, and a periodic
//! ping timer, all feeding into the priority work queue. The main loop
//! drains the queue and dispatches each item.

use std::sync::Arc;
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::sync::{mpsc, watch};
use tracing::{debug, error, info, trace, warn};

use crate::command::{BoxedCommand, CommandAction};
use crate::error::{Error, Result};
use crate::event::EventHandler;
use crate::fancy_codec::{self, FancyCodec};
use crate::message::{ControlMessage, ServerMessage, UdpMessage};
use crate::transport::audio_codec::AudioPacketCodec;
use crate::transport::ocb2::Ocb2CryptState;
use crate::proto::mumble_tcp;
use crate::state::ServerState;
use crate::transport::tcp::{TcpConfig, TcpTransport};
use crate::transport::udp::{CryptState, UdpConfig, UdpTransport};
use crate::work_queue::{self, WorkItem, WorkQueueSender};

/// The Mumble protocol version advertised to the server.
///
/// The server uses this to decide which features to enable (e.g. channel
/// listen requires >= 1.4, protobuf audio requires >= 1.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MumbleVersion {
    /// Major version component (e.g. **1** in 1.6.0).
    pub major: u16,
    /// Minor version component (e.g. **6** in 1.6.0).
    pub minor: u16,
    /// Patch version component (e.g. **0** in 1.6.0).
    pub patch: u16,
}

impl MumbleVersion {
    /// Create a new version from major/minor/patch components.
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self { major, minor, patch }
    }

    /// Legacy v1 encoding: `(major << 16) | (minor << 8) | patch`.
    pub const fn encode_v1(self) -> u32 {
        ((self.major as u32) << 16) | ((self.minor as u32) << 8) | (self.patch as u32)
    }

    /// v2 encoding: `(major << 48) | (minor << 32) | (patch << 16)`.
    pub const fn encode_v2(self) -> u64 {
        ((self.major as u64) << 48) | ((self.minor as u64) << 32) | ((self.patch as u64) << 16)
    }
}

impl std::fmt::Display for MumbleVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl Default for MumbleVersion {
    fn default() -> Self {
        Self::new(1, 6, 0)
    }
}

/// Top-level configuration for the Mumble client.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// TCP transport configuration (host, port, TLS).
    pub tcp: TcpConfig,
    /// UDP transport configuration (host, port).
    pub udp: UdpConfig,
    /// Interval between keep-alive TCP pings.
    pub ping_interval: Duration,
    /// Mumble protocol version advertised to the server.
    pub version: MumbleVersion,
    /// When true, always send audio via TCP tunnel even if UDP is available.
    pub force_tcp: bool,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            tcp: TcpConfig::default(),
            udp: UdpConfig::default(),
            ping_interval: Duration::from_secs(15),
            version: MumbleVersion::default(),
            force_tcp: false,
        }
    }
}

/// Handle returned to callers for submitting commands into the running client.
#[derive(Debug, Clone)]
pub struct ClientHandle {
    cmd_tx: mpsc::Sender<BoxedCommand>,
    force_tcp_tx: watch::Sender<bool>,
    audio_out_tx: mpsc::Sender<UdpMessage>,
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

    /// Submit an outbound audio packet on the high-priority audio channel.
    ///
    /// Unlike [`send`], this bypasses the command queue and is drained at
    /// the same priority as inbound UDP, so audio is never starved by
    /// control traffic.  Non-blocking: drops the packet if the channel is
    /// full.
    pub fn send_audio(&self, msg: UdpMessage) -> Result<()> {
        self.audio_out_tx.try_send(msg).map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => {
                Error::InvalidState("audio channel full".into())
            }
            mpsc::error::TrySendError::Closed(_) => Error::QueueClosed,
        })
    }

    /// Toggle force-TCP mode at runtime.
    ///
    /// When set to `true`, any active UDP transport is torn down and audio
    /// falls back to the TCP tunnel.  When set back to `false`, the event
    /// loop re-establishes UDP using the stored crypto material from the
    /// last `CryptSetup`.
    pub fn set_force_tcp(&self, force: bool) {
        // watch::Sender::send only fails if all receivers are dropped,
        // which means the event loop has already exited.
        let _ = self.force_tcp_tx.send(force);
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
                         retrying in {}s...",
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

    let mut tcp = tcp.ok_or_else(|| last_err.unwrap_or_else(|| Error::Other("connection loop exited without recording an error".into())))?;
    info!("TCP connected to {}:{}", config.tcp.server_host, config.tcp.server_port);

    // 2. Send the Version message FIRST - before anything else touches the
    //    stream.  The server requires version >= 1.4 for channel listen.
    let ver = config.version;
    let version_msg = ControlMessage::Version(mumble_tcp::Version {
        version_v1: Some(ver.encode_v1()),
        version_v2: Some(ver.encode_v2()),
        release: Some(format!("FancyMumble {}", env!("CARGO_PKG_VERSION"))),
        os: Some(std::env::consts::OS.into()),
        os_version: None,
        // Announce Fancy Mumble extension support, version derived from Cargo.toml.
        // The server responds with its own fancy_version if it supports them.
        fancy_version: Some(crate::FANCY_VERSION),
    });
    tcp.send(&version_msg).await?;
    info!("Version {ver} sent");

    let (tcp_reader, tcp_writer) = tcp.split();

    // 2. Create work queue
    let (wq_sender, wq_receiver, audio_out_rx) = work_queue::create();

    // 3. Build client handle (for external command submission)
    let (ext_cmd_tx, ext_cmd_rx) = mpsc::channel::<BoxedCommand>(32);
    let (force_tcp_tx, force_tcp_rx) = watch::channel(config.force_tcp);
    let client_handle = ClientHandle {
        cmd_tx: ext_cmd_tx,
        force_tcp_tx,
        audio_out_tx: wq_sender.audio_sender(),
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
            force_tcp_rx,
            wq_sender,
            wq_receiver,
            audio_out_rx,
            ext_cmd_rx,
            ping_interval,
        )
        .await
        {
            error!("client event loop error: {e}");
        }
        warn!("client event loop task exiting");
    });

    Ok((client_handle, join))
}

// -- Event loop -----------------------------------------------------

#[allow(clippy::too_many_arguments, reason = "protocol event loop requires all transport handles")]
async fn event_loop<H: EventHandler>(
    mut handler: H,
    tcp_reader: crate::transport::tcp::TcpReader,
    tcp_writer: crate::transport::tcp::TcpWriter,
    udp_config: UdpConfig,
    mut force_tcp_rx: watch::Receiver<bool>,
    wq_sender: WorkQueueSender,
    mut wq_receiver: work_queue::WorkQueueReceiver,
    mut audio_out_rx: mpsc::Receiver<UdpMessage>,
    ext_cmd_rx: mpsc::Receiver<BoxedCommand>,
    ping_interval: Duration,
) -> Result<()> {
    let mut state = ServerState::new();
    let state_decrypt_stats = state.decrypt_stats.clone();

    let (outbound_tx, outbound_rx) = mpsc::channel::<ControlMessage>(64);
    let tcp_writer_task = tokio::spawn(tcp_writer_loop(tcp_writer, outbound_rx));
    let mut tcp_reader_task = tokio::spawn(tcp_reader_loop(tcp_reader, wq_sender.clone()));
    let ping_task = tokio::spawn(ping_loop(
        outbound_tx.clone(),
        state.ping_stats.clone(),
        state.decrypt_stats.clone(),
        ping_interval,
    ));
    let cmd_forwarder_task = tokio::spawn(cmd_forwarder_loop(ext_cmd_rx, wq_sender.clone()));

    let mut codec: Box<dyn FancyCodec> = Box::new(fancy_codec::LegacyCodec);
    let mut udp_sender: Option<UdpSender> = None;
    let mut udp_reader_task: Option<tokio::task::JoinHandle<()>> = None;
    let mut stored_crypto: Option<StoredCrypto> = None;
    let mut force_tcp = *force_tcp_rx.borrow();

    info!("entering main event loop");
    let mut tcp_reader_alive = true;
    let mut outbound_audio_count: u64 = 0;
    let mut loop_iteration: u64 = 0;

    loop {
        loop_iteration += 1;

        let item = tokio::select! {
            biased;
            item = wq_receiver.recv() => Some(item),
            Some(msg) = audio_out_rx.recv() => {
                send_one_audio_packet(&msg, &mut udp_sender, &outbound_tx, &mut outbound_audio_count);
                None
            }
            Ok(()) = force_tcp_rx.changed() => {
                let new_force = *force_tcp_rx.borrow_and_update();
                if new_force != force_tcp {
                    force_tcp = new_force;
                    handle_force_tcp_change(
                        force_tcp, &stored_crypto, &udp_config, &wq_sender,
                        &mut udp_sender, &mut udp_reader_task, &state.decrypt_stats, &mut handler,
                    ).await;
                }
                None
            }
            result = &mut tcp_reader_task, if tcp_reader_alive => {
                tcp_reader_alive = false;
                warn!("TCP reader ended unexpectedly: {result:?}");
                Some(WorkItem::Shutdown)
            }
        };

        let Some(item) = item else { continue };

        let mut ctx = EventLoopCtx {
            handler: &mut handler,
            state: &mut state,
            outbound_tx: &outbound_tx,
            codec: &mut codec,
            udp_sender: &mut udp_sender,
            udp_reader_task: &mut udp_reader_task,
            stored_crypto: &mut stored_crypto,
            decrypt_stats: state_decrypt_stats.clone(),
            udp_config: &udp_config,
            wq_sender: &wq_sender,
            force_tcp,
        };
        if ctx.dispatch_item(item, loop_iteration).await == LoopAction::Break {
            break;
        }
    }

    ping_task.abort();
    cmd_forwarder_task.abort();
    if tcp_reader_alive {
        tcp_reader_task.abort();
    }
    tcp_writer_task.abort();
    if let Some(task) = &udp_reader_task {
        task.abort();
    }
    debug!("all sub-tasks aborted");
    Ok(())
}

// -- Helpers --------------------------------------------------------

/// Drains the outbound channel and writes each message to the TCP stream.
async fn tcp_writer_loop(
    mut tcp_writer: crate::transport::tcp::TcpWriter,
    mut outbound_rx: mpsc::Receiver<ControlMessage>,
) {
    while let Some(msg) = outbound_rx.recv().await {
        if let Err(e) = tcp_writer.send(&msg).await {
            error!("failed to send TCP message: {e}");
            break;
        }
    }
}

/// Reads control messages from the TCP stream and feeds them into the
/// work queue.
async fn tcp_reader_loop(
    mut tcp_reader: crate::transport::tcp::TcpReader,
    wq_sender: WorkQueueSender,
) {
    loop {
        match tcp_reader.recv().await {
            Ok(msg) => {
                if wq_sender.send_tcp(msg).await.is_err() {
                    break;
                }
            }
            Err(e) => {
                warn!("TCP read error: {e}");
                break;
            }
        }
    }
}

/// Sends periodic TCP pings with accumulated latency statistics.
async fn ping_loop(
    outbound_tx: mpsc::Sender<ControlMessage>,
    ping_stats: crate::state::SharedPingStats,
    decrypt_stats: crate::transport::ocb2::SharedPacketStats,
    interval_duration: Duration,
) {
    let mut interval = tokio::time::interval(interval_duration);
    loop {
        let _ = interval.tick().await;
        let stats_snapshot = ping_stats
            .lock()
            .ok()
            .map(|s| s.clone())
            .unwrap_or_default();
        let crypto_snapshot = decrypt_stats
            .lock()
            .ok()
            .map(|s| s.clone())
            .unwrap_or_default();
        let ping = mumble_tcp::Ping {
            timestamp: Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
            ),
            good: Some(crypto_snapshot.good),
            late: Some(crypto_snapshot.late),
            lost: Some(crypto_snapshot.lost),
            resync: Some(crypto_snapshot.resync),
            tcp_packets: Some(stats_snapshot.tcp_packets),
            tcp_ping_avg: Some(stats_snapshot.tcp_ping_avg),
            tcp_ping_var: Some(stats_snapshot.tcp_ping_var),
            udp_packets: Some(stats_snapshot.udp_packets),
            udp_ping_avg: Some(stats_snapshot.udp_ping_avg),
            udp_ping_var: Some(stats_snapshot.udp_ping_var),
        };
        if outbound_tx
            .send(ControlMessage::Ping(ping))
            .await
            .is_err()
        {
            break;
        }
    }
}

/// Forwards externally submitted commands into the priority work queue.
async fn cmd_forwarder_loop(
    mut ext_cmd_rx: mpsc::Receiver<BoxedCommand>,
    wq_sender: WorkQueueSender,
) {
    while let Some(cmd) = ext_cmd_rx.recv().await {
        if wq_sender.send_command(cmd).await.is_err() {
            break;
        }
    }
}

/// Signal from a dispatch handler: keep looping or exit.
#[derive(PartialEq, Eq)]
enum LoopAction {
    Continue,
    Break,
}

/// Mutable context threaded through event-loop dispatch helpers,
/// avoiding long parameter lists on each function call.
struct EventLoopCtx<'a, H> {
    handler: &'a mut H,
    state: &'a mut ServerState,
    outbound_tx: &'a mpsc::Sender<ControlMessage>,
    codec: &'a mut Box<dyn FancyCodec>,
    udp_sender: &'a mut Option<UdpSender>,
    udp_reader_task: &'a mut Option<tokio::task::JoinHandle<()>>,
    stored_crypto: &'a mut Option<StoredCrypto>,
    decrypt_stats: crate::transport::ocb2::SharedPacketStats,
    udp_config: &'a UdpConfig,
    wq_sender: &'a WorkQueueSender,
    force_tcp: bool,
}

impl<H: EventHandler> EventLoopCtx<'_, H> {
    async fn dispatch_item(&mut self, item: WorkItem, loop_iteration: u64) -> LoopAction {
        let before = tokio::time::Instant::now();
        let action = match item {
            WorkItem::ServerMessage(msg) => self.handle_server_message(msg).await,
            WorkItem::UserCommand(cmd) => self.handle_user_command(cmd).await,
            WorkItem::Shutdown => {
                info!("shutdown signal");
                self.handler.on_disconnected();
                LoopAction::Break
            }
        };
        let elapsed = before.elapsed();
        if elapsed.as_millis() > 50 {
            warn!("event loop: processing took {elapsed:?} (iter={loop_iteration})");
        }
        action
    }

    async fn handle_server_message(&mut self, server_msg: ServerMessage) -> LoopAction {
        // Decode: unwrap Fancy messages from PluginData on legacy servers.
        let server_msg = match server_msg {
            ServerMessage::Control(ctrl) => ServerMessage::Control(self.codec.decode(ctrl)),
            udp @ ServerMessage::Udp(_) => udp,
        };

        match &server_msg {
            ServerMessage::Control(ctrl) => {
                if !matches!(
                    ctrl,
                    ControlMessage::UdpTunnel(_)
                        | ControlMessage::Ping(_)
                        | ControlMessage::PermissionQuery(_)
                ) {
                    debug!(type_id = ctrl.type_id(), "inbound control message");
                }
                if let ControlMessage::UdpTunnel(ref data) = ctrl {
                    trace!("handle_server_message: UdpTunnel ({} bytes)", data.len());
                    match crate::transport::audio_codec::decode_tunnel_audio(data) {
                        Ok(audio) => self.handler.on_udp_message(&UdpMessage::Audio(audio)),
                        Err(e) => {
                            warn!(
                                "UdpTunnel audio decode failed ({} bytes): {e}",
                                data.len()
                            );
                        }
                    }
                } else {
                    if let ControlMessage::CryptSetup(cs) = ctrl {
                        handle_crypt_setup(
                            cs,
                            self.udp_config,
                            self.force_tcp,
                            self.wq_sender,
                            self.stored_crypto,
                            self.udp_sender,
                            self.udp_reader_task,
                            &self.decrypt_stats,
                            self.handler,
                        )
                        .await;
                    }

                    handle_control_message(ctrl, self.state, self.handler);

                    // Upgrade the codec when the server announces its Fancy version.
                    if matches!(ctrl, ControlMessage::Version(_)) {
                        *self.codec = fancy_codec::select_codec(
                            self.state.connection.server_fancy_version,
                        );
                    }

                    // Piggyback a UDP ping on every TCP Ping response to
                    // keep the NAT mapping alive.
                    if matches!(ctrl, ControlMessage::Ping(_)) {
                        self.send_udp_ping().await;
                    }

                    if matches!(ctrl, ControlMessage::Reject(_)) {
                        info!("server rejected connection, exiting event loop");
                        self.handler.on_disconnected();
                        return LoopAction::Break;
                    }
                }
            }
            ServerMessage::Udp(udp_msg) => {
                trace!("handle_server_message: UDP message");
                self.handler.on_udp_message(udp_msg);
                trace!("handle_server_message: on_udp_message returned");
            }
        }
        LoopAction::Continue
    }

    async fn handle_user_command(&mut self, cmd: BoxedCommand) -> LoopAction {
        let output = cmd.execute(self.state);

        for msg in output.tcp_messages {
            let type_id = msg.type_id();
            let Some(msg) = self.codec.encode(msg, self.state) else {
                warn!(type_id, "codec dropped outbound message");
                continue;
            };
            if self.outbound_tx.send(msg).await.is_err() {
                error!("outbound channel closed");
                break;
            }
        }

        self.send_udp_output(&output.udp_messages).await;

        if output.disconnect {
            info!("disconnect requested");
            self.handler.on_disconnected();
            return LoopAction::Break;
        }
        LoopAction::Continue
    }

    /// Send a UDP ping to keep the NAT mapping alive.
    async fn send_udp_ping(&mut self) {
        if let Some(sender) = &mut self.udp_sender {
            let payload = crate::transport::udp::encode_udp_message(&udp_ping_message());
            if let Err(e) = sender.send_raw(&payload).await {
                warn!("UDP ping send failed: {e}");
            }
        }
    }

    /// Send outbound UDP audio, preferring real UDP with TCP tunnel fallback.
    async fn send_udp_output(&mut self, messages: &[UdpMessage]) {
        for udp_msg in messages {
            let UdpMessage::Audio(audio) = udp_msg else {
                continue;
            };

            let use_tunnel = if let Some(sender) = &mut self.udp_sender {
                let payload = crate::transport::udp::encode_udp_message(udp_msg);
                if let Err(e) = sender.send_raw(&payload).await {
                    warn!("UDP send failed, falling back to TCP tunnel: {e}");
                    true
                } else {
                    false
                }
            } else {
                true
            };

            if use_tunnel {
                let tunnel_data =
                    crate::transport::audio_codec::ProtobufAudioCodec::encode(audio);
                let tunnel = ControlMessage::UdpTunnel(tunnel_data);
                if self.outbound_tx.send(tunnel).await.is_err() {
                    error!("outbound channel closed");
                    break;
                }
            }
        }
    }
}

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
                let version_str = fancy_utils::version::fancy_version_string(fv);
                info!(fancy_version = %version_str, "server supports Fancy Mumble extensions");
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
            // Store the server's packet counters from the Ping reply.
            let to_client = crate::transport::ocb2::PacketStats {
                good: p.good.unwrap_or(0),
                late: p.late.unwrap_or(0),
                lost: p.lost.unwrap_or(0),
                resync: p.resync.unwrap_or(0),
            };
            if let Ok(mut stats) = state.server_packet_stats.lock() {
                stats.clone_from(&to_client);
            }
            let from_client = state
                .decrypt_stats
                .lock()
                .ok()
                .map(|s| s.clone())
                .unwrap_or_default();
            handler.on_ping_stats(from_client, to_client);
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

// -- UDP sender handle -----------------------------------------------

/// Lightweight handle for sending encrypted UDP packets.
struct UdpSender {
    socket: Arc<UdpSocket>,
    crypt: Ocb2CryptState,
}

impl UdpSender {
    /// Encrypt and send a pre-encoded UDP payload.
    async fn send_raw(&mut self, payload: &[u8]) -> Result<()> {
        let encrypted = self.crypt.encrypt(payload)?;
        let _n = self.socket
            .send(&encrypted)
            .await
            .map_err(Error::Io)?;
        Ok(())
    }

    /// Non-blocking encrypt + send. Returns `Ok(true)` when the packet
    /// was sent, `Ok(false)` when the OS buffer was full (packet dropped),
    /// or `Err` for other failures.
    fn try_send_raw(&mut self, payload: &[u8]) -> Result<bool> {
        let encrypted = self.crypt.encrypt(payload)?;
        match self.socket.try_send(&encrypted) {
            Ok(_) => Ok(true),
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(false),
            Err(e) => Err(Error::Io(e)),
        }
    }
}

/// Non-blocking drain of all pending outbound audio packets.
///
/// Called at the top of every event-loop iteration so audio is sent
/// Send a single outbound audio packet via UDP (preferred) or TCP tunnel
/// (fallback).  Fully non-blocking -- uses `try_send_raw` on the UDP
/// socket and `try_send` on the TCP channel, so this function **never
/// awaits** and cannot stall the event loop.
fn send_one_audio_packet(
    msg: &UdpMessage,
    udp_sender: &mut Option<UdpSender>,
    outbound_tx: &mpsc::Sender<ControlMessage>,
    counter: &mut u64,
) {
    let UdpMessage::Audio(audio) = msg else {
        return;
    };
    *counter += 1;
    if *counter == 1 || counter.is_multiple_of(500) {
        debug!(
            "event loop: outbound audio #{} (udp={})",
            counter,
            udp_sender.is_some(),
        );
    }

    let sent_udp = if let Some(sender) = udp_sender.as_mut() {
        let payload = crate::transport::udp::encode_udp_message(msg);
        match sender.try_send_raw(&payload) {
            Ok(true) => true,
            Ok(false) => {
                trace!("UDP send would block, falling back to TCP tunnel");
                false
            }
            Err(e) => {
                warn!("UDP audio send failed: {e}");
                false
            }
        }
    } else {
        false
    };

    if !sent_udp {
        let tunnel_data =
            crate::transport::audio_codec::ProtobufAudioCodec::encode(audio);
        let tunnel = ControlMessage::UdpTunnel(tunnel_data);
        if outbound_tx.try_send(tunnel).is_err() {
            warn!("TCP tunnel channel full, dropping audio packet");
        }
    }
}

/// Stored key material from the last `CryptSetup` so UDP can be
/// (re-)enabled at runtime when `force_tcp` is toggled off.
struct StoredCrypto {
    key: Vec<u8>,
    client_nonce: Vec<u8>,
    server_nonce: Vec<u8>,
}

/// Handle a `CryptSetup` message: extract keys and start the UDP transport.
#[allow(clippy::too_many_arguments, reason = "mirrors handle_control_message pattern; grouping would add indirection")]
async fn handle_crypt_setup<H: EventHandler>(
    cs: &mumble_tcp::CryptSetup,
    udp_config: &UdpConfig,
    force_tcp: bool,
    wq_sender: &WorkQueueSender,
    stored_crypto: &mut Option<StoredCrypto>,
    udp_sender: &mut Option<UdpSender>,
    udp_reader_task: &mut Option<tokio::task::JoinHandle<()>>,
    decrypt_stats: &crate::transport::ocb2::SharedPacketStats,
    handler: &mut H,
) {
    // Full key setup: key + client_nonce + server_nonce all present
    let (Some(key), Some(client_nonce), Some(server_nonce)) =
        (&cs.key, &cs.client_nonce, &cs.server_nonce)
    else {
        // Partial CryptSetup (nonce resync) - update decrypt nonce if we have a sender
        if let Some(sn) = &cs.server_nonce {
            debug!("CryptSetup nonce resync received");
            // Server nonce resync only affects the reader's decrypt state.
            // The reader task owns its own CryptState, so a full resync
            // isn't trivially possible without a channel/Arc<Mutex>. For now
            // log it; a future improvement could add a nonce update channel.
            let _ = sn;
        }
        return;
    };

    // Always store the crypto material so UDP can be enabled later.
    *stored_crypto = Some(StoredCrypto {
        key: key.clone(),
        client_nonce: client_nonce.clone(),
        server_nonce: server_nonce.clone(),
    });

    if force_tcp {
        info!("UDP disabled (force_tcp=true), using TCP tunnel for audio");
        handler.on_audio_transport_changed(false);
        return;
    }

    start_udp(
        key,
        client_nonce,
        server_nonce,
        udp_config,
        wq_sender,
        udp_sender,
        udp_reader_task,
        decrypt_stats,
        handler,
    )
    .await;
}

/// Handle a runtime `force_tcp` toggle from the UI.
#[allow(clippy::too_many_arguments, reason = "mirrors handle_crypt_setup pattern")]
async fn handle_force_tcp_change<H: EventHandler>(
    force_tcp: bool,
    stored_crypto: &Option<StoredCrypto>,
    udp_config: &UdpConfig,
    wq_sender: &WorkQueueSender,
    udp_sender: &mut Option<UdpSender>,
    udp_reader_task: &mut Option<tokio::task::JoinHandle<()>>,
    decrypt_stats: &crate::transport::ocb2::SharedPacketStats,
    handler: &mut H,
) {
    if force_tcp {
        // Tear down active UDP transport.
        if let Some(task) = udp_reader_task.take() {
            task.abort();
        }
        *udp_sender = None;
        info!("force_tcp enabled at runtime, switched to TCP tunnel");
        handler.on_audio_transport_changed(false);
    } else {
        // Re-enable UDP if we have stored crypto material.
        if let Some(crypto) = stored_crypto {
            start_udp(
                &crypto.key,
                &crypto.client_nonce,
                &crypto.server_nonce,
                udp_config,
                wq_sender,
                udp_sender,
                udp_reader_task,
                decrypt_stats,
                handler,
            )
            .await;
        } else {
            debug!("force_tcp disabled but no CryptSetup received yet; UDP will start when server sends keys");
        }
    }
}

/// Initialize the encrypted UDP transport and spawn the reader task.
#[allow(clippy::too_many_arguments, reason = "groups all transport handles needed to set up UDP")]
async fn start_udp<H: EventHandler>(
    key: &[u8],
    client_nonce: &[u8],
    server_nonce: &[u8],
    udp_config: &UdpConfig,
    wq_sender: &WorkQueueSender,
    udp_sender: &mut Option<UdpSender>,
    udp_reader_task: &mut Option<tokio::task::JoinHandle<()>>,
    decrypt_stats: &crate::transport::ocb2::SharedPacketStats,
    handler: &mut H,
) {

    // Initialize encrypt CryptState (for outbound audio)
    let mut encrypt_crypt = Ocb2CryptState::new();
    if let Err(e) = encrypt_crypt.set_key(key, client_nonce, server_nonce) {
        warn!("failed to initialize UDP encrypt crypto: {e}");
        return;
    }

    // Initialize decrypt CryptState (for inbound audio)
    let mut decrypt_crypt = Ocb2CryptState::new();
    if let Err(e) = decrypt_crypt.set_key(key, client_nonce, server_nonce) {
        warn!("failed to initialize UDP decrypt crypto: {e}");
        return;
    }

    // Connect UDP socket
    let transport = match UdpTransport::connect(udp_config, crate::transport::udp::PlaintextCryptState).await {
        Ok(t) => t,
        Err(e) => {
            warn!("failed to connect UDP socket: {e}");
            return;
        }
    };

    let socket = transport.socket_arc();

    // Abort any previous reader task
    if let Some(task) = udp_reader_task.take() {
        task.abort();
    }

    // Spawn UDP reader task
    let reader_socket = socket.clone();
    let reader_wq = wq_sender.clone();
    let reader_stats = decrypt_stats.clone();
    *udp_reader_task = Some(tokio::spawn(async move {
        udp_reader_loop(reader_socket, decrypt_crypt, reader_stats, reader_wq).await;
    }));

    // Store sender handle
    *udp_sender = Some(UdpSender {
        socket,
        crypt: encrypt_crypt,
    });

    // Send an initial encrypted UDP ping so the server discovers our
    // public UDP endpoint (NAT traversal).  Without this the server
    // has no address to forward audio to.
    if let Some(sender) = udp_sender.as_mut() {
        let payload = crate::transport::udp::encode_udp_message(&udp_ping_message());
        if let Err(e) = sender.send_raw(&payload).await {
            warn!("failed to send initial UDP ping: {e}");
        } else {
            debug!("sent initial UDP ping for NAT traversal");
        }
    }

    info!("UDP transport started with OCB2-AES128 encryption");
    handler.on_audio_transport_changed(true);
}

/// Build a timestamped UDP ping message.
fn udp_ping_message() -> UdpMessage {
    use crate::proto::mumble_udp;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    UdpMessage::Ping(mumble_udp::Ping {
        timestamp,
        ..Default::default()
    })
}

/// Background task: reads encrypted UDP datagrams, decrypts, decodes, and
/// feeds them into the work queue.
async fn udp_reader_loop(
    socket: Arc<UdpSocket>,
    mut crypt: Ocb2CryptState,
    shared_stats: crate::transport::ocb2::SharedPacketStats,
    wq_sender: WorkQueueSender,
) {
    let mut buf = vec![0u8; 1024];
    loop {
        let n = match socket.recv(&mut buf).await {
            Ok(n) if n > 0 => n,
            Ok(_) => {
                debug!("UDP socket closed");
                break;
            }
            Err(e) => {
                // On Windows, ICMP port-unreachable can cause recv to
                // return ConnectionReset - this is normal if the server
                // hasn't opened UDP yet. Just retry.
                if e.kind() == std::io::ErrorKind::ConnectionReset {
                    continue;
                }
                warn!("UDP read error: {e}");
                break;
            }
        };

        let decrypted = match crypt.decrypt(&buf[..n]) {
            Ok(data) => data,
            Err(e) => {
                warn!("UDP decrypt failed, skipping: {e}");
                continue;
            }
        };

        // Publish updated counters after each decrypt attempt.
        if let Ok(mut stats) = shared_stats.lock() {
            stats.clone_from(&crypt.stats);
        }

        match crate::transport::udp::decode_udp_message(&decrypted) {
            Ok(msg) => {
                if wq_sender.send_udp(msg).await.is_err() {
                    break;
                }
            }
            Err(e) => {
                warn!("UDP decode failed, skipping: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mumble_version_v1_encoding() {
        let v = MumbleVersion::new(1, 6, 0);
        // (1 << 16) | (6 << 8) | 0 = 0x0001_0600
        assert_eq!(v.encode_v1(), 0x0001_0600);
    }

    #[test]
    fn mumble_version_v2_encoding() {
        let v = MumbleVersion::new(1, 6, 0);
        assert_eq!(v.encode_v2(), (1u64 << 48) | (6u64 << 32));
    }

    #[test]
    fn mumble_version_display() {
        assert_eq!(MumbleVersion::new(1, 6, 0).to_string(), "1.6.0");
        assert_eq!(MumbleVersion::new(1, 5, 1).to_string(), "1.5.1");
    }

    #[test]
    fn mumble_version_default_is_1_6_0() {
        let v = MumbleVersion::default();
        assert_eq!(v, MumbleVersion::new(1, 6, 0));
    }

    #[test]
    fn mumble_version_v1_v2_consistency() {
        // v1 and v2 should encode the same version, just different bit layout
        let v = MumbleVersion::new(1, 5, 0);
        assert_eq!(v.encode_v1(), (1 << 16) | (5 << 8));
        assert_eq!(v.encode_v2(), 0x0001_0005_0000_0000);
    }
}
