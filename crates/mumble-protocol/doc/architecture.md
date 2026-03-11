# Architecture Overview

## 1. Purpose

`mumble-protocol` is a fully asynchronous Rust library that implements the
[Mumble](https://wiki.mumble.info/wiki/Protocol) voice-chat protocol.
It provides a high-level client API suitable for building desktop/mobile
Mumble clients, bots, and bridges without exposing raw protocol details.

## 2. Design Principles

| Principle | How it is applied |
|-----------|-------------------|
| **Async-first** | Built on `tokio`; every I/O operation is non-blocking. |
| **Separation of concerns** | Each module (transport, audio, command, state) has a single responsibility. |
| **Command pattern** | User actions are self-contained command structs - adding a feature never touches the event loop. |
| **Trait abstraction** | Audio capture, playback, encoding, decoding, and filtering are traits; platform code is kept outside the library. |
| **Priority scheduling** | A biased `select!` work queue ensures audio packets are always processed before control messages or user commands. |
| **Protobuf wire format** | All Mumble messages are Protobuf-encoded with a `[type:u16][length:u32][payload]` TCP framing. |

## 3. High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                         Application / UI                            │
│                   (mumble-gui / mumble-tauri)                       │
└───────────────────────────┬─────────────────────────────────────────┘
                            │  ClientHandle::send(cmd)
                            ▼
┌─────────────────────────────────────────────────────────────────────┐
│                       mumble-protocol                               │
│  ┌──────────┐  ┌──────────────┐  ┌────────────┐  ┌──────────────┐  │
│  │ Command  │  │  Client      │  │   Audio    │  │  Transport   │  │
│  │ Pattern  │  │  Event Loop  │  │  Pipeline  │  │  TCP / UDP   │  │
│  └────┬─────┘  └──────┬───────┘  └─────┬──────┘  └──────┬───────┘  │
│       │               │               │                │           │
│       │         ┌─────┴──────┐        │          ┌─────┴──────┐   │
│       │         │ Work Queue │◄───────┘          │  Codec     │   │
│       │         │ (priority) │                   │  (framing) │   │
│       │         └─────┬──────┘                   └────────────┘   │
│       │               │                                           │
│       │         ┌─────┴──────┐                                    │
│       └────────►│ Server     │                                    │
│                 │ State      │                                    │
│                 └────────────┘                                    │
└─────────────────────────────────────────────────────────────────────┘
                            │
                     TLS / UDP
                            ▼
                   ┌────────────────┐
                   │  Mumble Server │
                   └────────────────┘
```

## 4. Module Map

```
mumble-protocol/src/
├── lib.rs              # Crate root - re-exports all public modules
├── client.rs           # Client orchestrator (event loop, spawned tasks)
├── error.rs            # Error enum and Result alias
├── event.rs            # EventHandler trait for server callbacks
├── message.rs          # ControlMessage / UdpMessage / TcpMessageType enums
├── state.rs            # ServerState: User, Channel, ConnectionInfo
├── work_queue.rs       # Priority work queue (UDP > TCP > commands)
│
├── transport/
│   ├── mod.rs          # Re-exports
│   ├── tcp.rs          # TLS TCP transport (TcpTransport, TcpReader, TcpWriter)
│   ├── udp.rs          # UDP transport with CryptState trait
│   ├── codec.rs        # TCP wire-format framing (encode/decode)
│   └── audio_codec.rs  # Legacy & protobuf audio packet codec
│
├── command/
│   ├── mod.rs          # CommandAction trait, CommandOutput, re-exports
│   ├── authenticate.rs # Authenticate command
│   ├── ban_user.rs     # BanUser command
│   ├── channel_listen.rs # ChannelListen command
│   ├── disconnect.rs   # Disconnect command
│   ├── join_channel.rs # JoinChannel command
│   ├── kick_user.rs    # KickUser command
│   ├── request_ban_list.rs # RequestBanList command
│   ├── request_user_stats.rs # RequestUserStats command
│   ├── send_audio.rs   # SendAudio command
│   ├── send_text_message.rs # SendTextMessage command
│   ├── set_comment.rs  # SetComment command
│   ├── set_self_deaf.rs # SetSelfDeaf command
│   ├── set_self_mute.rs # SetSelfMute command
│   └── set_voice_target.rs # SetVoiceTarget command
│
├── audio/
│   ├── mod.rs          # Pipeline architecture overview
│   ├── sample.rs       # AudioFrame, AudioFormat, SampleFormat
│   ├── capture.rs      # AudioCapture trait + SilentCapture
│   ├── playback.rs     # AudioPlayback trait + NullPlayback
│   ├── encoder.rs      # AudioEncoder trait + OpusEncoder
│   ├── decoder.rs      # AudioDecoder trait + OpusDecoder
│   ├── pipeline.rs     # OutboundPipeline / InboundPipeline
│   └── filter/
│       ├── mod.rs      # AudioFilter trait + FilterChain
│       ├── noise_gate.rs    # NoiseGate (voice-activity gating)
│       ├── automatic_gain.rs # AutomaticGainControl (AGC)
│       ├── denoiser.rs      # SpectralDenoiser (ML stub)
│       └── volume.rs        # VolumeFilter (linear gain)
│
└── proto/
    ├── mod.rs          # Re-exports generated code
    ├── mumble_proto.rs # Generated from Mumble.proto (TCP messages)
    └── mumble_udp.rs   # Generated from MumbleUDP.proto (UDP messages)
```

## 5. Key Abstractions

### 5.1 Client Orchestrator (`client.rs`)

The entry point is `client::run<H: EventHandler>(config, handler)` which:

1. Connects TCP (TLS) to the server and sends a `Version` message.
2. Splits the TCP stream into independent `TcpReader` / `TcpWriter`.
3. Creates the priority work queue.
4. Spawns concurrent tasks:
   - **TCP reader** → feeds `ControlMessage`s into the work queue.
   - **Ping timer** → sends periodic keep-alive pings via the outbound channel.
   - **Command forwarder** → relays external commands into the work queue.
5. Enters the main dispatch loop that drains the work queue.
6. Returns a `ClientHandle` for submitting commands from the outside.

### 5.2 Priority Work Queue (`work_queue.rs`)

Three independent `mpsc` channels feed into a single consumer using a
**biased `tokio::select!`**:

| Priority | Source | Channel |
|----------|--------|---------|
| 1 (highest) | UDP transport | `udp_rx` |
| 2 | TCP transport | `tcp_rx` |
| 3 (lowest) | User commands | `cmd_rx` |

This guarantees that time-sensitive audio packets are never starved by
control traffic or user-initiated actions.

### 5.3 Transport Layer (`transport/`)

| Component | Role |
|-----------|------|
| `TcpTransport` | TLS 1.2+ connection; sends/receives framed `ControlMessage`s. |
| `TcpReader` / `TcpWriter` | Split halves for concurrent read/write in separate tasks. |
| `UdpTransport<C: CryptState>` | Datagram socket with pluggable OCB2-AES128 encryption. |
| `codec` | Encodes/decodes the `[type:u16][length:u32][payload]` TCP wire format. |
| `audio_codec` | Legacy binary audio codec (Mumble < 1.5) and protobuf-based audio (Mumble 1.5+). |

### 5.4 Command Pattern (`command/`)

Every user action is a struct implementing `CommandAction`:

```rust
pub trait CommandAction: Debug + Send + Sync + 'static {
    fn execute(&self, state: &ServerState) -> CommandOutput;
}
```

`CommandOutput` carries:
- `tcp_messages: Vec<ControlMessage>` - messages to send over TCP.
- `udp_messages: Vec<UdpMessage>` - messages to send over UDP.
- `disconnect: bool` - whether to shut down the client.

Adding a new command requires only a new file + struct.

### 5.5 Audio Pipeline (`audio/`)

The pipeline is split into **outbound** (capture → network) and **inbound**
(network → playback) directions. Each direction is a linear chain of
trait-based stages:

**Outbound**: `AudioCapture → FilterChain → AudioEncoder → Network`
**Inbound**: `Network → AudioDecoder → FilterChain → AudioPlayback`

All stages communicate through `AudioFrame` - a timestamped PCM buffer
with embedded format metadata.

### 5.6 Server State (`state.rs`)

`ServerState` aggregates:
- `users: HashMap<u32, User>` - connected users keyed by session ID.
- `channels: HashMap<u32, Channel>` - channel tree keyed by channel ID.
- `connection: ConnectionInfo` - session ID, bandwidth cap, welcome text.

State is updated incrementally by `apply_user_state`, `apply_channel_state`,
and `apply_server_sync`.

### 5.7 Event Handler (`event.rs`)

```rust
pub trait EventHandler: Send + 'static {
    fn on_control_message(&mut self, msg: &ControlMessage) {}
    fn on_udp_message(&mut self, msg: &UdpMessage) {}
    fn on_connected(&mut self) {}
    fn on_disconnected(&mut self) {}
}
```

All methods are defaulted to no-ops so consumers only override what they need.

## 6. External Dependencies

| Dependency | Purpose |
|------------|---------|
| `tokio` | Async runtime, channels, timers |
| `tokio-rustls` / `rustls` | TLS for the TCP control channel |
| `prost` | Protobuf encoding/decoding |
| `bytes` | Efficient byte buffer management |
| `opus` (feature-gated) | Opus audio codec |
| `thiserror` | Ergonomic error derivation |
| `tracing` | Structured logging |

## 7. Thread / Task Model

```
┌────────────────────────────────────────────────────────┐
│ tokio runtime                                          │
│                                                        │
│  Task 1: TCP Reader       ──► wq_sender.send_tcp()     │
│  Task 2: Ping Timer       ──► outbound_tx.send()       │
│  Task 3: Command Fwd      ──► wq_sender.send_command() │
│  Task 4: TCP Writer        ◄── outbound_rx.recv()      │
│  Task 5: Main Event Loop   ◄── wq_receiver.recv()      │
│           ├─ updates ServerState                       │
│           ├─ calls EventHandler                        │
│           └─ sends outbound messages                   │
│                                                        │
│  (Optional)                                            │
│  Task 6: UDP Reader       ──► wq_sender.send_udp()     │
└────────────────────────────────────────────────────────┘
```

All tasks communicate exclusively through `mpsc` channels - there is no
shared mutable state and no locks in the hot path.
