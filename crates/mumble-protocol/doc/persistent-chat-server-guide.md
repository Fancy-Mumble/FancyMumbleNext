# Fancy Mumble Persistent Chat - Server Implementation Guide

> This document describes what a server administrator / server developer
> needs to implement to support the Fancy Mumble persistent encrypted
> chat protocol extension.
>
> **Audience**: Server plugin/companion developers, server administrators.
>
> **Prerequisites**: Familiarity with the Mumble protocol, protobuf,
> and basic cryptography concepts.
>
> **Companion document**: See `persistent-chat.md` for the full
> architecture, encryption scheme, and client-side design.

---

## Table of Contents

1. [Overview](#1-overview)
2. [Deployment Model](#2-deployment-model)
3. [Message Interception](#3-message-interception)
4. [Storage Requirements](#4-storage-requirements)
5. [Channel Configuration](#5-channel-configuration)
6. [Protocol Messages to Handle](#6-protocol-messages-to-handle)
7. [Message Storage API](#7-message-storage-api)
8. [Key Announcement Relay](#8-key-announcement-relay)
9. [Access Control](#9-access-control)
10. [Retention & Cleanup](#10-retention--cleanup)
11. [Monitoring & Ops](#11-monitoring--ops)
12. [Reference Configuration](#12-reference-configuration)
13. [Compatibility Matrix](#13-compatibility-matrix)

---

## 1. Overview

The Fancy Mumble persistent chat extension allows chat messages to be
stored on (or alongside) the Mumble server and retrieved by clients on
reconnect. All messages are **end-to-end encrypted** - the server
stores opaque ciphertext and has no ability to read message content.

The server's responsibilities are:

1. **Store** encrypted message blobs received from clients.
2. **Deliver** stored messages when clients request history.
3. **Provide** channel persistence configuration to clients via
   extended `ChannelState` protobuf fields (`pchat_mode`, etc.).
4. **Relay** key exchange messages between clients.
5. **Enforce** access control (which users can fetch which channels).
6. **Clean up** expired messages according to retention policy.

The server does **NOT**:

- Decrypt messages.
- Validate message content.
- Manage encryption keys (beyond relaying key-exchange messages).
- Modify message payloads in any way.

---

## 2. Deployment Model

The companion service can be deployed in one of two ways:

### Option A: Mumble Server Plugin (Recommended)

A plugin that hooks into murmur's `PluginDataTransmission` handling.
The plugin intercepts messages with `dataID` prefix `fancy-pchat-` and
processes them instead of (or in addition to) forwarding them.

```
  Mumble Client <--TLS--> Murmur Server
                              |
                         [Plugin Hook]
                              |
                         Companion Logic
                              |
                         Storage Backend
                         (SQLite / PostgreSQL)
```

### Option B: Sidecar Service

A separate process connected to murmur via Ice (Murmur's RPC
interface) or gRPC. The sidecar registers a virtual "bot" session
that receives `PluginDataTransmission` messages.

```
  Mumble Client <--TLS--> Murmur Server <--Ice/gRPC--> Sidecar Service
                                                            |
                                                       Storage Backend
```

In this model, clients address storage messages to the sidecar's
session ID. The sidecar session ID is discovered from the companion's
key announcement (it announces itself like any Fancy Mumble client).

---

## 3. Message Interception

### Which Messages to Intercept

All `PluginDataTransmission` messages where `dataID` starts with
`fancy-pchat-`. The relevant `dataID` values are:

| dataID | Direction | Action |
|--------|-----------|--------|
| `fancy-pchat-msg` | Client to Server | **Store** the encrypted payload |
| `fancy-pchat-fetch` | Client to Server | **Query** storage and respond |
| `fancy-pchat-key-announce` | Client to Server | **Store and broadcast** to other Fancy clients |
| `fancy-pchat-key-exchange` | Client to Client | **Relay** (forward to target session) |
| `fancy-pchat-epoch-countersig` | Client to Channel | **Relay** (broadcast to Fancy clients in channel; sender must be in `pchat_key_custodians`) |

### Messages the Server Sends

| dataID | Direction | When |
|--------|-----------|------|
| `fancy-pchat-msg-deliver` | Server to Client | On live message storage (confirmation + relay to other Fancy clients) |
| `fancy-pchat-fetch-resp` | Server to Client | In response to `fancy-pchat-fetch` |
| `fancy-pchat-key-request` | Server to Client | Broadcast to online members when a new user needs key material |
| `fancy-pchat-epoch-countersig` | Server to Client | Relayed key custodian countersignature for epoch transitions |
| `fancy-pchat-ack` | Server to Client | After storing (or rejecting) a message |

> **Note:** Channel persistence configuration is carried via extended
> protobuf fields (`pchat_mode`, etc.) on the `ChannelState` message
> (see section 5), not from a separate `PluginDataTransmission` message.

### Detection of Fancy Mumble Clients

A connecting client that supports persistent chat sends
`Version.fancy_version >= 2`. The server should track which connected
sessions are Fancy Mumble v2+ and only send `fancy-pchat-*` messages
to those sessions.

---

## 4. Storage Requirements

### Database Schema

The server needs to store the following data:

#### Table: `pchat_messages`

| Column | Type | Description |
|--------|------|-------------|
| `message_id` | TEXT (UUID) | Message identifier (unique per sender per channel) |
| `channel_id` | INTEGER | Channel this message belongs to |
| `timestamp` | INTEGER | Unix epoch milliseconds (server-assigned or server-validated) |
| `sender_hash` | TEXT | TLS certificate hash of sender |
| `mode` | TEXT | `POST_JOIN` or `FULL_ARCHIVE` |
| `payload` | BLOB | Complete encrypted message (opaque bytes) |
| `payload_size` | INTEGER | Size in bytes (for quota enforcement) |
| `superseded_by` | TEXT (UUID) | If non-null, the `message_id` of the replacement message (epoch fork re-send). Superseded messages are excluded from fetch responses. |
| `replaces_id` | TEXT (UUID) | If non-null, the `message_id` of the original message this one replaces. |
| `created_at` | TIMESTAMP | Server-side insertion time |

**Primary key**: `(channel_id, sender_hash, message_id)` - scoping
uniqueness to the sender prevents a malicious user from claiming
another user's message IDs.

**Indexes**:
- `(channel_id, timestamp)` - for chronological queries
- `(channel_id, message_id)` - for cursor-based pagination
- `(created_at)` - for retention cleanup

#### Table: `pchat_channel_config`

Caches per-channel persistence configuration extracted from
`ChannelState` protobuf messages. The source of truth is murmur's own
database (which stores the full `ChannelState` including `pchat_*`
fields); this table allows the companion to query config without
re-parsing protobuf.

| Column | Type | Description |
|--------|------|-------------|
| `channel_id` | INTEGER (PK) | Mumble channel ID |
| `mode` | INTEGER | `0`=NONE, `1`=POST_JOIN, `2`=FULL_ARCHIVE |
| `max_history` | INTEGER | Max messages stored (0 = unlimited) |
| `retention_days` | INTEGER | Auto-delete after N days (0 = forever) |

#### Table: `pchat_user_keys`

| Column | Type | Description |
|--------|------|-------------|
| `cert_hash` | TEXT (PK) | User's TLS certificate hash |
| `algorithm_version` | INTEGER NOT NULL | Cryptographic algorithm suite (1 = X25519 + Ed25519) |
| `identity_public` | BLOB | Key-agreement public key (32 bytes for version 1) |
| `signing_public` | BLOB | Signature public key (32 bytes for version 1) |
| `signature` | BLOB | Signature binding public keys to cert hash |
| `updated_at` | TIMESTAMP | Last key announcement time |

#### Table: `pchat_member_join` (POST_JOIN mode only)

| Column | Type | Description |
|--------|------|-------------|
| `channel_id` | INTEGER | Channel |
| `cert_hash` | TEXT | User's certificate hash |
| `joined_at` | TIMESTAMP | First time user joined this channel |
| `epoch_at_join` | INTEGER | Epoch number at join time |

**Unique index**: `(channel_id, cert_hash)`

#### Table: `pchat_pending_key_requests`

Stores key distribution requests that could not be fulfilled
immediately because no existing members were online. When an
authorized member connects, the server delivers pending requests
from this table.

| Column | Type | Description |
|--------|------|-------------|
| `request_id` | TEXT (UUID, PK) | Server-generated unique request ID |
| `channel_id` | INTEGER | Channel the requester needs keys for |
| `mode` | TEXT | `POST_JOIN` or `FULL_ARCHIVE` |
| `requester_hash` | TEXT | TLS certificate hash of the requesting user |
| `requester_public` | BLOB | X25519 public key of the requester (32 bytes) |
| `relay_cap` | INTEGER | Max responses the server will relay (bandwidth cap, NOT a security parameter; default 7 for FULL_ARCHIVE, 3 for POST_JOIN) |
| `relays_sent` | INTEGER | Number of key-exchange responses relayed so far (default 0) |
| `created_at` | TIMESTAMP | When the request was created |
| `fulfilled_at` | TIMESTAMP | When relay_cap was reached (NULL if pending) |
| `fulfilled_by` | TEXT | cert_hash of the first responding member (NULL if pending) |

**Indexes**:
- `(channel_id, fulfilled_at)` - find pending requests per channel
  (WHERE `fulfilled_at IS NULL`)
- `(requester_hash, fulfilled_at)` - enforce per-user pending limit
- `(created_at)` - for timeout cleanup

**Per-channel and per-user limits**: Before inserting a new key request,
the server MUST check:
- Pending requests for this `requester_hash` do not exceed the
  per-user limit (default 5). If exceeded, reject with a
  `fancy-pchat-ack` status `"rejected"`, reason
  `"key_request_limit_exceeded"`.
- Pending (unfulfilled) requests for this `channel_id` do not exceed
  the per-channel soft cap (default 100). When the cap is reached,
  the server does NOT reject the new request. Instead, it applies
  **FIFO eviction**: evict the oldest pending request from the
  requester identity that currently holds the most pending slots in
  this channel. If all requesters have equal slot counts, evict the
  globally oldest request. The evicted request is silently dropped.

  ```sql
  -- Find the requester_hash with the most pending requests
  SELECT requester_hash, COUNT(*) as cnt
  FROM pchat_pending_key_requests
  WHERE channel_id = ? AND fulfilled_at IS NULL
  GROUP BY requester_hash
  ORDER BY cnt DESC, MIN(created_at) ASC
  LIMIT 1;

  -- Delete their oldest pending request
  DELETE FROM pchat_pending_key_requests
  WHERE request_id = (
    SELECT request_id FROM pchat_pending_key_requests
    WHERE channel_id = ? AND requester_hash = ? AND fulfilled_at IS NULL
    ORDER BY created_at ASC
    LIMIT 1
  );
  ```

  This eviction policy prevents a single actor from monopolising the
  queue and locking out legitimate users via Denial of Service.

### Storage Estimates

- Average encrypted message payload: ~500 bytes to 2 KB
- 1000 messages per channel = ~0.5 to 2 MB per channel
- With 100 active channels at max 10,000 messages = 50 to 200 MB total
- Attachment-heavy channels will be larger

---

## 5. Channel Configuration

### Admin Configuration

Persistence is configured **per channel via extended protobuf fields**
in the `ChannelState` message. Fancy Mumble adds four fields at high
field IDs (100+) to avoid clashing with future upstream Mumble
additions:

```protobuf
message ChannelState {
    // ... standard Mumble fields (1-13) ...

    optional uint32 pchat_mode           = 100; // 0=NONE,1=POST_JOIN,2=FULL_ARCHIVE
    optional uint32 pchat_max_history    = 101; // max stored messages (0=unlimited)
    optional uint32 pchat_retention_days = 102; // auto-delete after N days (0=forever)
    repeated string pchat_key_custodians = 103; // cert hashes of key custodians
}
```

Legacy Mumble clients and servers silently ignore unknown protobuf
fields (standard protobuf behaviour), so these extensions cause no
breakage.

#### Field Definitions

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `pchat_mode` | uint32 | absent = NONE | `0` NONE, `1` POST_JOIN, `2` FULL_ARCHIVE |
| `pchat_max_history` | uint32 | server default (e.g. 5000) | Maximum messages stored (0 = unlimited) |
| `pchat_retention_days` | uint32 | server default (e.g. 90) | Auto-delete after N days (0 = forever) |
| `pchat_key_custodians` | repeated string | empty list | Cert hashes of key custodians (trusted authorities for key lifecycle) |

#### How the Admin Configures a Channel

Configuration uses the **same mechanism as any other channel property
change** (name, description, etc.): the admin sends a `ChannelState`
protobuf message with the `pchat_*` fields set.

1. The admin opens the channel editor in Fancy Mumble (or any client
   that supports the extended `ChannelState` fields) and sets the
   desired persistence mode, max history, and retention days.
2. The client sends a `ChannelState` message to the server with:
   ```protobuf
   ChannelState {
       channel_id: 5,
       pchat_mode: 3,             // FULL_ARCHIVE
       pchat_max_history: 10000,
       pchat_retention_days: 90,
   }
   ```
   Only the fields being changed need to be present (standard
   protobuf partial-update semantics, same as changing `name` or
   `description`).
3. The server companion intercepts the incoming `ChannelState`:
   - Verifies the sender has admin/operator permissions for the
     channel (same ACL check as for name/description changes).
   - Persists the `pchat_*` values in its `pchat_channel_config`
     database table.
4. Murmur re-broadcasts the updated `ChannelState` (including the
   `pchat_*` fields) to all connected clients. Legacy clients
   silently ignore the unknown fields.
5. Fancy Mumble clients receive the broadcast and update their
   local channel state with the new persistence config.

> **Note:** Because `pchat_*` fields are standard protobuf optional
> fields on `ChannelState`, murmur treats them like any other unknown
> field - it stores them in its SQLite database and re-broadcasts them
> on channel state updates and to newly connecting clients. The
> companion service can additionally persist them in its own database
> for query performance.

### Server Companion Configuration

The companion service needs a global config file for server-wide
settings:

```ini
[pchat]
# Enable persistent chat companion
enabled = true

# Storage backend
storage = sqlite
sqlite_path = /var/lib/murmur/pchat.db

# Rate limiting
fetch_rate_limit = 10/min
msg_rate_limit = 30/min
key_announce_rate_limit = 5/min
key_announce_ip_rate_limit = 20/min
key_exchange_rate_limit = 20/min

# Require registered Mumble accounts for persistent chat
require_registration = true

# Cleanup interval (seconds)
cleanup_interval = 3600

# Pending key request expiry (days)
pending_key_request_max_age_days = 7

# Maximum payload size per message (bytes)
max_payload_size = 65536

# Timestamp validation: "override" (replace with server time) or
# "delta" (reject if abs(client - server) > max_timestamp_delta_ms)
timestamp_mode = override
max_timestamp_delta_ms = 5000

# Maximum total storage (MB, 0 = unlimited)
max_storage_mb = 500

# Default values when ChannelState omits optional pchat fields
default_max_history = 5000
default_retention_days = 90
```

> Per-channel settings (`mode`, `max_history`, `retention_days`) are
> set by admins via `ChannelState` protobuf messages (same as channel
> name/description edits). Murmur persists them in its database. The
> companion caches them in `pchat_channel_config` for query use.

### Detecting Configuration Changes

The companion hooks into murmur's `ChannelState` processing. When an
incoming `ChannelState` message contains any `pchat_*` fields:

1. Verify the sender has admin/operator permissions for the channel.
2. Extract `pchat_mode`, `pchat_max_history`, and
   `pchat_retention_days` from the message.
3. Update the `pchat_channel_config` row in the companion's database.
4. If a channel's mode changes from persistent to `NONE` (mode 0),
   stop accepting new messages but keep existing stored messages until
   retention expires (or admin explicitly purges).

Murmur itself handles re-broadcasting the updated `ChannelState`
(including the `pchat_*` fields) to all connected clients - the
companion does not need to duplicate this.

### Broadcasting Configuration

Since `pchat_*` fields are part of the `ChannelState` protobuf message,
murmur's standard behaviour handles broadcasting automatically:

- **On connect**: Murmur sends `ChannelState` for every channel during
  the handshake. If `pchat_*` fields were previously set, they are
  included.
- **On change**: When a `ChannelState` update is received (from an
  admin client), murmur re-broadcasts it to all connected clients.
- **Legacy clients**: Silently ignore the unknown `pchat_*` fields
  per standard protobuf behaviour. No special handling needed.

---

## 6. Protocol Messages to Handle

All payloads use **MessagePack** encoding inside
`PluginDataTransmission.data`. The server must decode and re-encode
MessagePack. Reference implementations are available in every major
language (msgpack.org).

### 6.1 Handling `fancy-pchat-msg` (Store Request)

**Input** (from client):

```
{
  "message_id": "550e8400-e29b-41d4-a716-446655440000",
  "channel_id": 5,
  "timestamp": 1710500000000,
  "sender_hash": "abc123def456...",
  "mode": "FULL_ARCHIVE",
  "envelope": <bytes>,          // opaque encrypted blob
  "epoch": 3 | null,            // only present for POST_JOIN mode
  "chain_index": 42 | null,     // only present for POST_JOIN mode
  "replaces_id": null | "old-uuid"  // if set, this message replaces the referenced message
}
```

**Server actions**:

1. **Validate** the sender:
   - `sender_hash` matches the connecting session's TLS certificate hash.
   - Channel `channel_id` exists and has the claimed `mode`.
   - Message `message_id` does not already exist for this
     `(channel_id, sender_hash)` tuple (no duplicates).
   - Payload size is within configured limits.
   - If `replaces_id` is non-null:
     - The referenced message MUST exist in `pchat_messages` with
       the same `channel_id` and `sender_hash` (a sender can only
       replace their own messages).
     - Mark the original message as superseded:
       ```sql
       UPDATE pchat_messages
       SET superseded_by = :new_message_id
       WHERE message_id = :replaces_id
         AND channel_id = :channel_id
         AND sender_hash = :sender_hash;
       ```
     - Superseded messages are excluded from `fancy-pchat-fetch`
       responses (the replacement message is returned instead).
     - If the referenced message does not exist (e.g. it was already
       purged by retention), accept the new message normally (store
       it without marking any original as superseded).
2. **Validate timestamp**: The client-provided timestamp MUST NOT be
   trusted blindly. Apply one of:
   - **Override** (recommended): Replace `timestamp` with the server's
     current time at ingestion. This is simplest and eliminates all
     timestamp manipulation risks.
   - **Enforce delta**: Reject the message if
     `abs(client_timestamp - server_time) > 5000ms`. This preserves
     client-local precision while preventing backdating or
     future-dating attacks.
   The `created_at` column always records the server's clock regardless
   of which approach is used.
3. **Store** in `pchat_messages` table.
4. **Send `fancy-pchat-ack`** to the sender:
   ```
   { "message_id": "...", "status": "stored" }
   ```
5. **Optionally relay** the encrypted message to other connected Fancy
   Mumble sessions in the channel via `fancy-pchat-msg-deliver` (so
   they can cache it locally without a future fetch).

### 6.2 Handling `fancy-pchat-fetch` (History Request)

**Input** (from client):

```
{
  "channel_id": 5,
  "before_id": "550e8400-..." | null,
  "after_id": null,
  "limit": 50
}
```

**Server actions**:

1. **Authorize**: Check that the requesting user has access to the
   channel.
2. **Mode-based filtering**:
   - **POST_JOIN**: Only return messages with `timestamp >=` the
     user's `joined_at` from `pchat_member_join`.
   - **FULL_ARCHIVE**: Return all messages (no additional filtering).
3. **Pagination**: If `before_id` is set, return messages with
   `timestamp < messages[before_id].timestamp`, ordered by timestamp
   descending, limited to `limit`.
4. **Respond** with `fancy-pchat-fetch-resp`:
   ```
   {
     "channel_id": 5,
     "messages": [ { ... }, { ... } ],
     "has_more": true,
     "total_stored": 1247
   }
   ```

### 6.3 Handling `fancy-pchat-key-announce` (Key Registration)

**Input** (from client):

```
{
  "algorithm_version": 1,
  "identity_public": <32 bytes>,
  "signing_public": <32 bytes>,
  "cert_hash": "abc123...",
  "timestamp": 1710500000000,
  "signature": <bytes>,
  "tls_signature": <bytes>
}
```

**Server actions**:

1. **Validate `algorithm_version`**: The server MUST reject
   announcements with an `algorithm_version` it does not recognise.
   Currently the only valid value is `1` (X25519 + Ed25519). This
   prevents storing key material the server cannot validate or that
   peers cannot parse.
2. **Verify identity**: `cert_hash` MUST match the connecting session's
   TLS certificate hash. Reject immediately if they differ (prevents
   a client from registering keys under another user's identity).
3. **Verify TLS signature**: The server MUST verify the `tls_signature`
   field over `algorithm_version (1B) || cert_hash || timestamp ||
   identity_public || signing_public` using the client's TLS
   certificate public key. This confirms the client controls the TLS
   private key. Without this check, a malicious client can upload
   garbage keys, causing a Denial of Service: other clients would
   fail to encrypt messages targeting the impersonated cert_hash.
   The Ed25519 `signature` is verified by **peers**, not the server
   (the server does not yet know the Ed25519 public key at announce
   time -- that is what is being announced).
4. **Store** or update the public keys (`algorithm_version`,
   `identity_public` and `signing_public`) in `pchat_user_keys`.
5. **Broadcast** the announcement to all other connected Fancy Mumble
   sessions (so they can cache the `algorithm_version` and both
   public keys for DH and signature verification).

### 6.4 Handling `fancy-pchat-key-exchange` (Key Distribution)

This is a **relay-only** operation. The server does not interpret the
encrypted key payload but does validate identity and manage
confirmation tracking.

**Input** (from client):

```
{
  "channel_id": 5,
  "mode": "POST_JOIN",
  "epoch": 4,
  "encrypted_key": <bytes>,
  "sender_hash": "abc123...",
  "recipient_hash": "def456...",
  "request_id": "a1b2c3d4-..." | null,
  "timestamp": 1710500000000,
  "algorithm_version": 1,
  "signature": <bytes>,
  "parent_fingerprint": <8 bytes> | null,
  "epoch_fingerprint": <8 bytes>
}
```

**Server actions**:

1. **Validate sender identity**: The server MUST hard-validate that
   `sender_hash == current_session.cert_hash`. Without this check,
   Client A can forge the `sender_hash` field to impersonate Client B
   and send garbage key material to Client C, causing Client C to use
   a key that Client B never agreed to.
2. **Relay tracking** (if `request_id` is present and non-null):
   Check `pchat_pending_key_requests` for a row matching
   `request_id`.
   - If the relay cap is reached
     (`relays_sent >= relay_cap`), silently
     drop this response.
   - Otherwise, increment `relays_sent`:
     ```sql
     UPDATE pchat_pending_key_requests
     SET relays_sent = relays_sent + 1,
         fulfilled_by = COALESCE(fulfilled_by, :sender_hash),
         fulfilled_at = CASE
           WHEN relays_sent + 1 >= relay_cap
           THEN NOW() ELSE fulfilled_at END
     WHERE request_id = :request_id
       AND relays_sent < relay_cap
       AND fulfilled_by IS DISTINCT FROM :sender_hash;
     ```
     If the UPDATE affects 0 rows, either the cap was already
     reached or this sender already responded - drop this response.
     The `IS DISTINCT FROM` check prevents the same member from
     sending multiple responses (each response MUST come from
     a different identity).

   **Note**: The `relay_cap` is a **bandwidth/performance cap**, NOT
   a security parameter. Consensus enforcement is entirely
   client-side (see architecture doc section 5.3). The server's role
   is to relay responses efficiently; the client decides when it has
   enough confirmations.

   **Epoch broadcasts** (`request_id` is null): When `request_id` is
   null, the key-exchange is an **epoch broadcast** from a POST_JOIN
   responder distributing the new epoch key to existing members (see
   architecture doc section 5.2). The server MUST skip all relay
   tracking (no `pchat_pending_key_requests` lookup). Proceed
   directly to step 3 (look up recipient, forward).
3. **Look up** the session ID for `recipient_hash` (from
   `pchat_user_keys` + active sessions).
4. **Forward** the message as `PluginDataTransmission` to the
   recipient session. The server forwards ALL accepted responses
   (up to `relay_cap` for tracked requests, or unconditionally for
   epoch broadcasts) so the client can perform its own
   client-side consensus evaluation.
5. If the recipient is offline, **queue** the message for delivery on
   their next connect.

### 6.5 Generating `fancy-pchat-key-request` (Key Request Broadcast)

The server generates and sends `fancy-pchat-key-request` messages. This
is NOT a client-originated message - the server creates it when a new
user needs key material for a persistent channel.

**Trigger**: A Fancy Mumble v2+ client sends `fancy-pchat-key-announce`
while in a persistent channel (POST_JOIN or FULL_ARCHIVE) and the server
determines the client does not yet have a pending (unfulfilled) key
request for that channel.

**Output** (server to clients):

```
{
  "channel_id": 5,
  "mode": "POST_JOIN",
  "requester_hash": "def456...",
  "requester_public": <32 bytes>,
  "request_id": "a1b2c3d4-...",
  "timestamp": 1710500000000,
  "relay_cap": 7
}
```

**Server actions**:

1. **Enforce per-channel limit**: Count pending (unfulfilled) requests
   for this `channel_id`. If >= max (default 100), reject the request
   with a `fancy-pchat-ack` status `"rejected"`, reason
   `"key_request_limit_exceeded"`. Do NOT broadcast.
2. **Enforce per-user limit**: Count pending (unfulfilled) requests
   for this `requester_hash`. If >= max (default 5), reject similarly.
3. **Determine `relay_cap`**: This is a **bandwidth/performance cap**,
   NOT a security parameter. Consensus enforcement is entirely
   client-side. Recommended defaults:
   - **FULL_ARCHIVE**: `relay_cap = clamp(floor(online_members / 2), 1, 5) + 2`.
     The `+2` headroom ensures the client receives enough responses
     for its own threshold computation.
   - **POST_JOIN**: `relay_cap = 3` (allows the new member to
     receive competing epoch keys for deterministic fork resolution;
     see architecture doc section 5.2).
   These are configurable per server.
4. **Set `timestamp`**: Record the current server time (Unix epoch
   millis). Clients use this for freshness validation.
5. **Generate** a UUID `request_id`.
6. **Store** the request in `pchat_pending_key_requests` with
   `relays_sent = 0` and `fulfilled_at = NULL`.
7. **Broadcast** the `fancy-pchat-key-request` as
   `PluginDataTransmission` to all other online Fancy Mumble v2+
   sessions in the same channel.
7. If **no other Fancy Mumble members are online** in the channel, the
   request remains queued. When the first authorized member connects
   and joins the channel, the server delivers all pending
   (unfulfilled) requests for that channel (see section 8).
8. **Timeout cleanup**: Pending requests older than the configured
   timeout (e.g. 7 days) are periodically pruned. The affected user
   can re-trigger by re-announcing their key.

### 6.6 Handling `fancy-pchat-epoch-countersig` (Epoch Countersignature)

Channel admins/creators send `fancy-pchat-epoch-countersig` to
countersign epoch transitions so that new users can verify epoch
keys without a prior `parent_fingerprint`. Only users listed in
`pchat_key_custodians` (or the channel creator) should be allowed
to send this message.

**Server actions:**

1. Validate the sender's cert hash appears in the channel's
   `pchat_key_custodians` list, or that the sender is the channel
   creator (from murmur's channel metadata).
2. Verify `signer_hash` matches the sender's cert hash.
3. Verify `distributor_hash` matches the sender's cert hash (for
   standalone countersigs, the distributor is the signer themselves).
4. Validate `timestamp` freshness: reject if
   `abs(server_time - timestamp) > 5000ms`.
5. **Broadcast** the `PluginDataTransmission` to all other online
   Fancy Mumble v2+ sessions in the same channel.
6. Optionally **store** the countersignature alongside the epoch
   record in `pchat_keys` so it can be included in historical key
   fetches.

The server does **not** verify the Ed25519 countersignature itself;
that is the client's responsibility. The server only enforces the
sender identity, key custodian permission, and timestamp freshness
checks.

**Note on atomic delivery**: Key custodians may also embed their
countersignature directly in a `fancy-pchat-key-exchange` payload
(via the `countersignature` and `countersigner_hash` fields). The
server relays such key-exchange messages normally; no special
handling is needed beyond the existing `fancy-pchat-key-exchange`
relay logic (section 6.4).

---

## 7. Message Storage API

For implementors who prefer an API abstraction over direct SQL, here is
the recommended interface:

```
interface MessageStore {
    // Store a message
    store(message_id, channel_id, timestamp, sender_hash, mode, payload) -> Result

    // Fetch messages with mode-based access control
    fetch(channel_id, requester_hash, before_id?, after_id?, limit) -> (messages[], has_more, total)

    // Delete expired messages
    cleanup_expired() -> deleted_count

    // Check if message exists
    exists(message_id) -> bool

    // Record member join (POST_JOIN mode)
    record_join(channel_id, cert_hash, epoch) -> Result

    // Get channel config
    get_config(channel_id) -> ChannelConfig?

    // Update channel config
    set_config(channel_id, mode, max_history, retention_days) -> Result

    // Create a pending key request
    create_key_request(request_id, channel_id, mode, requester_hash, requester_public, relay_cap) -> Result

    // Record a relay for a pending key request (returns false if relay_cap already reached or same sender)
    record_key_relay(request_id, sender_hash) -> bool

    // Check if a key request has reached its relay cap (relays_sent >= relay_cap)
    is_key_request_fulfilled(request_id) -> bool

    // Get unfulfilled pending key requests for a channel
    get_pending_key_requests(channel_id) -> PendingKeyRequest[]

    // Count pending key requests per channel (for limit enforcement)
    count_pending_requests_for_channel(channel_id) -> u32

    // Count pending key requests per user (for limit enforcement)
    count_pending_requests_for_user(requester_hash) -> u32

    // Prune expired pending key requests
    cleanup_expired_key_requests(max_age_days) -> deleted_count
}
```

---

## 8. Key Announcement Relay & Pending Key Requests

The server acts as both a key directory and a decentralized key
distribution coordinator. There is no designated "key admin" - any
online member holding the current channel key can distribute it to
new members.

### 8.1 Key Announcement Flow

When a Fancy Mumble client connects:

1. The client sends `fancy-pchat-key-announce` with its identity
   public key.
2. The server validates the announcement:
   - Verifies `tls_signature` and `cert_hash` match the session's
     TLS certificate (existing requirement).
   - **Anti-rollback**: If the server already has a stored key
     announcement for this `cert_hash` with a `timestamp` >= the
     incoming announcement's `timestamp`, the server SHOULD reject
     the announcement (silently drop or return an error ack). This
     prevents replay of older key-announce messages that could roll
     back a user's identity keys. The server stores the latest
     `timestamp` per `cert_hash` in `pchat_user_keys.updated_at`
     (or a dedicated timestamp column).
3. The server stores it in `pchat_user_keys` (upsert, updating
   the stored timestamp).
4. The server sends all previously stored public keys to the new client
   (batch delivery via `PluginDataTransmission`).
5. The server broadcasts the new client's public key to all other
   connected Fancy Mumble sessions.

This allows clients to respond to key requests
(POST_JOIN/FULL_ARCHIVE mode) without out-of-band communication.

### 8.2 Key Request Broadcast (Online Members Present)

When a new user joins a persistent channel (POST_JOIN or FULL_ARCHIVE)
and announces their key:

1. The server enforces per-user (default 5) pending request limits.
   If the per-user limit is exceeded, the request is rejected. If
   the per-channel soft cap (default 100) is reached, the oldest
   request from the heaviest requester is evicted (FIFO eviction).
2. The server determines `relay_cap` (a bandwidth cap, NOT a security
   parameter): recommended `clamp(floor(online_members / 2), 1, 5) + 2`
   for FULL_ARCHIVE, `3` for POST_JOIN (allows the new member to
   receive competing epoch keys for fork resolution). Consensus
   enforcement is entirely client-side (see architecture doc
   section 5.3).
3. The server generates a `fancy-pchat-key-request` with a new UUID
   `request_id`, a `timestamp` (current server time), and the
   `relay_cap` value.
4. The server stores the request in `pchat_pending_key_requests`
   (with `relays_sent = 0`, `fulfilled_at = NULL`).
5. The server broadcasts the request to all other online Fancy Mumble
   v2+ sessions in the channel.
6. Members respond with `fancy-pchat-key-exchange` (each signed with
   their Ed25519 identity key). Each response includes `signature`,
   `timestamp`, `epoch_fingerprint`, and (for POST_JOIN)
   `parent_fingerprint`.
7. The server relays up to `relay_cap` responses from **distinct**
   senders (same sender cannot respond twice). Each relayed response
   is forwarded to the new user. The server does NOT evaluate
   consensus; it simply relays.
8. When `relays_sent` reaches `relay_cap`, the server marks the
   request as fulfilled and stops relaying. Later responses are
   silently dropped.

**Epoch broadcast (POST_JOIN)**: After responding to the key request,
the winning responder also sends `fancy-pchat-key-exchange` messages
with `request_id: null` to each other existing online member in the
channel (one per member, each encrypted to that member's X25519
public key). The server relays these unconditionally (no relay
tracking applies when `request_id` is null, see section 6.4).

Because multiple members may respond before the server marks the
request as fulfilled, existing members may receive competing epoch
keys from different responders. This is resolved entirely
client-side via a deterministic tie-breaker: clients accept the
epoch key from the sender with the lexicographically smallest
`cert_hash` and discard competing keys (see architecture doc
section 5.2).
9. The receiving client evaluates consensus entirely on its own
   (see architecture doc section 5.3):
   - Verifies Ed25519 signature against each sender's known public key.
   - Checks timestamp freshness.
   - Key custodian shortcut: if sender is a key custodian (from
     `pchat_key_custodians`) or channel originator, accepts
     immediately as Verified.
   - Otherwise: opens a 10-second collection window, accumulates
     responses, computes `required_threshold` from locally observed
     members, evaluates consensus after the window closes.

### 8.3 Pending Queue (No Online Members)

When no other Fancy Mumble members are online in the channel:

1. The key request remains in `pchat_pending_key_requests` with
   `relays_sent = 0`.
2. When an authorized Fancy Mumble member connects and joins the
   channel, the server checks for pending (unfulfilled) requests:
   ```sql
   SELECT * FROM pchat_pending_key_requests
   WHERE channel_id = :channel_id
     AND fulfilled_at IS NULL
   ORDER BY created_at ASC;
   ```
3. The server delivers these pending requests to the newly connected
   member as `fancy-pchat-key-request` messages.
4. The member's client processes each request (respecting its
   per-connection batch limit of 50), signs each key-exchange with
   its Ed25519 identity key, and sends back
   `fancy-pchat-key-exchange` responses.
5. The server routes each key exchange to the recipient (immediately
   if online, or queued for their next connect). For FULL_ARCHIVE,
   additional members connecting later may also respond to the same
   pending request until `relay_cap` is reached.

**Client-side batch limit**: Clients MUST enforce a maximum of 50
key requests processed per connection. If the server delivers more
pending requests, the client processes only the first 50 and ignores
the rest. This prevents cryptographic exhaustion DoS via a flood of
queued requests. Clients SHOULD also throttle processing to max 10
key requests per second.

### 8.4 Pending Request Cleanup

Pending key requests that remain unfulfilled for longer than a
configurable period (recommended: 7 days) should be pruned:

```sql
DELETE FROM pchat_pending_key_requests
WHERE fulfilled_at IS NULL
  AND created_at < NOW() - INTERVAL :max_age_days DAY;
```

Fulfilled requests can also be pruned after a shorter period (e.g.
24 hours) since they are only needed for deduplication.

The affected user can re-trigger a key request by re-joining the
channel or re-announcing their key.

### 8.5 Key Validity

Public keys (both X25519 and Ed25519) are bound to TLS certificate
hashes. If a user generates a new client certificate, their identity
keys change. The server should:

- Accept the new key announcement (overwrite both X25519 and Ed25519
  public keys).
- Notify connected clients of the key change (so they update their
  peer key caches for both DH and signature verification).
- Flag messages encrypted with the old key as potentially inaccessible
  for the user (they need to re-import their old certificate or accept
  data loss).

---

## 9. Access Control

### Channel Access

A user can only fetch messages from channels they have permission to
enter. Use murmur's existing ACL system to determine channel access.

### Mode-Based Restrictions

| Mode | Who Can Fetch | Additional Check |
|------|---------------|------------------|
| POST_JOIN | Only registered users who have joined | Check `pchat_member_join.joined_at` |
| FULL_ARCHIVE | Any user with channel access | Channel ACL only |

### Rate Limiting

Implement rate limiting on all persistent chat operations to prevent
abuse:

| Operation | Recommended Limit | Rationale |
|-----------|------------------|-----------|
| `fancy-pchat-fetch` | 10/min per session | Prevent history scraping |
| `fancy-pchat-msg` | 30/min per session | Prevent storage flooding |
| `fancy-pchat-key-announce` | 5/min per session, 20/min per IP | Rapid key rotation forces all peers to re-encrypt; potential DoS vector |
| `fancy-pchat-key-exchange` | 20/min per session | Prevent key exchange flooding |
| Pending key requests per channel | 100 soft cap (FIFO eviction) | Prevent resource exhaustion; evict heaviest requester's oldest request when full |
| Pending key requests per user | 5 max | Prevent a single user from flooding the queue |

All limits are configurable per server.

**Client-side limits** (enforced by the Fancy Mumble client, not the
server):

| Limit | Default | Rationale |
|-------|---------|-----------|
| Key requests processed per connection | 50 | Prevent cryptographic exhaustion from queued request floods |
| Key request processing rate | 10/sec | Bound CPU usage from asymmetric crypto operations |

### Registration Requirement

Servers SHOULD require **Mumble user registration** (registered
accounts with server-stored certificates) before allowing persistent
chat interactions. Unregistered/guest users can still use standard
volatile Mumble chat but SHOULD be denied:

- Sending `fancy-pchat-msg` (message storage)
- Sending `fancy-pchat-key-announce` (identity key registration)
- Receiving `fancy-pchat-fetch-resp` (history retrieval)

This raises the cost of abuse and ensures identity keys are bound to
stable, server-recognized identities rather than ephemeral sessions.

### Quota Enforcement

- Per-channel message count limit (`max_history`).
- Per-channel total storage size limit.
- When quota is exceeded, delete oldest messages first.
- Return `quota_exceeded` in `fancy-pchat-ack` when a new message would
  exceed limits.

---

## 10. Retention & Cleanup

### Automatic Cleanup

Run a periodic cleanup task (recommended: every hour):

```sql
DELETE FROM pchat_messages
WHERE created_at < NOW() - INTERVAL retention_days DAY
AND channel_id IN (
    SELECT channel_id FROM pchat_channel_config
    WHERE retention_days > 0
);
```

Additionally, prune stale pending key requests (see section 8.4):

```sql
DELETE FROM pchat_pending_key_requests
WHERE fulfilled_at IS NULL
  AND created_at < NOW() - INTERVAL 7 DAY;

DELETE FROM pchat_pending_key_requests
WHERE fulfilled_at IS NOT NULL
  AND fulfilled_at < NOW() - INTERVAL 1 DAY;
```

### Manual Cleanup

Admins should be able to:

- Clear all messages for a channel.
- Clear all messages from a specific user (e.g., after ban).
- Adjust retention policy (existing messages are not retroactively
  affected unless explicitly purged).

### Storage Reclamation

After deleting messages, run database maintenance:

- **SQLite**: `VACUUM`
- **PostgreSQL**: Auto-vacuum handles this

---

## 11. Monitoring & Ops

### Metrics to Track

| Metric | Description |
|--------|-------------|
| `pchat_messages_stored_total` | Total messages stored (counter) |
| `pchat_messages_fetched_total` | Total fetch requests served (counter) |
| `pchat_storage_bytes` | Total encrypted payload bytes stored (gauge) |
| `pchat_channels_persistent` | Number of channels with persistence enabled (gauge) |
| `pchat_cleanup_deleted_total` | Messages deleted by retention cleanup (counter) |
| `pchat_key_exchanges_total` | Key exchange relays performed (counter) |
| `pchat_key_requests_total` | Key requests broadcast (counter) |
| `pchat_key_requests_pending` | Currently unfulfilled key requests (gauge) |
| `pchat_key_requests_fulfilled_total` | Key requests successfully fulfilled (counter) |
| `pchat_key_requests_expired_total` | Pending key requests pruned by timeout (counter) |
| `pchat_fetch_latency_ms` | Fetch request duration (histogram) |

### Health Checks

- Database connectivity.
- Storage usage vs. configured limits.
- Cleanup task last-run timestamp.
- Connected Fancy Mumble sessions count.

### Logging

Log (at INFO level):
- Channel config changes.
- Messages stored/rejected (without payload content).
- Fetch requests with result counts.
- Key announcements and exchanges.
- Retention cleanup runs with deletion counts.

Never log:
- Encrypted payloads.
- Key material.
- User identity private keys.

---

## 12. Reference Configuration

Per-channel persistence is configured via `ChannelState` protobuf
fields (see section 5). The companion service needs only a small
global config:

### Minimal Setup (SQLite)

```ini
[pchat]
# Enable persistent chat companion
enabled = true

# Storage backend
storage = sqlite
sqlite_path = /var/lib/murmur/pchat.db

# Rate limiting
fetch_rate_limit = 10/min

# Cleanup interval (seconds)
cleanup_interval = 3600

# Maximum payload size per message (bytes)
max_payload_size = 65536

# Maximum total storage (MB, 0 = unlimited)
max_storage_mb = 500

# Default values for optional pchat fields omitted in ChannelState
default_max_history = 5000
default_retention_days = 90
```

### PostgreSQL Setup

```ini
[pchat]
enabled = true
storage = postgresql
postgresql_url = postgresql://mumble:secret@localhost/mumble_pchat

# ... same global settings as above
```

### Docker Compose Addition

```yaml
services:
  mumble-server:
    image: mumblevoip/mumble-server:latest
    # ... existing config
    # Per-channel persistence is configured by editing channel
    # descriptions in the Mumble client (no config file needed).

  pchat-companion:
    image: fancymumble/pchat-companion:latest
    environment:
      MUMBLE_ICE_HOST: mumble-server
      MUMBLE_ICE_PORT: 6502
      PCHAT_STORAGE: sqlite
      PCHAT_SQLITE_PATH: /data/pchat.db
      PCHAT_MAX_PAYLOAD_SIZE: 65536
      PCHAT_MAX_STORAGE_MB: 500
    volumes:
      - pchat-data:/data
    depends_on:
      - mumble-server

volumes:
  pchat-data:
```

---

## 13. Compatibility Matrix

| Client Version | Server (no companion) | Server (with companion) |
|---------------|----------------------|------------------------|
| Legacy Mumble | Full compatibility | Full compatibility (companion invisible) |
| Fancy Mumble v1 | Full compatibility | Full compatibility (companion invisible) |
| Fancy Mumble v2+ | Full compatibility (no persistence) | Full persistence support |

| Feature | Required Server Version | Notes |
|---------|------------------------|-------|
| Real-time chat | Any murmur | Standard `TextMessage` |
| Message storage | Companion v1+ | `PluginDataTransmission` interception |
| Config via protobuf | Companion v1+ | `pchat_mode` etc. in `ChannelState` protobuf |
| Key relay | Companion v1+ | Public key directory + key request broadcast |
| Key request queue | Companion v1+ | Async store-and-forward for offline members |
| POST_JOIN mode | Companion v1+ | Requires join tracking |
| FULL_ARCHIVE mode | Companion v1+ | Simplest mode to implement |
| Retention cleanup | Companion v1+ | Periodic background task |

### Implementation Priority

For a minimum viable implementation, implement in this order:

1. **FULL_ARCHIVE mode** - simplest key model (single channel key),
   no per-user tracking needed for access control.
2. **Channel configuration** - `pchat_*` fields in `ChannelState`.
3. **Message storage & fetch** - core store/retrieve loop.
4. **Key announcement relay** - public key directory.
5. **Key request broadcast & pending queue** - decentralized key
   distribution with async store-and-forward for offline members.
6. **POST_JOIN mode** - add join tracking and epoch-based filtering.
7. **Retention cleanup** - scheduled background task (including
   pending key request expiry).
8. **Monitoring & rate limiting** - operational hardening.

---

## Appendix: MessagePack Field Reference

All payloads use MessagePack with **string keys** (not integer keys)
for readability and forward compatibility. Unknown fields must be
ignored (not rejected) to allow future extensions.

### Required MessagePack Libraries

| Language | Library |
|----------|---------|
| Rust | `rmp-serde` |
| Python | `msgpack` |
| Go | `github.com/vmihailenco/msgpack` |
| C++ | `msgpack-c` |
| Java/Kotlin | `org.msgpack:msgpack-core` |
| Node.js | `@msgpack/msgpack` |
