# Detailed Design - mumble-protocol

This document provides an in-depth description of every module, data
structure, trait, and interaction within the `mumble-protocol` crate.

---

## Table of Contents

1. [Error Handling](#1-error-handling)
2. [Message Types](#2-message-types)
3. [Transport Layer](#3-transport-layer)
   - 3.1 [TCP Transport](#31-tcp-transport)
   - 3.2 [TCP Codec (Wire Framing)](#32-tcp-codec-wire-framing)
   - 3.3 [UDP Transport](#33-udp-transport)
   - 3.4 [Audio Codec](#34-audio-codec)
4. [Server State](#4-server-state)
5. [Event Handler](#5-event-handler)
6. [Priority Work Queue](#6-priority-work-queue)
7. [Command Pattern](#7-command-pattern)
   - 7.1 [Core Trait & Output](#71-core-trait--output)
   - 7.2 [Concrete Commands](#72-concrete-commands)
8. [Audio Pipeline](#8-audio-pipeline)
   - 8.1 [Sample Types](#81-sample-types)
   - 8.2 [Capture](#82-capture)
   - 8.3 [Playback](#83-playback)
   - 8.4 [Encoder](#84-encoder)
   - 8.5 [Decoder](#85-decoder)
   - 8.6 [Filters](#86-filters)
   - 8.7 [Pipelines](#87-pipelines)
9. [Client Orchestrator](#9-client-orchestrator)
   - 9.1 [Configuration](#91-configuration)
   - 9.2 [Startup Sequence](#92-startup-sequence)
   - 9.3 [Task Model](#93-task-model)
   - 9.4 [Event Loop Dispatch](#94-event-loop-dispatch)
   - 9.5 [Shutdown](#95-shutdown)
10. [Protobuf Definitions](#10-protobuf-definitions)
11. [Design Decisions & Trade-offs](#11-design-decisions--trade-offs)

---

## 1. Error Handling

**File:** `src/error.rs`

A single `Error` enum covers all failure modes in the library:

| Variant | Source | Description |
|---------|--------|-------------|
| `Io(io::Error)` | `From` | Underlying I/O failure (TCP/UDP) |
| `Tls(rustls::Error)` | `From` | TLS handshake or protocol-level failure |
| `Decode(prost::DecodeError)` | `From` | Protobuf deserialization failure |
| `Encode(prost::EncodeError)` | `From` | Protobuf serialization failure |
| `UnknownMessageType(u16)` | - | Received a TCP message with an unrecognized type ID |
| `Rejected(String)` | - | Server rejected the connection (wrong password, etc.) |
| `ConnectionClosed` | - | The TCP/UDP connection was closed cleanly |
| `QueueClosed` | - | All senders of a work-queue channel were dropped |
| `InvalidState(String)` | - | Logic error or unexpected protocol state |
| `OpusCodec(String)` | - | Opus encoder or decoder failure |

The library defines `pub type Result<T> = std::result::Result<T, Error>;` for
convenience.

All error variants derive `thiserror::Error` for automatic `Display` and
`From` implementations.

---

## 2. Message Types

**File:** `src/message.rs`

### 2.1 TcpMessageType

An enum with `#[repr(u16)]` that maps each Mumble TCP message to its
numeric type ID (0–26). Used by the codec for framing. Implements
`TryFrom<u16>` so that invalid IDs produce `Error::UnknownMessageType`.

### 2.2 ControlMessage

A Rust enum with one variant per TCP message type. Each variant wraps the
corresponding `prost`-generated struct from `proto::mumble_tcp`. The
special variant `UdpTunnel(Vec<u8>)` holds raw bytes because tunnelled
audio is not a single protobuf message but the raw UDP packet payload.

### 2.3 UdpMessage

```rust
pub enum UdpMessage {
    Audio(mumble_udp::Audio),
    Ping(mumble_udp::Ping),
}
```

Represents a decoded UDP datagram - either Opus audio or a UDP ping.

### 2.4 ServerMessage

A unified wrapper:

```rust
pub enum ServerMessage {
    Control(ControlMessage),
    Udp(UdpMessage),
}
```

This is the type stored inside `WorkItem::ServerMessage` so the event loop
handles both transports uniformly.

---

## 3. Transport Layer

**Directory:** `src/transport/`

### 3.1 TCP Transport

**File:** `src/transport/tcp.rs`

#### TcpConfig

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `server_host` | `String` | `"localhost"` | Hostname or IP of the Mumble server |
| `server_port` | `u16` | `64738` | TCP port |
| `accept_invalid_certs` | `bool` | `true` | Accept self-signed TLS certificates |

#### TcpTransport

Wraps a `TlsStream<TcpStream>` from `tokio-rustls`. Provides:

- **`connect(config)`** - Establishes a TCP connection, performs TLS
  handshake, returns `Self`.
- **`send(msg)`** - Encodes a `ControlMessage` using the codec and
  writes the framed bytes to the TLS stream.
- **`recv()`** - Reads from the TLS stream into an internal `BytesMut`
  buffer, attempts to decode a complete frame. Blocks until a full
  message is available.
- **`split()`** - Consumes `self` and returns `(TcpReader, TcpWriter)`,
  each owning one half of the split TLS stream.

#### TcpReader / TcpWriter

Independent halves that can be moved into separate `tokio::spawn` tasks
for concurrent read/write without synchronization.

#### TLS Configuration

- When `accept_invalid_certs` is `true`, an `InsecureVerifier` is used
  that accepts all server certificates (necessary for the vast majority
  of Mumble servers which use self-signed certificates).
- When `false`, the system root certificate store (via `webpki-roots`) is
  used for proper certificate verification.

### 3.2 TCP Codec (Wire Framing)

**File:** `src/transport/codec.rs`

Mumble TCP messages are framed as:

```
┌────────────┬─────────────┬──────────────────┐
│ type: u16  │ length: u32 │ payload: [u8]    │
│ (big-end.) │ (big-end.)  │ (protobuf bytes) │
└────────────┴─────────────┴──────────────────┘
```

- **Header size:** 6 bytes (2 + 4).
- **Maximum payload:** 8 MiB (`MAX_PAYLOAD_SIZE`).
- **`encode(msg)`** - Serializes the `ControlMessage` variant to protobuf
  bytes, prepends the 6-byte header, returns `Vec<u8>`.
- **`decode(buf)`** - Attempts to parse one frame from a `BytesMut`.
  Returns `Ok(None)` if there are not enough bytes yet (partial frame),
  `Ok(Some(msg))` on success, or `Err` on protocol violations.

Internally uses `serialize_control_message` and
`deserialize_control_message` match arms that cover all 27 message types.

### 3.3 UDP Transport

**File:** `src/transport/udp.rs`

#### CryptState Trait

```rust
pub trait CryptState: Send + Sync {
    fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>>;
    fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>>;
    fn is_initialized(&self) -> bool;
}
```

Abstracts the OCB2-AES128 encryption used by Mumble UDP. The encryption
keys are delivered via `CryptSetup` on the TCP channel. A
`PlaintextCryptState` is provided for testing.

#### UdpTransport\<C: CryptState>

- **`connect(config, crypt)`** - Resolves the server hostname via tokio's
  async DNS resolver, binds a local UDP socket, and calls
  `socket.connect(server_addr)`.
- **`send(msg)`** - Encodes the `UdpMessage` to bytes, encrypts via
  `CryptState`, and sends the datagram.
- **`recv()`** - Receives a datagram, decrypts, decodes, and returns the
  `UdpMessage`. Skips packets that fail decryption or decoding.
- **`set_crypt_state(crypt)`** - Hot-swaps the crypto state (e.g. after
  a key renegotiation via `CryptSetup`).

#### UDP Wire Format

A single-byte type marker prefix:
- `0x20` - Ping (followed by protobuf `mumble_udp::Ping`).
- `0x80`+ - Audio (followed by protobuf `mumble_udp::Audio`).

Maximum datagram size: 1024 bytes (`MAX_UDP_SIZE`).

### 3.4 Audio Codec

**File:** `src/transport/audio_codec.rs`

Handles both **legacy** (pre-1.5) and **protobuf** (1.5+) audio encoding.

#### Legacy Format

```
┌──────────┬──────────┬──────────┬────────────────┬────────────┐
│ header   │ session  │ sequence │ opus len+term  │ opus data  │
│ 1 byte   │ varint   │ varint   │ varint         │ N bytes    │
│(type|tgt)│(srv→cli) │          │ len|terminator │            │
└──────────┴──────────┴──────────┴────────────────┴────────────┘
```

- **header**: `(audio_type << 5) | target`. Type 4 = Opus.
- **session**: Only present in server → client direction.
- **sequence**: Monotonically increasing frame counter.
- **opus len**: Bottom 13 bits = payload length, bit 13 = terminator flag.

Uses **Mumble-style varints** (NOT protobuf LEB128). Encoding widths:
7-bit (1 byte), 14-bit (2 bytes), 21-bit (3 bytes), 28-bit (4 bytes),
32-bit (5 bytes), 64-bit (9 bytes).

#### Protobuf Format

Straightforward `prost::Message::encode_to_vec()` /
`prost::Message::decode()` of `mumble_udp::Audio`.

#### Tunnel Audio

`decode_tunnel_audio(data)` first tries protobuf decoding, then falls
back to legacy binary decoding. This allows the library to interoperate
with both old and new Mumble servers transparently.

---

## 4. Server State

**File:** `src/state.rs`

### ServerState

Central aggregation of all server-side entities:

```rust
pub struct ServerState {
    pub connection: ConnectionInfo,
    pub users: HashMap<u32, User>,
    pub channels: HashMap<u32, Channel>,
}
```

#### ConnectionInfo

Populated during handshake from `ServerSync`:

| Field | Type | Description |
|-------|------|-------------|
| `session_id` | `Option<u32>` | Our own session ID on the server |
| `max_bandwidth` | `Option<u32>` | Server-enforced bandwidth cap |
| `welcome_text` | `Option<String>` | Server welcome message (HTML) |

#### User

One entry per connected user, keyed by `session: u32`:

| Field | Type | Description |
|-------|------|-------------|
| `session` | `u32` | Unique session ID |
| `name` | `String` | Display name |
| `channel_id` | `u32` | Current channel |
| `mute` / `deaf` | `bool` | Server-enforced mute/deaf |
| `self_mute` / `self_deaf` | `bool` | User-initiated mute/deaf |
| `comment` | `String` | User comment (HTML) |
| `hash` | `String` | Certificate hash |

#### Channel

One entry per channel, keyed by `channel_id: u32`:

| Field | Type | Description |
|-------|------|-------------|
| `channel_id` | `u32` | Unique channel ID |
| `parent_id` | `Option<u32>` | Parent channel (forms a tree) |
| `name` | `String` | Display name |
| `description` | `String` | Channel description (HTML) |
| `position` | `i32` | Sort order |
| `temporary` | `bool` | Auto-deleted when empty |
| `max_users` | `u32` | 0 = unlimited |

#### State Update Methods

All state updates are **incremental** (partial updates):

- `apply_user_state(&UserState)` - Creates or updates a user. Only fields
  present in the protobuf message are overwritten.
- `apply_channel_state(&ChannelState)` - Creates or updates a channel.
- `apply_server_sync(&ServerSync)` - Records session ID, bandwidth, welcome.
- `remove_user(session)` - Removes user from the map.
- `remove_channel(channel_id)` - Removes channel from the map.
- `own_session()` - Returns our session ID (if handshake is complete).

---

## 5. Event Handler

**File:** `src/event.rs`

```rust
pub trait EventHandler: Send + 'static {
    fn on_control_message(&mut self, msg: &ControlMessage) {}
    fn on_udp_message(&mut self, msg: &UdpMessage) {}
    fn on_connected(&mut self) {}
    fn on_disconnected(&mut self) {}
}
```

**Design rationale:**
- All methods have default no-op implementations, so consumers override
  only the callbacks they need.
- `Send + 'static` bounds are required because the handler is moved into
  a `tokio::spawn` task.
- `&mut self` allows handlers to maintain internal state (e.g. recording
  events, updating UI state).

**Built-in implementations:**
- `NoopEventHandler` - Does nothing. Useful for headless/bot clients.

**Lifecycle:**
1. `on_control_message` / `on_udp_message` - Called for every inbound
   message during the event loop.
2. `on_connected` - Called exactly once when `ServerSync` is processed.
3. `on_disconnected` - Called on clean shutdown or connection loss.

---

## 6. Priority Work Queue

**File:** `src/work_queue.rs`

### Architecture

Three independent `tokio::sync::mpsc` channels with a unified consumer:

```
UDP sender  ──► udp_tx ──► udp_rx ──┐
TCP sender  ──► tcp_tx ──► tcp_rx ──┼──► biased select! ──► WorkItem
Cmd sender  ──► cmd_tx ──► cmd_rx ──┘
```

### WorkItem

```rust
pub enum WorkItem {
    ServerMessage(ServerMessage),
    UserCommand(BoxedCommand),
    Ping,
    Shutdown,
}
```

### Priority Semantics

The `WorkQueueReceiver::recv()` method uses `tokio::select! { biased; ... }`
to always check channels in this order:

1. **UDP** (audio) - highest priority. Audio must never be delayed by
   control messages or user actions.
2. **TCP** (control) - medium priority. Server state updates.
3. **Commands** - lowest priority. User-initiated actions.

If all channels are closed, `WorkItem::Shutdown` is returned.

### WorkQueueSender

Cloneable handle with three typed methods:
- `send_udp(UdpMessage)` - used by the UDP reader task.
- `send_tcp(ControlMessage)` - used by the TCP reader task.
- `send_command(BoxedCommand)` - used by the command forwarder.

### Factory

```rust
pub fn create() -> (WorkQueueSender, WorkQueueReceiver)
```

Creates all three channel pairs with configurable buffer sizes (default:
UDP=256, TCP=128, Commands=32).

---

## 7. Command Pattern

**Directory:** `src/command/`

### 7.1 Core Trait & Output

```rust
pub trait CommandAction: Debug + Send + Sync + 'static {
    fn execute(&self, state: &ServerState) -> CommandOutput;
}

pub struct CommandOutput {
    pub tcp_messages: Vec<ControlMessage>,
    pub udp_messages: Vec<UdpMessage>,
    pub disconnect: bool,
}

pub type BoxedCommand = Box<dyn CommandAction>;
```

**Design rationale:**
- Each command is a standalone, self-describing struct.
- Commands receive a **read-only** view of `ServerState` (e.g. to read
  their own session ID).
- The event loop is a pure dispatcher - it never needs to know about
  specific command types.
- Adding a new command = adding one file; **no existing code changes**.

### 7.2 Concrete Commands

| Command | Input Fields | Produces |
|---------|-------------|----------|
| `Authenticate` | `username`, `password?`, `tokens` | TCP: `Authenticate` msg |
| `JoinChannel` | `channel_id` | TCP: `UserState` (session + channel) |
| `SendTextMessage` | `channel_ids`, `user_sessions`, `tree_ids`, `message` | TCP: `TextMessage` |
| `SendAudio` | `opus_data`, `target`, `frame_number`, `positional_data?`, `is_terminator` | UDP: `Audio` msg |
| `SetSelfMute` | `muted: bool` | TCP: `UserState` |
| `SetSelfDeaf` | `deafened: bool` | TCP: `UserState` |
| `SetComment` | `comment: String` | TCP: `UserState` |
| `Disconnect` | (none) | Sets `disconnect = true` |
| `KickUser` | `session`, `reason` | TCP: `UserRemove` |
| `BanUser` | `session`, `reason`, `duration` | TCP: `BanList` |
| `ChannelListen` | `channel_id`, `listen: bool` | TCP: `UserState` |
| `RequestBanList` | (none) | TCP: `BanList` (query) |
| `RequestUserStats` | `session` | TCP: `UserStats` |
| `SetVoiceTarget` | `id`, `entries: Vec<VoiceTargetEntry>` | TCP: `VoiceTarget` |

#### VoiceTargetEntry

Sub-structure for `SetVoiceTarget`:

| Field | Type | Description |
|-------|------|-------------|
| `sessions` | `Vec<u32>` | Target user sessions |
| `channel_id` | `Option<u32>` | Target channel |
| `children` | `bool` | Include child channels |
| `links` | `bool` | Include linked channels |
| `group` | `Option<String>` | Target group name |

---

## 8. Audio Pipeline

**Directory:** `src/audio/`

### 8.1 Sample Types

**File:** `src/audio/sample.rs`

#### SampleFormat

```rust
pub enum SampleFormat { I16, F32 }
```

- `I16` - 16-bit signed integer. Native Opus input.
- `F32` - 32-bit float, normalized to `[-1.0, 1.0]`. Preferred by filters.
- `byte_width()` returns 2 or 4 respectively.

#### AudioFormat

```rust
pub struct AudioFormat {
    pub sample_rate: u32,    // Typically 48000
    pub channels: u16,       // 1 (mono) or 2 (stereo)
    pub sample_format: SampleFormat,
}
```

Pre-defined constants: `MONO_48KHZ_F32`, `MONO_48KHZ_I16`.

#### AudioFrame

```rust
pub struct AudioFrame {
    pub data: Vec<u8>,       // Raw PCM bytes (interleaved)
    pub format: AudioFormat,
    pub sequence: u64,       // Monotonic frame counter
    pub is_silent: bool,     // Set by noise gate
}
```

Every pipeline stage speaks in terms of `AudioFrame`. This is the **lingua
franca** that keeps all stages decoupled.

Helper methods:
- `samples_per_channel()` - Computes sample count from `data.len()` and format.
- `as_f32_samples()` / `as_f32_samples_mut()` - Reinterpret data as `&[f32]`.

### 8.2 Capture

**File:** `src/audio/capture.rs`

```rust
pub trait AudioCapture: Send + 'static {
    fn format(&self) -> AudioFormat;
    fn read_frame(&mut self) -> Result<AudioFrame>;
    fn start(&mut self) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
}
```

Abstracts the OS audio input device. Implementations interact with
WASAPI (Windows), PulseAudio (Linux), or CoreAudio (macOS).

**Built-in:** `SilentCapture` - produces silent frames at a given rate.
Useful for testing and bots.

### 8.3 Playback

**File:** `src/audio/playback.rs`

```rust
pub trait AudioPlayback: Send + 'static {
    fn format(&self) -> AudioFormat;
    fn write_frame(&mut self, frame: &AudioFrame) -> Result<()>;
    fn start(&mut self) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
}
```

Abstracts the OS audio output device. Mirror of `AudioCapture`.

**Built-in:** `NullPlayback` - discards all audio (testing/bots).

### 8.4 Encoder

**File:** `src/audio/encoder.rs`

```rust
pub trait AudioEncoder: Send + 'static {
    fn input_format(&self) -> AudioFormat;
    fn encode(&mut self, frame: &AudioFrame) -> Result<EncodedPacket>;
    fn reset(&mut self);
}
```

#### EncodedPacket

```rust
pub struct EncodedPacket {
    pub data: Vec<u8>,          // Compressed payload
    pub sequence: u64,          // Frame sequence number
    pub frame_samples: u32,     // Duration in samples
}
```

#### OpusEncoder (feature-gated: `opus-codec`)

Wraps `opus::Encoder` with:

| Config Field | Default | Description |
|-------------|---------|-------------|
| `bitrate` | 72,000 bps | Target bitrate |
| `frame_size` | 960 (20ms @ 48kHz) | Samples per channel per frame |
| `application` | `Voip` | Opus application mode |
| `vbr` | `true` | Variable bitrate |
| `complexity` | 5 | Encoder complexity (0–10) |
| `fec` | `true` | Forward error correction |
| `packet_loss_percent` | 10 | Expected packet loss % |
| `dtx` | `false` | Discontinuous transmission |

DTX is **disabled by default** because it can cause robotic artifacts
when the signal is near the noise-gate threshold.

### 8.5 Decoder

**File:** `src/audio/decoder.rs`

```rust
pub trait AudioDecoder: Send + 'static {
    fn output_format(&self) -> AudioFormat;
    fn decode(&mut self, packet: &EncodedPacket) -> Result<AudioFrame>;
    fn decode_lost(&mut self) -> Result<AudioFrame>;
    fn reset(&mut self);
}
```

`decode_lost()` generates a **packet loss concealment** (PLC) frame.
The default produces silence; the Opus implementation uses the codec's
built-in PLC which interpolates from previous state.

#### OpusDecoder (feature-gated: `opus-codec`)

Wraps `opus::Decoder`. Pre-allocates a decode buffer sized for the
maximum Opus frame duration (120ms). Produces 48 kHz mono or stereo F32.

### 8.6 Filters

**File:** `src/audio/filter/mod.rs`

```rust
pub trait AudioFilter: Send + 'static {
    fn name(&self) -> &str;
    fn process(&mut self, frame: &mut AudioFrame) -> Result<()>;
    fn reset(&mut self);
    fn is_enabled(&self) -> bool;
    fn set_enabled(&mut self, enabled: bool);
}
```

Filters process audio **in-place** - no allocations in the hot path.

#### FilterChain

An ordered `Vec<Box<dyn AudioFilter>>` that runs each filter sequentially.
Disabled filters are skipped. Provides `process()`, `reset()`, `push()`.

#### Concrete Filters

| Filter | File | Description | Key Config |
|--------|------|-------------|------------|
| `NoiseGate` | `noise_gate.rs` | Voice-activity gating with hysteresis | `open_threshold`, `close_threshold`, `hold_frames`, `attack_samples`, `release_samples` |
| `AutomaticGainControl` | `automatic_gain.rs` | Envelope-follower AGC normalizing signal level | `target_level`, `max_gain`, `min_gain`, `attack`, `release` |
| `SpectralDenoiser` | `denoiser.rs` | ML-based noise suppression (stub/passthrough) | `attenuation` |
| `VolumeFilter` | `volume.rs` | Linear gain (software volume knob) | `gain` (0.0–10.0) |

**Recommended filter chain order** (outbound):
1. `NoiseGate` - cheapest filter; silences quiet frames early.
2. `AutomaticGainControl` - normalizes speech level.
3. `SpectralDenoiser` - removes remaining background noise.
4. `VolumeFilter` - final user-controlled volume.

### 8.7 Pipelines

**File:** `src/audio/pipeline.rs`

#### OutboundPipeline

Owns: `AudioCapture` + `FilterChain` + `AudioEncoder`.

```
capture.read_frame()
    → filter_chain.process(&mut frame)
    → encoder.encode(&frame) → EncodedPacket
```

`tick()` returns one of:

| Variant | Meaning | Caller action |
|---------|---------|---------------|
| `Audio(packet)` | Speech frame | Send with `is_terminator = false` |
| `Terminator(packet)` | Last speech frame | Send with `is_terminator = true`, then stop |
| `Silence` | Gate active | Keep draining capture, don't send |
| `NoData` | Buffer empty | Stop draining this tick |

Internally tracks `was_talking` to detect speech → silence transitions
and emit terminators.

#### InboundPipeline

Owns: `AudioDecoder` + `FilterChain` + `AudioPlayback`.

```
decoder.decode(&packet)
    → filter_chain.process(&mut frame)
    → playback.write_frame(&frame)
```

- `tick(packet)` - Decode, filter, play one packet.
- `handle_packet_loss()` - Generate a PLC frame via `decoder.decode_lost()`.

---

## 9. Client Orchestrator

**File:** `src/client.rs`

### 9.1 Configuration

```rust
pub struct ClientConfig {
    pub tcp: TcpConfig,
    pub udp: UdpConfig,
    pub ping_interval: Duration,  // default: 15s
}
```

### 9.2 Startup Sequence

`client::run(config, handler)`:

1. **TCP connect** - `TcpTransport::connect(&config.tcp)`.
2. **Send Version** - Immediately sends a `Version` message advertising
   protocol v1.5.0 (both legacy `version_v1` and new `version_v2` fields).
3. **Split stream** - `tcp.split()` → `(TcpReader, TcpWriter)`.
4. **Create work queue** - `work_queue::create()`.
5. **Create ClientHandle** - External command submission channel.
6. **Spawn event loop** - `tokio::spawn(event_loop(...))`.
7. **Return** - `(ClientHandle, JoinHandle)`.

### 9.3 Task Model

| Task | Responsibility | Communication |
|------|---------------|---------------|
| **TCP Reader** | Reads framed messages from `TcpReader`, forwards to work queue | `wq_sender.send_tcp()` |
| **TCP Writer** | Drains outbound channel, writes to `TcpWriter` | `outbound_rx.recv()` |
| **Ping Timer** | Sends `Ping` every `ping_interval` | `outbound_tx.send(Ping)` |
| **Command Forwarder** | Relays `ClientHandle` commands to work queue | `wq_sender.send_command()` |
| **Main Event Loop** | Drains work queue, dispatches events | `wq_receiver.recv()` |
| **(Optional) UDP Reader** | Reads/decrypts UDP datagrams | `wq_sender.send_udp()` |

All inter-task communication is through `tokio::sync::mpsc` channels.
There are **no locks** in the hot path.

### 9.4 Event Loop Dispatch

The main loop calls `wq_receiver.recv()` which returns a `WorkItem`.
Dispatch logic:

#### ServerMessage::Control

1. **UdpTunnel** - Decoded via `audio_codec::decode_tunnel_audio()`.
   Dispatched to `handler.on_udp_message()`.
2. **ServerSync** - Updates state, calls `handler.on_connected()`.
3. **UserState** - Updates `state.apply_user_state()`.
4. **UserRemove** - Calls `state.remove_user()`.
5. **ChannelState** - Updates `state.apply_channel_state()`.
6. **ChannelRemove** - Calls `state.remove_channel()`.
7. **CryptSetup** - Triggers `start_udp()` to initialize UDP transport.
8. **Reject** - Logs a warning.
9. **All others** - Forwarded to `handler.on_control_message()`.

#### ServerMessage::Udp

Forwarded directly to `handler.on_udp_message()`.

#### UserCommand

1. `cmd.execute(&state)` → `CommandOutput`.
2. All `tcp_messages` are sent via the outbound channel.
3. All `udp_messages` containing audio are encoded via
   `audio_codec::encode_protobuf_audio()` and sent as `UdpTunnel`
   through TCP (fallback path).
4. If `output.disconnect` is `true`, `handler.on_disconnected()` is
   called and the loop exits.

#### Shutdown

`handler.on_disconnected()` is called and the loop exits.

### 9.5 Shutdown

The event loop terminates when:
- A `Disconnect` command sets `output.disconnect = true`.
- All work-queue sender channels are dropped (`WorkItem::Shutdown`).
- The TCP reader encounters a fatal error and all channels close.

---

## 10. Protobuf Definitions

**Directory:** `src/proto/`

Generated at build time by `prost-build` from:

| Proto file | Rust module | Contents |
|------------|-------------|----------|
| `proto/Mumble.proto` | `proto::mumble_tcp` | 27 TCP message types (Version, Authenticate, Ping, ServerSync, UserState, ChannelState, TextMessage, CryptSetup, etc.) |
| `proto/MumbleUDP.proto` | `proto::mumble_udp` | `Audio`, `Ping` |

The generated code is `include!()`-ed at compile time.

---

## 11. Design Decisions & Trade-offs

### 11.1 Biased select! for Priority

**Decision:** Use `tokio::select! { biased; }` instead of a priority queue data structure.

**Rationale:** Mumble audio requires sub-20ms latency. A biased select on
separate channels guarantees O(1) priority checking without heap
allocations or mutex contention.

### 11.2 Command Pattern over Central Dispatch

**Decision:** Each command is a self-contained struct implementing
`CommandAction` instead of a large match arm in the event loop.

**Rationale:** Open/closed principle - new features don't modify existing
code. Each command is independently testable.

### 11.3 Trait Abstraction for Audio

**Decision:** Every audio stage (capture, playback, encoder, decoder,
filter) is behind a trait.

**Rationale:** The library can run headless (bots) with `SilentCapture` /
`NullPlayback`, while desktop clients plug in real OS audio backends.
Codec implementations (Opus) are feature-gated.

### 11.4 TCP-Tunnel Fallback for Audio

**Decision:** When UDP is unavailable, audio is tunnelled through TCP
using `UdpTunnel` messages.

**Rationale:** Many networks block UDP. The tunnel provides transparent
fallback at the cost of increased latency.

### 11.5 Incremental State Updates

**Decision:** `ServerState` is updated with partial `apply_*` methods
instead of full-replace semantics.

**Rationale:** The Mumble server sends partial updates (only changed
fields are set in protobuf). Full-replace would lose fields not included
in the update.

### 11.6 CryptState Trait for UDP Encryption

**Decision:** Encryption is abstracted behind `CryptState` rather than
hard-coding OCB2-AES128.

**Rationale:** Allows testing with `PlaintextCryptState` and future
migration to different encryption schemes without touching transport code.

### 11.7 Version Negotiation

**Decision:** The client advertises both `version_v1` (legacy) and
`version_v2` (Mumble 1.5+) in the initial `Version` message.

**Rationale:** Ensures compatibility with older servers while enabling
protobuf-based UDP audio with modern ones.

---

*Last updated: 2026-03-10*
