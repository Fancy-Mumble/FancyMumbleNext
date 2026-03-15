# Persistent Encrypted Chat - Architecture & Protocol Extension

> Design document for the Fancy Mumble persistent chat feature.
> This describes a backwards-compatible Mumble protocol extension that
> enables server-stored, end-to-end encrypted chat history.

## Table of Contents

1. [Goals & Constraints](#goals--constraints)
2. [Protocol Extension Overview](#protocol-extension-overview)
3. [Persistence Modes](#persistence-modes)
4. [E2E Encryption Scheme](#e2e-encryption-scheme)
5. [Key Management](#key-management)
6. [Wire Format](#wire-format)
7. [Message Lifecycle](#message-lifecycle)
8. [Client Architecture](#client-architecture)
9. [Backwards Compatibility](#backwards-compatibility)
10. [Security Considerations](#security-considerations)

---

## 1. Goals & Constraints

### Goals

- **Persistent chat**: Messages survive server restarts and client
  reconnections. Users who disconnect and reconnect later can retrieve
  earlier messages.
- **E2E encryption**: The server stores opaque ciphertext. It has no
  knowledge of message content, sender names (inside the encrypted
  envelope), or any metadata beyond what it needs for routing.
- **Two access modes**: Server admins configure per-channel how much
  history a user can access (see section 3).
- **Backwards compatible**: Legacy Mumble servers and clients continue
  to work. The extension uses `PluginDataTransmission` exclusively -
  a message type that legacy servers already forward and legacy clients
  silently ignore.
- **Offline-friendly**: Messages can be offloaded to disk on the client
  and re-fetched from the server when needed.

### Constraints

- **Extended protobuf schema**: Fancy Mumble extends `Mumble.proto`
  with additional optional fields starting at high field IDs (100+)
  to avoid clashing with future upstream Mumble additions.
  Legacy clients and servers silently ignore unknown protobuf fields
  (standard protobuf behaviour), so these extensions are inherently
  backwards-compatible.
- **No server-side logic changes to core Mumble**: The persistence
  layer is a separate companion service (or plugin) that the server
  admin deploys alongside murmur. The companion intercepts
  `PluginDataTransmission` messages with specific `dataID` prefixes.
- **Message IDs**: We use client-generated UUID v4 values. These are
  collision-resistant and do not conflict with any Mumble-internal
  sequence numbering.

---

## 2. Protocol Extension Overview

All persistent-chat communication flows through `PluginDataTransmission`
with `dataID` values prefixed `fancy-pchat-`. This namespace is
unlikely to collide with upstream or other extensions.

### Message Types (dataID values)

| dataID | Direction | Purpose |
|--------|-----------|---------|
| `fancy-pchat-msg` | Client to Server | Encrypted message for storage |
| `fancy-pchat-msg-deliver` | Server to Client | Stored message delivery (single or batch) |
| `fancy-pchat-fetch` | Client to Server | Request stored messages |
| `fancy-pchat-fetch-resp` | Server to Client | Response to fetch request |
| `fancy-pchat-key-exchange` | Peer to Peer | Key distribution (via server relay) |
| `fancy-pchat-key-announce` | Client to Server | Announce public key material |
| `fancy-pchat-key-request` | Server to Client | Broadcast key request for a new member |
| `fancy-pchat-epoch-countersig` | Client to Client | Key custodian countersignature on epoch transitions (see 5.6.4) |
| `fancy-pchat-ack` | Server to Client | Storage acknowledgement |

> **Note:** Channel persistence configuration is **not** a
> `PluginDataTransmission` message. It is carried via extended
> protobuf fields (`pchat_mode`, `pchat_max_history`,
> `pchat_retention_days`) added to the `ChannelState` message at
> high field IDs (100+). See section 6.1.

### Why PluginDataTransmission

1. Legacy murmur forwards `PluginDataTransmission` to listed
   `receiverSessions` without interpretation - it is already the
   sanctioned extensibility mechanism.
2. Legacy clients ignore unknown `dataID` values.
3. The companion server service hooks into the same forwarding path,
   intercepting messages addressed to a virtual "storage" session or
   broadcast to the channel.

---

## 3. Persistence Modes

A server admin configures each channel with one of three modes:

| Mode | Enum | Description |
|------|------|-------------|
| **None** | `NONE` | No persistence. Standard volatile Mumble chat. |
| **Post-Join** | `POST_JOIN` | Users can access all messages sent after the moment they first joined the channel (registered-user identity, not session). Messages from before their initial join are inaccessible. |
| **Full Archive** | `FULL_ARCHIVE` | Users can access all stored messages regardless of when they joined. A shared channel key encrypts everything. |
| **Server-Managed** | `SERVER_MANAGED` | *(Future - not yet implemented.)* Messages are encrypted only in transit (TLS) and stored **plaintext** (or with a server-held key) on the companion server. The server can read and index message content. No client-side key management is required. See section 3.1 for design notes. |

### Mode Implications for Encryption

| Mode | Key Model | Who Can Decrypt |
|------|-----------|-----------------|
| POST_JOIN | Rotating group key, re-keyed on member join | Members from their join epoch onwards |
| FULL_ARCHIVE | Static group key, distributed to all members | Anyone with channel access |
| SERVER_MANAGED *(future)* | No client-side key; server-held key or plaintext | Server + any authorised client |

### Server-Side vs Client-Side Access Control

Server-side filtering (e.g. checking `pchat_member_join` before
returning messages) is strictly a
**bandwidth optimization**. It reduces unnecessary network traffic by
not sending ciphertext that clients cannot decrypt anyway.

**The client's cryptographic keys are the sole enforcer of access
control.** Even if a malicious or buggy server delivers messages it
should have filtered out, those messages remain undecryptable without
the correct key material. Clients MUST NOT rely on the server to
enforce confidentiality; they MUST:

1. Rotate channel keys (new epoch) on roster changes (see section 5.2).
2. Never share old epoch keys with newly joined members.
3. Treat any message they cannot decrypt as inaccessible (silently
   discard or display a "cannot decrypt" placeholder).

This separation means a compromised companion server cannot grant
unauthorized access to historical messages - the worst it can do is
deliver ciphertext the client has no key for.

### 3.1 Future: SERVER_MANAGED Mode (Client-to-Server Encryption Only)

> **Status: design placeholder -- not yet implemented.**
> This mode is documented here so that all protocol structures,
> wire formats, and trait abstractions are designed with forward
> compatibility in mind. Implementations MUST reserve `pchat_mode = 3`
> for this purpose and MUST NOT assign that value to another mode.

#### Motivation

POST_JOIN and FULL_ARCHIVE provide strong E2E guarantees but require
complex client-side key management (identity seeds, epoch ratchets,
consensus, TOFU, custodians). For some deployments -- particularly
private gaming communities where the server operator is fully trusted
-- this overhead is unnecessary. A simpler mode where the server
stores messages in plaintext (or encrypted with a server-held key)
eliminates all key exchange, all client-side key storage, and all
trust verification UX, while still providing:

- **Transport encryption**: Messages are protected in transit by the
  existing Mumble TLS channel.
- **Persistent history**: The companion server stores messages and
  serves them to authorised clients on reconnect.
- **Server-side access control**: The server enforces who can read
  what (e.g. based on Mumble ACLs or registration status), rather
  than relying on client-side key possession.
- **Server-side search and indexing**: Because the server can read
  message content, it can offer full-text search, spam filtering,
  moderation tools, and audit logs -- features that are impossible
  with E2E encryption.

#### Design Constraints (for future implementation)

1. **`pchat_mode = 3` (SERVER_MANAGED)**: Reserved in the
   `ChannelState` protobuf extension. Clients that encounter this
   value MUST understand the mode but MAY refuse to use it (e.g.
   display a warning that messages are not end-to-end encrypted).

2. **No client-side key exchange**: The `fancy-pchat-key-announce`,
   `fancy-pchat-key-request`, and `fancy-pchat-key-exchange` flows
   are skipped entirely for SERVER_MANAGED channels. The `KeyManager`
   is not involved.

3. **Message format**: The `fancy-pchat-msg` payload is sent with the
   `envelope` field containing a **plaintext** `MessageEnvelope`
   (serialised but not encrypted). The `epoch`, `chain_index`, and
   `epoch_fingerprint` fields are omitted (or set to zero / empty).
   A new boolean field `encrypted: false` (or absence of a version
   byte in the envelope) signals to the companion server that no
   decryption is needed.

4. **Server-side storage**: The companion stores the plaintext
   `MessageEnvelope` directly. It MAY apply server-side encryption at
   rest (e.g. database-level encryption or a server-held symmetric
   key) -- this is an operational choice, not a protocol concern.

5. **Fetch responses**: `fancy-pchat-fetch-resp` returns messages with
   plaintext envelopes. The client deserialises them directly without
   calling `KeyManager.decrypt()`.

6. **Access control model**: The server enforces access based on
   Mumble ACLs, user registration status, or channel membership. The
   companion server is the sole authority for who receives which
   messages. There is no client-side access control.

7. **Trust indicator**: The client MUST display a clear indicator that
   the channel is **not** end-to-end encrypted. Suggested UX: an open
   lock icon with the text "Server-managed -- not E2E encrypted.
   The server operator can read messages in this channel."

8. **Trait compatibility**: `PersistenceMode::ServerManaged` MUST be
   added to the `PersistenceMode` enum. `MessageProvider` and
   `CompositeMessageProvider` MUST handle it without requiring a
   `KeyManager`. The `PersistentMessageProvider` should delegate to a
   sub-provider that skips all encryption/decryption.

9. **Wire format reservation**: The `mode` field in `fancy-pchat-msg`
   and `fancy-pchat-key-exchange` already uses string values
   (`"POST_JOIN"`, `"FULL_ARCHIVE"`). Reserve `"SERVER_MANAGED"` for
   this mode. Clients that do not support the mode MUST ignore (not
   reject) messages with `mode: "SERVER_MANAGED"`.

#### Security Trade-offs

| Property | E2E modes (POST_JOIN / FULL_ARCHIVE) | SERVER_MANAGED |
|----------|--------------------------------------|----------------|
| Server reads content | No | **Yes** |
| Transport encryption | TLS | TLS |
| Key management complexity | High (seeds, epochs, consensus) | **None** |
| Forward secrecy | Epoch-level (POST_JOIN) | **None** (server retains plaintext) |
| Server compromise impact | Ciphertext leak only | **Full plaintext exposure** |
| Search / moderation | Not possible server-side | **Full server-side capability** |
| Compliance / audit | Client-only | **Server-side audit logs** |

This mode explicitly sacrifices confidentiality from the server
operator in exchange for operational simplicity. It MUST be clearly
documented as such in both admin and user-facing documentation.

#### Implementation Checklist (for when this mode is built)

- [ ] Add `SERVER_MANAGED = 3` variant to `PersistenceMode` enum
- [ ] Update `ChannelState` protobuf extension handling to parse
      `pchat_mode = 3`
- [ ] Add `PlaintextMessageProvider` (no encryption, delegates
      directly to the companion server's storage)
- [ ] Update `CompositeMessageProvider` to route `ServerManaged`
      channels to `PlaintextMessageProvider`
- [ ] Update `fancy-pchat-msg` wire format: allow unencrypted
      envelope, omit epoch/chain fields
- [ ] Update `fancy-pchat-fetch-resp` handling: skip
      `KeyManager.decrypt()` for `SERVER_MANAGED` messages
- [ ] Add "not E2E encrypted" trust indicator in frontend
- [ ] Skip `fancy-pchat-key-*` flows for `SERVER_MANAGED` channels
- [ ] Update server companion to accept and store plaintext envelopes
- [ ] Write integration tests for `SERVER_MANAGED` message lifecycle

---

## 4. E2E Encryption Scheme

The encryption design draws on established practices from the Signal
Protocol (Double Ratchet for 1:1), the Messaging Layer Security (MLS)
protocol (RFC 9420 for group key agreement), and the Matrix/Megolm
protocol (group ratchet for efficient fan-out). The scheme is adapted
for Mumble's simpler trust model.

### Cryptographic Primitives

| Purpose | Algorithm | Notes |
|---------|-----------|-------|
| Asymmetric key pair | X25519 | Per-user identity key, derived from independent identity seed |
| Digital signatures | Ed25519 | Key-exchange authentication; derived from same seed as X25519 via birational map |
| Key agreement | X25519 Diffie-Hellman | Group DH for POST_JOIN/FULL_ARCHIVE |
| Symmetric encryption | XChaCha20-Poly1305 | 192-bit nonce (no nonce-reuse risk), AEAD |
| Key derivation | HKDF-SHA256 | Derive message keys from shared secrets |
| Message authentication | Poly1305 (part of AEAD) | Integrity + authenticity |
| Hashing | SHA-256 | Key fingerprints, commitment schemes |
| Random | OS CSPRNG | Nonces, ephemeral keys |

> **Ed25519 / X25519 relationship**: Both curves share the same
> underlying field (Curve25519). Each user derives an Ed25519 signing
> key pair alongside their X25519 DH key pair from the same identity
> seed (see section 5.1). In Rust, `ed25519-dalek` and `x25519-dalek`
> support this directly. The Ed25519 public key is included in
> `fancy-pchat-key-announce` so peers can verify signatures on
> key-exchange messages independently of the server.

### Why XChaCha20-Poly1305

- 192-bit nonce eliminates nonce-reuse concerns even with random
  generation (birthday bound at 2^96 messages).
- Constant-time, no padding oracles (unlike AES-CBC).
- Widely available in the `ring` / `chacha20poly1305` Rust crates,
  already a dependency via the existing OffloadStore.
- Performance is excellent on all platforms including Android without
  AES-NI.

### Encryption Envelope

Every encrypted message has this structure:

```
+--------------+-------------+-----------------------------+
| Version (1B) | Nonce (24B) | AEAD Ciphertext + Tag (16B) |
+--------------+-------------+-----------------------------+
```

- **Version byte**: `0x01` for XChaCha20-Poly1305 with HKDF-SHA256.
  Allows future algorithm upgrades.
- **Nonce**: 24 bytes, randomly generated per message.
- **Ciphertext**: The encrypted **padded plaintext** (see below).
- **Tag**: 16-byte Poly1305 authentication tag (appended by AEAD).

#### Plaintext-to-Ciphertext Pipeline

The AEAD plaintext input is constructed as follows:

```
MessageEnvelope (JSON/MessagePack)
        |
        v
  Serialize to byte array
        |
        v
  Append randomized padding (section 6.3)
        |
        v
  padded_plaintext = serialized_envelope || padding_bytes || pad_count (2B BE)
        |
        v
  XChaCha20-Poly1305 encrypt(key, nonce, padded_plaintext, AAD)
        |
        v
  ciphertext + tag
```

On decryption, the receiver:
1. Decrypts and authenticates the ciphertext.
2. Reads the last 2 bytes of the decrypted plaintext as a big-endian
   `u16` (`pad_count`).
3. Strips the last `pad_count` bytes to recover the serialized
   `MessageEnvelope`.
4. Deserializes the `MessageEnvelope`.

### Associated Data (AAD)

The AEAD additional authenticated data binds the ciphertext to its
context, preventing re-targeting attacks:

```
AAD = channel_id (4B big-endian) || message_id (16B UUID bytes) || timestamp (8B big-endian)
```

This ensures a ciphertext cannot be moved to a different channel or
have its ID/timestamp tampered with without failing authentication.

---

## 5. Key Management

### 5.1 Identity Keys

Every Fancy Mumble user generates a long-term **identity seed**
independently of their Mumble TLS client certificate. The seed is
generated once via the OS CSPRNG and stored in the app's secure
storage (OS keychain / encrypted file). The TLS certificate is used
only for Mumble transport authentication; the E2EE identity is fully
independent.

Two related key pairs are derived from the seed:

```
identity_seed = random(32)    // OS CSPRNG, generated once on first run
dh_keypair      = X25519::from_seed(
    HKDF-SHA256(ikm=identity_seed, salt="fancy-pchat-v1", info="x25519")
)
signing_keypair = Ed25519::from_seed(
    HKDF-SHA256(ikm=identity_seed, salt="fancy-pchat-v1", info="ed25519")
)
```

Both public keys (X25519 for DH, Ed25519 for signatures) are announced
via `fancy-pchat-key-announce` when connecting to a server. The
announcement is signed by BOTH the Ed25519 identity key AND the TLS
certificate (see section 6.8), which binds the E2EE identity to the
Mumble transport identity without coupling their lifecycles.

**Seed backup via BIP39 mnemonic**: On first run, the client MUST
present the user with a 24-word BIP39 mnemonic derived from the
`identity_seed` (256-bit entropy = 24 words). The user is prompted
to write down or securely store this phrase. On a new device or
after data loss, the user can restore their E2EE identity by entering
the mnemonic, which deterministically recovers `identity_seed` and
thus both key pairs.

```
mnemonic = bip39_encode(identity_seed)     // 24 words
identity_seed = bip39_decode(mnemonic)     // deterministic recovery
```

**Why decouple from TLS certificates**: Deriving the identity seed
from the TLS certificate private key (as in earlier designs) creates
tight coupling between the transport layer and E2EE:
- Regenerating or rotating the TLS certificate silently changes the
  E2EE identity, invalidating all peer trust (TOFU pins, manual
  verifications).
- Certificate loss means permanent E2EE identity loss with no
  recovery path.
- The TLS private key may be stored in formats or locations that
  make extraction for HKDF derivation problematic (HSMs, OS-managed
  key stores).

With an independent seed, the user controls their E2EE identity
lifecycle independently of certificate management.

### 5.2 POST_JOIN Mode Keys (Group Ratchet)

A rotating group key is maintained for the channel:

1. **Epoch**: A new epoch starts whenever the channel roster changes:
   a new member joins for the first time (registered identity, not
   session), OR an existing member is removed/banned/leaves
   permanently. This ensures old epoch keys are mathematically
   useless to removed members, regardless of what the server delivers.
2. **Epoch key**: Generated by the member who initiates the new epoch.
   Any currently online member who holds the current epoch key can
   initiate the next epoch (there is no designated "key admin").
3. **Key distribution** (decentralized):
   - When a new member joins, the server broadcasts a
     `fancy-pchat-key-request` to all online Fancy Mumble members
     in the channel.
   - Each responding member independently generates a new epoch
     key, encrypts it to the new member's identity public key, and
     sends a `fancy-pchat-key-exchange` back through the server
     (with `request_id` set to the request UUID).
   - The responder **signs the key-exchange payload** with their
     Ed25519 identity key (see section 6.6). The recipient MUST
     verify this signature using the sender's known Ed25519 public
     key before accepting the key.
   - The key-exchange includes a `parent_fingerprint` field:
     `SHA-256(previous_epoch_key)[0..8]`. This cryptographically
     chains the new epoch to the previous one.
   - The server relays up to `relay_cap` (default 3 for POST_JOIN)
     key-exchange responses from distinct senders to the new member.
     When `relay_cap` is reached, the request is marked fulfilled
     and late responses are silently dropped.
   - The epoch key is NEVER distributed to users who were removed
     before the epoch started.
   - If **no members are online** when the new user joins, the server
     queues the request. When the first authorized member reconnects,
     the server delivers the pending request and that member responds
     (see section 7.3 for the full async flow).

   **Epoch broadcast to existing members**: After sending the
   key-exchange to the new member, the responder MUST also distribute
   the same new epoch key to every other existing online member of
   the channel. For each existing member, the responder sends a
   separate `fancy-pchat-key-exchange` message with:
   - `request_id: null` (distinguishes epoch broadcasts from
     key-request responses; the server skips relay tracking for
     these, see section 6.6).
   - `recipient_hash` set to the existing member's `cert_hash`.
   - `encrypted_key` encrypted to that member's X25519 public key.
   - The same `epoch`, `parent_fingerprint`, `epoch_fingerprint`,
     and `signature` fields as the key-exchange to the new member.

   Each existing member verifies `parent_fingerprint` against their
   current epoch key. If it matches and the Ed25519 signature is
   valid, they accept the new epoch. If `parent_fingerprint` does
   not match, they reject the key and raise a security alert
   (see section 5.4).

   **Epoch fork resolution** (deterministic tie-breaker): Because
   multiple members may simultaneously respond to the same
   `fancy-pchat-key-request` before the server marks it as fulfilled,
   a race condition can produce multiple competing epoch keys chained
   to the same parent. Without resolution, different members could
   end up on incompatible encryption states ("epoch fork").

   If a client receives multiple valid `fancy-pchat-key-exchange`
   messages for the same `(channel_id, epoch)` that are all chained
   to the same `parent_fingerprint`, it MUST apply a **deterministic
   tie-breaker**:

   1. **Select the canonical epoch key**: Among all candidates, the
      client accepts the epoch key from the sender with the
      **lexicographically smallest** `cert_hash` (compared as
      lowercase hex strings). This produces the same winner on every
      client regardless of message arrival order.
   2. **Discard losing forks**: All non-winning epoch keys are
      discarded. The client logs the discarded keys for auditing.
   3. **Re-send affected messages**: If the client sent any messages
      encrypted with a losing epoch key during the fork window, it
      MUST re-send those messages encrypted with the winning key.
      Each re-sent message gets a **new `message_id`** (to avoid the
      server's uniqueness constraint) but carries
      `replaces_id: <original_message_id>` (see section 6.2). This
      allows receiving clients to deduplicate: they replace the
      stale-epoch copy with the correctly-encrypted replacement.
      Recipients that receive messages encrypted with a discarded
      epoch key (and no replacement arrives) log a "stale epoch"
      warning.
   4. **Grace period**: To minimise fork-window messages, clients
      SHOULD buffer outbound messages for a brief period (recommended
      2 seconds) after detecting a roster change (new epoch trigger)
      before sending messages with the new epoch key. This gives time
      for the epoch broadcast to converge.

   > **Note**: To ensure the newly joined member can participate in
   > fork resolution, the server's `relay_cap` for POST_JOIN is set
   > to 3 (not 1). This allows the new member to receive the initial
   > cluster of racing key-exchange responses, apply the deterministic
   > tie-breaker, and converge on the same canonical epoch key as
   > existing members (who also receive epoch broadcasts via
   > `request_id: null` messages).
4. **Ratchet**: Within an epoch, a symmetric ratchet derives per-message
   keys:
   ```
   chain_key[0]   = epoch_key
   chain_key[n+1] = HKDF-SHA256(ikm=chain_key[n], info="fancy-pchat-chain-v1")
   message_key[n] = HKDF-SHA256(ikm=chain_key[n], info="fancy-pchat-msg-v1")
   ```
5. **Forward secrecy boundary**: Forward secrecy operates at the
   **epoch level**, not the intra-epoch message level. When the
   roster changes (user join/leave) a new epoch begins with a fresh
   epoch key, preventing former members from reading future messages.

   **Within an epoch**, intermediate chain keys are deleted after
   deriving the next one (limiting the window of exposure if a single
   chain key is compromised), but the **epoch key itself is retained
   locally** for the duration of `pchat_retention_days`. This is
   necessary because the persistent chat model requires that users
   who were offline can fetch and decrypt historical messages from
   within epochs they participated in.

   > **Note**: True per-message forward secrecy (similar to Signal's
   > Double Ratchet) is incompatible with persistent, fetchable
   > history. This is an intentional design trade-off, analogous to
   > Matrix's Megolm protocol.  Epoch transitions (roster changes)
   > remain the primary forward-secrecy boundary.

   **Key retention policy**:
   - `epoch_key` for each epoch the client participated in is stored
     locally for `pchat_retention_days` (from `ChannelState`). This
     allows re-deriving `chain_key[n]` and `message_key[n]` for any
     message index `n` within that epoch.
   - Intermediate `chain_key[n]` values are still deleted after
     forward-ratcheting to `chain_key[n+1]`. To decrypt message `n`
     from history, the client re-derives the chain from `epoch_key`
     forward to the required index.
   - After `pchat_retention_days` elapses, the epoch key is purged.
     Messages from purged epochs can no longer be decrypted.
   - This retention period also applies to received keys: if a peer
     sends an epoch key via key-exchange, the client records the
     epoch and its retention deadline based on the current
     `pchat_retention_days` value at time of receipt.
6. **New member**: When a user joins, a new epoch begins. The new user
   receives the new epoch key but NOT previous epoch keys, so they
   cannot decrypt messages from before their join.

### 5.3 FULL_ARCHIVE Mode Keys

A single static channel key is used:

1. **Channel key**: Generated by the first Fancy Mumble user to join
   the channel. Any online member who holds the channel key can
   distribute it to new members (there is no designated key holder).
2. **Distribution** (decentralized with client-enforced consensus):
   When a new member joins, the server broadcasts a
   `fancy-pchat-key-request` to all online Fancy Mumble members in
   the channel. Unlike the POST_JOIN first-responder model,
   FULL_ARCHIVE uses **multi-confirmation consensus** that is
   **enforced entirely by the receiving client**, not the server.

   The server relays key-exchange responses up to a `relay_cap`
   (a bandwidth/performance cap, NOT a security parameter). All
   responders encrypt the **same existing channel key** to the new
   member's identity public key and sign the key-exchange with their
   Ed25519 identity key.

   **Client-enforced consensus threshold**: The receiving client
   computes its own required consensus threshold from the channel
   members it directly observes in Mumble's native `ServerState`
   (i.e. the users it sees in the channel via `UserState` messages
   during handshake, which carry cert hashes and session IDs):
   ```
   required_threshold = clamp(floor(observed_members / 2), 1, 5)
   ```
   where `observed_members` is the number of other Fancy Mumble v2+
   users the client sees in the channel (known from `UserState` +
   prior `fancy-pchat-key-announce` messages). The client does NOT
   trust any server-provided member count for this computation.

   **Collection window**: After receiving the first key-exchange
   response for a `request_id`, the client opens a **10-second
   collection window** and accumulates all responses that arrive
   within that window. After the window closes, the client evaluates
   consensus:
   - If `>= required_threshold` distinct peers provided matching
     keys: trust level = **Verified**.
   - If responses disagree on the key value: trust level =
     **Disputed**, raise security alert.
   - If fewer than `required_threshold` responses arrived but all
     agree: trust level = **Unverified** (with a UI warning:
     "Only N of M expected members confirmed this key").

   **Why the server cannot subvert this**: The client independently
   knows which users are in the channel (from `ServerState`) and
   which are Fancy Mumble v2+ (from `fancy-pchat-key-announce`).
   A compromised server can suppress responses (leading to
   Unverified, which the user sees), but cannot forge responses
   (Ed25519 signatures prevent that). Forwarding colluding malicious
   responses is limited by the fact that the client requires
   `required_threshold` distinct peers it already recognises.

   **Key custodian trust shortcut**: If a key-exchange response is
   signed by a **key custodian** (a user whose cert hash appears in
   `ChannelState.pchat_key_custodians`, which the client observes
   directly; see section 5.7) or the **channel key originator** (the
   first user who generated the channel key, whose cert hash the
   client records locally on first key creation), the receiving
   client MAY immediately accept the key with **Verified** trust
   level, bypassing the collection window and multi-confirmation.
   This provides a fast path when a trusted authority is online. If
   no custodian or originator responds (or the client does not
   recognise any responder as such), the standard client-enforced
   consensus applies.

   If only one member is online (singleton channel), the client
   accepts with an "unverified" trust level and displays a warning.
   The async store-and-forward flow (section 7.3) handles the case
   where no members are online.
3. **New members**: Receive the same channel key, giving them access to
   all stored history.
4. **Key rotation**: Only on admin-initiated reset (e.g., after removing
   a compromised user). Any online member who holds the current key
   generates the new key and distributes it to all other online
   members. The server queues distribution for offline members.
5. **Trust establishment**: After accepting a channel key, the trust
   level is determined by the verification workflow (section 5.4):
   key custodian shortcut (Verified), multi-confirmation consensus
   (Verified), inline countersignature (Verified), or TOFU
   (Unverified) with an optional OOB verification prompt.
   As a supplementary check, the client MAY fetch recent messages
   and attempt trial decryption -- but this is a diagnostic signal
   only and does NOT promote trust level (see `check_key_by_decryption`
   in section 8.3).

**Trade-off**: FULL_ARCHIVE sacrifices forward secrecy for convenience.
If a user's identity key is compromised, all historical messages they
had access to are exposed. This is explicitly documented to server
admins.

### 5.4 Key Verification & Trust Levels

Every received key has an associated trust level that determines how
the client treats it:

| Trust Level | Meaning | UI Indicator |
|-------------|---------|--------------|
| **Manually Verified** | Key fingerprint confirmed via out-of-band comparison with a trusted member (see section 5.6) | Green shield icon with checkmark |
| **Verified** | Key confirmed via client-enforced multi-confirmation consensus (collection window), or signed by a key custodian/channel originator (see 5.7) | Green lock icon |
| **Unverified (TOFU)** | Key accepted on first use, not yet confirmed by consensus or out-of-band verification; user is protected against passive surveillance but vulnerable to active MITM | Yellow lock icon with "Unverified" tag |
| **Disputed** | Conflicting keys received from different members; key custodian fallback or manual OOB resolution required (see 5.4, Disputed Resolution) | Red warning icon with "Conflicting keys" banner |

**Verification workflow**:

1. **On key-exchange receive**: Verify the Ed25519 signature. If
   signature verification fails, reject the key immediately (do not
   store or use it). **Timestamp freshness check**: reject if the
   key-exchange `timestamp` is outside the acceptable window relative
   to the original `request_timestamp` (see section 6.6).
2. **Key custodian shortcut** (FULL_ARCHIVE): If the sender is a
   known key custodian (see section 5.7) or the channel key
   originator, immediately accept the key as verified (no multi-
   confirmation needed).
3. **Client-enforced multi-confirmation** (FULL_ARCHIVE): Open a
   10-second collection window after the first response. Accumulate
   all responses for the same `request_id`. After the window closes,
   compare all decrypted keys. If `>= required_threshold` (computed
   from locally observed members) agree: verified. Any mismatch:
   enter **Disputed resolution** (see below). Fewer than threshold
   but all agree: unverified (TOFU).
4. **TOFU acceptance** (all modes): If multi-confirmation is not
   achievable (e.g. only one member online, or first-ever join with
   no parent chain to check), the client accepts the key under TOFU
   (Trust On First Use). The key is usable immediately, but the UI
   shows it as **Unverified** with a clickable prompt to perform
   out-of-band verification (see section 5.6). This mirrors the
   established UX pattern from Signal, Matrix, and WhatsApp.
5. **Out-of-band fingerprint verification** (optional, promotes to
   Manually Verified): The user can compare the channel key
   fingerprint with a trusted member via voice, in-person, or any
   external channel. On match, the key is promoted to **Manually
   Verified** (highest trust level). See section 5.6 for the
   fingerprint display formats.
6. **Epoch fingerprint cross-check** (POST_JOIN): Each
   `fancy-pchat-msg` includes an `epoch_fingerprint` field
   (`SHA-256(epoch_key)[0..8]`). Compare against the locally held key.
   Mismatch means the client holds the wrong key for that epoch.
7. **Parent fingerprint chain** (POST_JOIN): When receiving a new epoch
   key, verify `parent_fingerprint` matches the hash of the locally
   held previous epoch key. Chain break = disputed.

**First-join limitation**: When a brand-new user joins a POST_JOIN
channel, they have no previous epoch key to check `parent_fingerprint`
against. In this case the client MUST accept the key under TOFU
(Unverified) and surface the out-of-band verification prompt. The
parent_fingerprint chain only provides value for subsequent epoch
transitions, not the initial key receipt.

#### Disputed Resolution

When conflicting keys are detected (consensus mismatch or
`parent_fingerprint` chain break), the client does NOT immediately
lock the channel to read-only. Instead, it applies a prioritised
resolution strategy:

1. **Key custodian trust shortcut**: Check if any of the conflicting
   key-exchange payloads came from a known key custodian or channel
   originator (determined from locally cached
   `ChannelState.pchat_key_custodians`; see section 5.7). If exactly
   one key was distributed by a custodian, silently discard the non-
   custodian key(s) and accept the custodian's key as **Verified**.
   The custodian's signing key identity is already trusted, so this
   is safe. Log the discarded key(s) for auditing.

2. **Inline countersignature shortcut**: If any of the conflicting
   key-exchange payloads carries a valid `countersignature` from a
   known key custodian (see section 6.6), accept that key as
   **Verified** and discard the others.

3. **Manual peer selection** (fallback): If no key custodian is
   present in the conflicting batch, the client shows a
   **non-blocking** warning banner:
   > "Conflicting encryption keys detected. Verify with a trusted
   > member to resolve. [Compare fingerprints]"

   The channel remains **readable** (using the majority key if one
   exists, or the most recent key) but **write-locked** until
   resolved. The user can click "Compare fingerprints" to open the
   verification dialog (section 5.6.3) and manually select which
   peer's key to trust via out-of-band comparison. Once verified,
   the selected key is promoted to **Manually Verified** and the
   conflicting key is permanently discarded.

4. **Automatic resolution timeout**: If the user does not manually
   resolve within 24 hours and a subsequent epoch transition occurs
   with a valid `parent_fingerprint` chain from the majority key
   **AND** a valid key custodian countersignature (standalone or
   inline), the dispute is auto-resolved in favour of the
   countersigned key. Without a countersignature, auto-resolution
   does NOT occur -- the dispute remains until manually resolved via
   OOB verification (step 3) or a key custodian comes online and
   countersigns the epoch. This prevents an attacker from forging
   subsequent epoch transitions to auto-resolve a dispute in their
   favour.

### 5.5 Key Storage (Client-Side)

```
{app_data_dir}/keys/
    identity_seed.key         # 32-byte identity seed (encrypted at rest with OS keychain)
    identity.pub              # X25519 public key (derived from seed)
    signing.pub               # Ed25519 public key (derived from seed)
    peers/
        {cert_hash}.pub       # Cached peer public keys + highest known announce timestamp
    channels/
        {server_hash}/
            {channel_id}.epoch    # Current epoch key + chain state (POST_JOIN)
            {channel_id}.history  # Retained epoch keys for historical decryption (POST_JOIN)
            {channel_id}.archive  # Static channel key (FULL_ARCHIVE)
            {channel_id}.trust    # Trust level + verification state
            {channel_id}.custodians # Pinned pchat_key_custodians list (TOFU, see 5.7)
```

### 5.6 Key Verification UX

The protocol provides two verification experiences based on the user's
preferences (controlled via Settings > Advanced > "Expert mode").

#### 5.6.1 Simple Mode (Default): Trust On First Use (TOFU)

For casual gaming communities and non-critical channels, strict
out-of-band verification is too much friction. The default mode uses
TOFU, following the same UX pattern as Signal, Matrix, and WhatsApp:

1. When the user joins a persistent channel and receives the epoch or
   channel key, the client accepts it immediately and marks it as
   **Unverified (TOFU)**.
2. The channel header shows a subtle, non-intrusive indicator: a
   **yellow shield** icon with the text "Unverified".
3. Clicking the yellow shield opens a tooltip:
   > "This channel's encryption key has not been verified.
   > You are protected against passive eavesdropping, but an active
   > attacker could intercept your messages.
   > [Verify with a key custodian] to turn this shield green."
4. The "Verify with a key custodian" link opens the verification
   dialog (see 5.6.3).
5. If the user does not verify, the channel remains fully functional
   with the yellow indicator. Messages are encrypted and decryptable;
   the only risk is an active server MITM (which requires the server
   operator to be the adversary).

**Key change detection**: If the channel key changes unexpectedly
(e.g. a new epoch key that does not chain from the previous one via
`parent_fingerprint`), the client displays a prominent warning:
> "The encryption key for this channel has changed unexpectedly.
> This could indicate a security issue. [Review details]"

This is analogous to Signal's "safety number has changed" warning.

#### 5.6.2 Expert Mode: Visual Fingerprints

When expert mode is enabled in settings, the client displays richer
cryptographic fingerprint information and provides tools for manual
out-of-band verification.

**Fingerprint derivation**: The channel key fingerprint used for
verification is computed as:
```
full_fingerprint = SHA-256(channel_key || channel_id (4B BE) || mode (1B))
```
This produces a 32-byte (256-bit) fingerprint bound to the specific
channel and mode, preventing cross-channel fingerprint reuse.

**Display formats** (user can toggle between these in the verification
dialog):

1. **Emoji sequence**: The fingerprint bytes are mapped to a curated
   set of 256 visually distinct emoji (one emoji per byte). The first
   8 bytes (8 emoji) are displayed as the "short fingerprint" for
   quick comparison; the full 32 emoji are available via "Show full
   fingerprint". Humans are exceptionally good at spotting when an
   emoji is out of place in a memorised sequence.

   Example (short): `lion guitar rocket pizza anchor tree moon star`

   The emoji set MUST be deterministic and identical across all Fancy
   Mumble clients. It is defined as a static lookup table in the
   client code (256 entries, one per byte value 0x00-0xFF), using
   only emoji that are visually distinct at small sizes and available
   on all major platforms (Unicode 13.0+ baseline).

2. **Word list**: The fingerprint bytes are mapped to a standard word
   list (similar to PGP word list or BIP39). Each byte maps to one
   word, producing a human-readable phrase.

   Example (short): `bacon apple guitar sunset anchor marble dawn crystal`

   Two word lists are used (even-position and odd-position words, as
   in PGP) to make transposition errors audible when read aloud.

3. **Hex string**: The raw SHA-256 hex, grouped into 4-character
   blocks for readability:
   `a4f8 29b1 cc03 7de2  91fa 0b4d ...`

   This is always available as a fallback and is the format stored
   in trust records.

The verification dialog also shows:
- **Channel name** and **server** for context.
- **Key distributor**: who sent the key (cert hash + display name).
- **Trust level**: current level with explanation.
- **"I have verified this fingerprint"** button to promote to
  Manually Verified.

#### 5.6.3 Verification Dialog

The verification dialog is accessible from:
- The channel header shield/lock icon (click).
- Settings > Channels > [channel] > Security.
- Right-click channel > "Verify encryption".

**Dialog contents**:

```
+-------------------------------------------------------+
| Channel Encryption Verification                       |
+-------------------------------------------------------+
| Channel: #general                                     |
| Mode: FULL_ARCHIVE                                    |
| Key distributed by: Alice (a4f8...)                   |
|                                                       |
| Fingerprint:                                          |
|                                                       |
|   [Emoji]  [Words]  [Hex]        <- toggle tabs       |
|                                                       |
|   lion guitar rocket pizza                             |
|   anchor tree moon star                                |
|                                                       |
|   [Show full fingerprint]                              |
|                                                       |
| Compare this fingerprint with a trusted channel       |
| member using voice chat, in person, or another        |
| secure channel.                                       |
|                                                       |
| Current trust: Unverified (TOFU)                      |
|                                                       |
| [ ] I have verified this fingerprint matches          |
|     [Mark as Verified]                                |
+-------------------------------------------------------+
```

When the user clicks "Mark as Verified", the trust level is promoted
to **Manually Verified** and the channel icon changes to a green
shield with a checkmark. This state persists across sessions (stored
in `{channel_id}.trust`).

If the key subsequently changes (new epoch or key rotation), the
trust level resets to **Unverified (TOFU)** and the user is prompted
to re-verify (analogous to Signal's safety number reset).

#### 5.6.4 Creator Countersignature (POST_JOIN Epoch Transitions)

To address the weakness that `parent_fingerprint` is uncheckable for
a brand-new user, and that any online member can generate a new
epoch key, the channel creator (or a designated key custodian; see
section 5.7) SHOULD countersign epoch transitions:

1. When a new epoch key is generated (by any member), the generator
   broadcasts the new `epoch_fingerprint` to the channel.
2. The channel creator (or key custodian), upon receiving the new
   epoch key and verifying `parent_fingerprint` chain continuity,
   signs:
   ```
   countersig_data = channel_id (4B BE) || epoch (4B BE)
                  || epoch_fingerprint (8B)
                  || parent_fingerprint (8B)
                  || timestamp (8B BE)
                  || distributor_hash (UTF-8 bytes)
   countersignature = Ed25519_sign(creator_signing_key, countersig_data)
   ```
   The `distributor_hash` is the cert hash of the user whose key
   distribution this countersignature endorses. For standalone
   countersigs (broadcast to the channel), this equals the
   `signer_hash` (the custodian endorses themselves). For inline
   countersigs embedded in a key-exchange (section 6.6), this equals
   the `sender_hash` of the key-exchange payload. Including
   `distributor_hash` in the signed data prevents a compromised
   server from extracting a countersignature from one user's
   key-exchange and attaching it to a different sender's payload.
3. The countersignature is broadcast as a
   `fancy-pchat-epoch-countersig` message (new `dataID`):
   ```
   {
     "channel_id": u32,
     "epoch": u32,
     "epoch_fingerprint": bytes,
     "parent_fingerprint": bytes,
     "signer_hash": string,
     "distributor_hash": string,   // cert hash of endorsed distributor (= signer_hash for standalone)
     "timestamp": u64,
     "countersignature": bytes
   }
   ```
4. Clients receiving the countersignature verify it against the
   known key custodian's Ed25519 public key. If valid, the epoch key
   is promoted to **Verified** even for new users who cannot check
   the parent chain themselves.
5. If no key custodian is online to countersign, the epoch key
   remains at its current trust level (TOFU for new users, or
   Verified for existing members who verified the chain).

**Replay prevention**: The `timestamp` (Unix epoch milliseconds) is
included in the signed data to prevent replay attacks. Clients MUST
reject a countersignature if:
- `timestamp` is more than 5 minutes in the past relative to the
  client's local clock, or
- a countersignature for the same `(channel_id, epoch, signer_hash)`
  with a higher timestamp has already been accepted.

This ensures an attacker cannot replay a legitimately signed
countersignature from a previous epoch transition.

**Atomic delivery via key-exchange**: Key custodians can also embed their
countersignature directly into the `fancy-pchat-key-exchange`
payload (see section 6.6, `countersignature` and
`countersigner_hash` fields). When present, the key and its trust
verification arrive as a single indivisible unit, avoiding the race
condition where the key arrives but the separate countersig message
is delayed or suppressed.

This is an optional but strongly recommended mechanism. Channels
where a key custodian has countersigned all epoch transitions provide
stronger guarantees than pure TOFU.

### 5.7 Key Custodians

Mumble's native ACL system does not expose a per-channel "admin" flag
to other clients -- a user only learns their *own* permission bitmask
via `PermissionQuery`. To give persistent chat a visible, per-channel
authority role that all clients can verify, Fancy Mumble introduces
the **key custodian** concept.

**Definition**: A key custodian is a user whose TLS certificate hash
appears in the `pchat_key_custodians` repeated field on `ChannelState`
(protobuf field 103). Key custodians are the trusted authorities for
a channel's encryption key lifecycle.

**Responsibilities**:
- Countersign epoch transitions (`fancy-pchat-epoch-countersig`).
- Serve as the key custodian trust shortcut during key distribution
  (their key-exchange responses are immediately accepted as Verified).
- Automatically resolve Disputed states (their key wins over non-
  custodian keys).

**Who can set key custodians**: Any user with channel operator
permissions (the same ACL check as changing a channel's name or
description). The operator sends a `ChannelState` update with the
desired `pchat_key_custodians` list. Murmur persists the field and
re-broadcasts the updated `ChannelState` to all connected clients.

**Client-side verification**: Clients learn the key custodian list
from `ChannelState` messages received during handshake and on channel
updates -- the same mechanism used for `pchat_mode`. When evaluating
whether a key-exchange sender is a trusted authority, the client
checks if `sender_hash` is in the locally cached
`pchat_key_custodians` list for that channel. This is entirely
client-side; the server has no role in trust evaluation beyond
relaying the `ChannelState`.

**Custodian list TOFU**: The client applies Trust On First Use to the
custodian list itself:

1. **Initial pin**: When the client first joins a persistent channel
   and receives the `ChannelState` containing `pchat_key_custodians`,
   it persists this list locally in
   `{channel_id}.custodians` (see section 5.5).

   **First-join trust restriction**: If the custodian list is
   **populated** (non-empty) on first observation, the pinned list
   is stored with `confirmed = false`. In this state, the custodians
   are pinned for TOFU change-detection purposes but do **NOT** grant
   consensus-bypass (trust shortcut) privileges. Keys distributed by
   these custodians are treated as ordinary peer responses, subject
   to normal consensus rules. The client MUST show a one-time prompt:
   > "This channel is managed by N key custodian(s). Review and
   > confirm to enable accelerated key verification.
   > [Review custodians] [Confirm]"

   Until the user explicitly confirms, `is_trusted_authority()`
   returns false for all custodians in the unconfirmed list.
   This prevents a compromised server from injecting a malicious
   custodian into the `ChannelState` sent to a new user and
   bypassing consensus via the trust shortcut.

   When the user confirms, `confirmed` is set to true and the
   custodians gain full trust shortcut privileges. If the custodian
   list is **empty** on first join (fallback to channel originator),
   no confirmation is needed (there is nothing to inject).
2. **Change detection**: On subsequent `ChannelState` updates, the
   client compares the incoming `pchat_key_custodians` list against
   the pinned list. If the lists differ (custodians added, removed,
   or reordered), the client MUST:
   - Show a prominent **"Channel Authority Changed"** warning banner:
     > "The channel's key custodians have changed. New custodians
     > will not be trusted until you accept this change.
     > [Review changes] [Accept]"
   - Display which cert hashes were added and which were removed.
   - **NOT grant trust shortcuts to newly added custodians** until the
     user explicitly accepts the change. Removed custodians lose their
     trust shortcut immediately.
   - Until accepted, the client uses only the previously pinned list
     for trust evaluation. Key-exchange responses from new (unaccepted)
     custodians are treated as ordinary peer responses (subject to
     normal consensus rules, not the custodian bypass).
3. **Acceptance**: When the user clicks "Accept", the client updates
   the pinned list to the new `pchat_key_custodians` and the new
   custodians gain full trust shortcut privileges.
4. **Empty-to-populated transition**: If the pinned list was empty
   (fallback to channel originator) and custodians are now configured
   for the first time, the same warning is shown -- the user must
   acknowledge the new authority structure.
5. **Persistence**: The pinned custodian list is stored alongside
   channel key material (section 5.5) and survives app restarts.

**Fallback -- channel key originator**: If no key custodians are
configured (empty list), the client falls back to trusting the
**channel key originator** -- the cert hash of the first user who
generated the channel's encryption key, tracked locally by the
client. This ensures the key custodian trust shortcut works even
when the server operator has not explicitly designated custodians.

**Security note**: The `pchat_key_custodians` field is set by whoever
has channel operator permissions on the Mumble server. A compromised
server can add arbitrary cert hashes to this list. The custodian list
TOFU mechanism (above) mitigates this: the client pins the list on
first use and requires explicit user acceptance before granting trust
shortcuts to newly added custodians. Combined with the
countersignature mechanism (section 5.6.4), which still requires valid
Ed25519 signatures, even a tampered custodian list cannot produce
valid countersignatures without the corresponding private key.

---

## 6. Wire Format

All payloads inside `PluginDataTransmission.data` are MessagePack-encoded
(compact binary, schema-flexible, widely supported). We avoid JSON for
efficiency since encrypted messages can be large.

### 6.1 Channel Persistence Config (ChannelState protobuf fields)

Not a `PluginDataTransmission` payload. Persistence configuration is
carried directly in the `ChannelState` protobuf message via extension
fields added by Fancy Mumble at high field IDs (100+). Legacy servers
and clients silently ignore unknown fields per standard protobuf
behaviour.

```protobuf
message ChannelState {
    // ... standard Mumble fields (1-13) ...

    // Fancy Mumble persistent chat extension (field IDs 100+)
    enum PchatMode {
        PCHAT_NONE           = 0;
        PCHAT_POST_JOIN      = 1;
        PCHAT_FULL_ARCHIVE   = 2;
        PCHAT_SERVER_MANAGED = 3;
    }
    optional PchatMode pchat_mode        = 100; // persistence mode for this channel
    optional uint32 pchat_max_history    = 101; // max messages stored (0=unlimited)
    optional uint32 pchat_retention_days = 102; // auto-delete after N days (0=forever)
    repeated string pchat_key_custodians = 103; // cert hashes of key custodians (see 5.7)
}
```

| Field | Values | Default |
|-------|--------|---------|
| `pchat_mode` | `PCHAT_NONE` (0), `PCHAT_POST_JOIN` (1), `PCHAT_FULL_ARCHIVE` (2), `PCHAT_SERVER_MANAGED` (3, *future*) | absent = NONE |
| `pchat_max_history` | `0` = unlimited, else max messages | server default |
| `pchat_retention_days` | `0` = forever, else days | server default |
| `pchat_key_custodians` | cert hash strings | empty list |

Clients receive these fields as part of the standard `ChannelState`
messages during handshake and on channel updates. No additional
request/response cycle is needed.

Admins configure persistence by sending a `ChannelState` message with
the desired `pchat_*` fields set - the same mechanism used for
changing a channel's name or description. Murmur persists the fields
in its database and re-broadcasts the updated `ChannelState` to all
connected clients.

### 6.2 fancy-pchat-msg (Client to Server)

```
{
  "message_id": string (UUID),
  "channel_id": u32,
  "timestamp": u64,           // Unix epoch millis
  "sender_hash": string,      // user cert hash (identity, not session)
  "mode": "POST_JOIN" | "FULL_ARCHIVE",
  "envelope": bytes,          // encrypted MessageEnvelope (see 4)
  "epoch": u32,               // only for POST_JOIN mode
  "chain_index": u32,         // only for POST_JOIN mode
  "epoch_fingerprint": bytes, // SHA-256(epoch_key)[0..8] for key cross-verification
  "replaces_id": string | null, // if non-null, this message replaces a previous message
                                  // with the given message_id (epoch fork re-send, see 5.2)
}
```

**`replaces_id`**: When a client re-sends a message after epoch fork
resolution (see section 5.2, step 3), it generates a **new**
`message_id` for the replacement but sets `replaces_id` to the
original message's UUID. This avoids the server's
`(channel_id, sender_hash, message_id)` uniqueness rejection while
allowing receiving clients to deduplicate:

- When a client receives a message with `replaces_id` set, it checks
  **both** its persistent encrypted store **and** its volatile
  in-memory store for a message matching `replaces_id` from the same
  `sender_hash`. This cross-provider search is critical because
  dual-path delivery (section 7.1) means the original message may
  exist as a plaintext real-time `TextMessage` in the volatile store
  even if the encrypted `PluginDataTransmission` copy was never
  received (e.g. the client went offline before the encrypted copy
  arrived). If found in **either** store, the old message is
  **replaced** (overwritten) by the new one. If not found in any
  store, the new message is inserted normally.
- The server stores the replacement as a new row. It SHOULD also
  mark the original message (if it exists) as superseded (see server
  guide section 6.1).
- Only the message's own sender can set `replaces_id` (enforced by
  the `sender_hash` binding). A client MUST ignore `replaces_id` if
  the referenced message's `sender_hash` does not match the
  replacement's `sender_hash`.
- `replaces_id` is intended exclusively for epoch fork re-sends, not
  general message editing. A message with `replaces_id` set MUST
  also have a different `epoch` and/or `epoch_fingerprint` than the
  original (the client verifies this).

### 6.3 MessageEnvelope (plaintext before encryption)

```
{
  "body": string,             // message content (HTML)
  "sender_name": string,      // display name at send time
  "sender_session": u32,      // session at send time
  "attachments": [            // optional
    { "name": string, "mime": string, "data": bytes }
  ]
}
```

#### Padded plaintext byte layout

The `MessageEnvelope` is serialized to a byte array (JSON or
MessagePack). Randomized padding is then **appended** directly to the
serialized bytes -- it is NOT placed inside the JSON/MessagePack
structure. This ensures all client implementations produce an
identical byte layout regardless of serialization library.

```
+-----------------------------------------------+
| serialized MessageEnvelope  (variable length)  |
+-----------------------------------------------+
| 0x00 padding bytes          (pad_count - 2)    |
+-----------------------------------------------+
| pad_count                   (2B big-endian u16)|
+-----------------------------------------------+
         ^--- this entire block is the AEAD plaintext input
```

- `pad_count` includes itself (minimum value: 2).
- The padding algorithm is defined in section 10.1 (Ciphertext
  Padding): 256-byte block alignment + random 0-255 byte jitter.
- On decryption, the receiver reads the last 2 bytes as
  `pad_count` (big-endian u16), strips the last `pad_count` bytes,
  then deserializes the remaining bytes as the `MessageEnvelope`.
- The padding bytes MUST be `0x00`. Non-zero bytes in the padding
  region (excluding the `pad_count` trailer) SHOULD trigger a
  warning but MUST NOT cause decryption failure (forward
  compatibility).

### 6.4 fancy-pchat-fetch (Client to Server)

```
{
  "channel_id": u32,
  "before_id": string | null, // pagination cursor (message_id)
  "limit": u32,               // max messages to return (default 50)
  "after_id": string | null,  // fetch messages after this ID
}
```

### 6.5 fancy-pchat-fetch-resp (Server to Client)

```
{
  "channel_id": u32,
  "messages": [               // array of fancy-pchat-msg payloads
    { ... }
  ],
  "has_more": bool,           // more messages available (pagination)
  "total_stored": u32,        // total messages in store for this channel
}
```

### 6.6 fancy-pchat-key-exchange (Peer to Peer)

```
{
  "channel_id": u32,
  "mode": "POST_JOIN" | "FULL_ARCHIVE",
  "epoch": u32,               // key epoch number
  "encrypted_key": bytes,     // epoch/channel key encrypted to recipient's X25519 public key
  "sender_hash": string,      // identity of key distributor
  "recipient_hash": string,   // intended recipient identity
  "request_id": string | null, // references a fancy-pchat-key-request (for dedup)
  "timestamp": u64,           // Unix epoch millis when this key-exchange was created
  "algorithm_version": u8,    // must match sender's key-announce algorithm_version (section 6.8)
  "signature": bytes,         // Ed25519 signature (see below)
  "parent_fingerprint": bytes | null, // SHA-256(previous_epoch_key)[0..8], POST_JOIN only
  "epoch_fingerprint": bytes, // SHA-256(distributed_key)[0..8]
  "countersignature": bytes | null,   // key custodian Ed25519 countersignature (see 5.6.4)
  "countersigner_hash": string | null // cert hash of the countersigning key custodian
}
```

**Atomic countersignature**: When a key custodian distributes a key,
they SHOULD embed their countersignature directly in the key-exchange
payload by populating `countersignature` and `countersigner_hash`.
The countersignature covers the same data as the standalone
`fancy-pchat-epoch-countersig` message (see section 5.6.4), with
`distributor_hash` set to the key-exchange's `sender_hash`:
```
countersig_data = channel_id (4B BE) || epoch (4B BE)
               || epoch_fingerprint (8B)
               || parent_fingerprint (8B)
               || timestamp (8B BE)
               || distributor_hash (UTF-8 bytes)
```
The recipient verifies the countersignature against the
`countersigner_hash`'s known Ed25519 public key, independently of
the key-exchange `signature`. The recipient MUST also verify that
the `distributor_hash` inside the signed data matches the
key-exchange's `sender_hash`. If valid and the countersigner is a
known key custodian (see section 5.7), the key is immediately
promoted to **Verified**.

When a non-custodian distributes a key, these fields are `null`. The
countersignature, if needed, arrives separately via
`fancy-pchat-epoch-countersig`.

**Signature**: The sender MUST sign the following concatenation with
their Ed25519 identity private key:

```
signed_data = algorithm_version (1B)
           || channel_id (4B BE) || mode (1B: 1=POST_JOIN, 2=FULL_ARCHIVE)
           || epoch (4B BE) || encrypted_key || recipient_hash (UTF-8 bytes)
           || request_id (UTF-8 bytes, or empty if null)
           || timestamp (8B BE)
```

`algorithm_version` is included as the first byte to prevent
downgrade attacks: without it, an intermediary could re-label a
version-1 key-exchange as version-2 (or vice versa), causing the
recipient to misinterpret the key material. Because the version byte
is covered by the Ed25519 signature, any modification invalidates the
signature. The value MUST match the sender's `algorithm_version` from
their most recent `fancy-pchat-key-announce` (section 6.8). Recipients
MUST reject key-exchanges whose `algorithm_version` does not match
the sender's announced version or is unrecognised.

The recipient MUST verify this signature using the sender's known
Ed25519 public key (received via `fancy-pchat-key-announce`) **before**
decrypting `encrypted_key`. If verification fails, the entire
key-exchange is rejected.

**Timestamp freshness**: The `timestamp` field MUST record the moment
the responder created this key-exchange (Unix epoch milliseconds). The
recipient MUST reject the key-exchange if:

- `timestamp < request_timestamp` (the key-exchange claims to be older
  than the request that triggered it), or
- `timestamp > request_timestamp + 300_000` (the key-exchange is more
  than 5 minutes after the original request).

`request_timestamp` is the `timestamp` field from the
`fancy-pchat-key-request` that triggered this exchange (see section
6.7). Including `timestamp` in the signed data prevents an attacker
from replaying a legitimately signed key-exchange from a previous
request window.

This prevents:

- A compromised server from injecting forged key material.
- A malicious member from distributing keys under another member's
  identity (even if the server's `sender_hash` validation is bypassed).

**`parent_fingerprint`** (POST_JOIN only): The first 8 bytes of
`SHA-256(previous_epoch_key)`. Creates a cryptographic chain from each
epoch to its predecessor. Existing members who receive the new epoch
key verify this fingerprint matches their current epoch key. If the
responder does not hold a previous epoch key (first epoch in the
channel), this field is `null`.

**`epoch_fingerprint`**: The first 8 bytes of `SHA-256(distributed_key)`.
Allows recipients and existing members to cross-check that the
distributed key matches the key used in messages (see
`epoch_fingerprint` in `fancy-pchat-msg`, section 6.2).

**`request_id` semantics**:
- **Non-null** (`request_id` references a `fancy-pchat-key-request`):
  This is a response to a key request. The server applies relay
  tracking (increments `relays_sent`, drops when `relay_cap` reached).
  Used for sending the new epoch key to the **new member** who
  triggered the key request.
- **Null**: This is an **epoch broadcast** to an existing member
  (POST_JOIN only, see section 5.2). The server does NOT apply relay
  tracking; it simply validates `sender_hash` and forwards the
  message to `recipient_hash`. Epoch broadcasts propagate the new
  epoch key to existing online members who did not originate the
  request.

If a client receives multiple epoch broadcasts for the same
`(channel_id, epoch)` from different senders with valid
`parent_fingerprint` chains, it MUST apply the deterministic
tie-breaker described in section 5.2 (lowest `cert_hash` wins).

### 6.7 fancy-pchat-key-request (Server to Client)

Broadcast by the server to all online Fancy Mumble members in a channel
when a new member joins and needs key material. This is NOT sent by
clients - the server generates it in response to a new member's
`fancy-pchat-key-announce` arrival in a persistent channel.

```
{
  "channel_id": u32,
  "mode": "POST_JOIN" | "FULL_ARCHIVE",
  "requester_hash": string,   // cert hash of the user who needs a key
  "requester_public": bytes,  // X25519 public key of the requester (32 bytes)
  "request_id": string,       // server-generated UUID to deduplicate responses
  "timestamp": u64,           // Unix epoch millis when the server created this request
  "relay_cap": u32,           // max responses the server will relay (bandwidth cap, NOT a security parameter)
}
```

**Race condition handling**: Multiple members may respond simultaneously.
The server relays up to `relay_cap` valid `fancy-pchat-key-exchange`
responses that reference this `request_id`, then stops relaying
further responses for efficiency. Clients SHOULD add a small random
delay (0-500ms) before responding to reduce wasted work, but
correctness does not depend on this.

**`relay_cap` is NOT a security parameter**: The `relay_cap` exists
solely to limit the number of responses the server relays for
bandwidth/performance reasons. **Security** (consensus threshold) is
enforced entirely by the receiving client (see section 5.3). A
compromised server can set `relay_cap` to 1 and only forward a single
response; the client handles this correctly by marking the key as
Unverified (since fewer than `required_threshold` peers confirmed).
The server cannot forge additional confirmations because each response
requires a valid Ed25519 signature from a distinct known peer.

**Relay cap defaults**:
- **FULL_ARCHIVE**: `relay_cap = clamp(floor(online_members / 2), 1, 5) + 2`.
  The `+2` headroom ensures the client receives enough responses for
  its own threshold computation even if some arrive late.
- **POST_JOIN**: `relay_cap = 3` (allows the new member to receive
  competing epoch keys for deterministic fork resolution; see
  section 5.2).

**Client-enforced consensus** (FULL_ARCHIVE): The receiving client
computes its own `required_threshold` from the Fancy Mumble v2+
members it directly observes in the channel (via `ServerState` /
`UserState` messages). It does NOT use any server-provided count.
See section 5.3 for the full consensus algorithm.

**Key custodian trust shortcut**: When the receiving client detects
that a key-exchange response is signed by a user it independently
identifies as a key custodian (from `ChannelState.pchat_key_custodians`;
see section 5.7) or the channel key originator (from local records),
it MAY accept the key as Verified immediately. This is a purely
client-side decision; the server plays no role in trust evaluation.

**POST_JOIN** (relay_cap = 3): Multiple responders may independently
generate new epoch keys. The new member receives up to `relay_cap`
responses and applies the deterministic tie-breaker (lowest
`cert_hash` wins; see section 5.2). Existing members also receive
epoch broadcasts (`request_id: null`) and apply the same tie-breaker.
All clients verify the winning key via `parent_fingerprint` chain.

**Timestamp freshness**: The `timestamp` field records when the server
created this request. Clients store this value and use it to validate
the freshness of incoming `fancy-pchat-key-exchange` responses (see
section 6.6). Responding clients also reference this timestamp to
ensure they are answering a recent request.

**Per-channel and per-user limits**: The server MUST enforce:
- Max pending (unfulfilled) key requests per requester identity
  (default 5). If the per-user limit is reached, the server rejects
  with a `fancy-pchat-ack` status `"rejected"` and reason
  `"key_request_limit_exceeded"`.
- Soft max pending key requests per channel (default 100). When the
  per-channel limit is reached, the server does NOT reject the new
  request. Instead, it applies **FIFO eviction**: evict the oldest
  pending request from the requester identity that currently holds
  the most pending slots. This ensures no single actor can monopolise
  the queue. If all requesters have equal slot counts, evict the
  globally oldest request. The evicted request is silently dropped;
  the affected user can re-announce their key to re-enter the queue.

This eviction policy prevents a Denial of Service where an attacker
registers many sessions (or uses unauthenticated connections),
joins a persistent channel, and fills the entire queue with garbage
requests that would otherwise permanently lock out legitimate users.

### 6.8 fancy-pchat-key-announce (Client to Server)

```
{
  "algorithm_version": u8,    // 1 = X25519 + Ed25519 (Curve25519 family)
  "identity_public": bytes,   // key-agreement public key (32 bytes for algorithm_version 1)
  "signing_public": bytes,    // signature public key (32 bytes for algorithm_version 1)
  "cert_hash": string,        // TLS certificate hash (links E2EE identity to Mumble transport identity)
  "timestamp": u64,           // announcement time
  "signature": bytes,         // sign(signing_private, algorithm_version || cert_hash || timestamp || identity_public || signing_public)
  "tls_signature": bytes,     // sign(tls_private_key, algorithm_version || cert_hash || timestamp || identity_public || signing_public)
}
```

**Algorithm versioning**: The `algorithm_version` field identifies the
cryptographic algorithms used for `identity_public` and `signing_public`:

| `algorithm_version` | Key Agreement | Signature | Key Sizes |
|---------------------|---------------|-----------|-----------|
| `1` | X25519 | Ed25519 | 32 + 32 bytes |
| `2`+ | Reserved for future algorithms (e.g. post-quantum) | | |

Clients MUST reject announcements with an unrecognised
`algorithm_version` and SHOULD display a user-visible warning
suggesting a client upgrade. This prevents silent misinterpretation
of key material (e.g. parsing a 48-byte post-quantum key as a 32-byte
X25519 key). The `algorithm_version` is included in the signed data
for both signatures, so it cannot be downgraded by an intermediary.

**Dual-signature binding**: The announcement carries two signatures
over the same data (`algorithm_version (1B) || cert_hash ||
timestamp || identity_public || signing_public`):

1. **Ed25519 signature** (`signature`): Proves the announcer controls
   the signing private key corresponding to `signing_public`. Peers
   use this to verify future key-exchange signatures.
2. **TLS signature** (`tls_signature`): Proves the announcer controls
   the TLS certificate private key corresponding to `cert_hash`. The
   server MUST verify this signature using the session's TLS
   certificate public key **and** confirm `cert_hash` matches the
   session's actual TLS certificate hash.

Both signatures cover `algorithm_version` as the first byte of the
signed data, ensuring the algorithm identifier cannot be stripped or
modified without invalidating the signatures.

This dual-signature scheme binds the independent E2EE identity to the
Mumble transport identity without coupling their lifecycles: the
`identity_seed` can survive TLS certificate rotation by re-announcing
with the new certificate's `tls_signature`, while peers update their
`cert_hash -> (X25519, Ed25519)` mapping.

**Anti-rollback**: Clients MUST track the highest observed `timestamp`
for each peer's `cert_hash` in their local `peer_keys` store (the
`highest_announce_ts` field in `PeerKeyRecord`, see section 8.3). When
a `fancy-pchat-key-announce` is received:

1. If the announcement's `timestamp` is **less than or equal to** the
   stored `highest_announce_ts` for that `cert_hash`, the announcement
   is **silently discarded**. This prevents identity rollback attacks
   where an attacker replays an older key-announce to trick peers into
   accepting a previously-compromised key pair.
2. If the `timestamp` is strictly greater, the client updates
   `highest_announce_ts` and accepts the new public keys.
3. If no previous record exists for the `cert_hash` (first-time
   announcement), the announcement is accepted and recorded (TOFU).

The server SHOULD also enforce timestamp monotonicity: reject
`fancy-pchat-key-announce` messages whose `timestamp` is <= the
previously stored value for that `cert_hash`. This provides a
defence-in-depth layer, but the client-side check is the authoritative
guard since the server is not fully trusted.

### 6.9 fancy-pchat-ack (Server to Client)

```
{
  "message_id": string,
  "status": "stored" | "rejected" | "quota_exceeded",
  "reason": string | null,
}
```

---

## 7. Message Lifecycle

### 7.1 Sending a Message

```
User types message
        |
        v
[Client] Generate message_id (UUID v4), timestamp
        |
        v
[Client] Look up channel persistence mode
        |
  +-----+------+------+
  |  NONE      | POST_JOIN/FULL_ARCHIVE
  v             v
Send plain    Encrypt MessageEnvelope
TextMessage   with mode-specific key
  |             |
  |             v
  |           Build fancy-pchat-msg payload
  |             |
  |             v
  |           Send PluginDataTransmission
  |           (receiverSessions = [companion_session])
  |             |
  |             +----> Send plain TextMessage (for live delivery
  |                    to currently-online users, contains body
  |                    as cleartext for real-time display)
  |
  v
Done (volatile)
```

**Important**: The plaintext `TextMessage` provides real-time delivery
to all online users (including legacy clients). The encrypted
`PluginDataTransmission` provides durable storage. This dual-path
approach means:

- Online users see messages immediately (no decryption latency).
- The server companion stores the encrypted copy.
- On reconnect, clients fetch encrypted copies and decrypt locally.
- Legacy clients never see the `PluginDataTransmission` at all.

### 7.2 Fetching History on Connect

```
[Client] Connects, receives ServerSync
        |
        v
[Client] Receives ChannelState for each channel
        |   (standard Mumble handshake - already happens)
        |
        v
[Client] Parses pchat_mode from each ChannelState
        |   to determine persistence mode per channel
        |
        v
[Client] For each persistent channel (mode != NONE):
        |   Send fancy-pchat-fetch { channel_id, limit: 50 }
        v
[Server Companion] Looks up stored messages
        |
        v
[Server Companion] Sends fancy-pchat-fetch-resp
        |                with array of encrypted messages
        v
[Client] For each message:
        |   1. Check if already in local cache
        |   2. Decrypt using mode-appropriate key
        |   3. Insert into message store (sorted by timestamp)
        v
[Client] Emit UI events -> React renders history
```

### 7.3 Key Exchange on Channel Join

Key distribution is **decentralized**: any online channel member who
holds the current key can distribute it to a new member. There is no
designated "key admin". The server acts as a relay service only.
**Consensus is enforced by the client, not the server** (see section
5.3).

#### Normal Flow (online members present)

```
[Client A] Joins persistent channel (POST_JOIN or FULL_ARCHIVE)
        |
        v
[Client A] Sends fancy-pchat-key-announce
        |   (X25519 + Ed25519 public keys, cert hash, signature)
        v
[Server] Verifies signature, stores public keys
        |   Broadcasts key-announce to other Fancy Mumble sessions
        |
        v
[Server] Generates fancy-pchat-key-request
        |   { channel_id, mode, requester_hash, requester_public,
        |     request_id, timestamp, relay_cap }
        |   Broadcasts to ALL online Fancy Mumble members in the channel
        v
[Online Members] Each member receives the key request
        |   Each validates timestamp freshness (reject if too old)
        |   Each adds a small random delay (0-500ms)
        v
[Member B] (responder)
        |   For POST_JOIN: generates new epoch key, sets parent_fingerprint
        |   For FULL_ARCHIVE: uses existing channel key
        |   Encrypts key to Client A's X25519 public key
        |   Signs payload (including own timestamp) with Ed25519 identity key
        v
[Member B] Sends fancy-pchat-key-exchange
        |   { ..., request_id, timestamp, recipient_hash = A, signature,
        |     parent_fingerprint, epoch_fingerprint }
        v
[Server] Validates sender_hash, matches request_id
        |   Relays response to Client A (up to relay_cap total)
        |   (relay_cap is a bandwidth cap, NOT a security parameter)
        v
[Client A] Receives key-exchange(s)
        |   1. Verifies Ed25519 signature (rejects if invalid)
        |   2. Checks timestamp freshness against request_timestamp
        |   3. If sender is key custodian/originator (from ChannelState):
        |      accept as Verified immediately
        |   4. Otherwise: accumulate in 10-second collection window
        |   (POST_JOIN: accepts 1 response, marks fulfilled)
        v
[Client A] For EACH received key-exchange:
        |   1. Verify Ed25519 signature against sender's known public key
        |   2. If signature invalid -> reject, log security event
        |   3. Decrypt epoch/channel key
        |   4. Verify epoch_fingerprint matches SHA-256(decrypted_key)[0..8]
        |
        v   (FULL_ARCHIVE: after 10-second collection window closes)
[Client A] Computes required_threshold = clamp(floor(observed_members / 2), 1, 5)
        |   (observed_members from local ServerState, NOT server-provided)
        |   Compare all decrypted keys:
        |   >= required_threshold distinct peers agree -> VERIFIED
        |   Any mismatch -> DISPUTED, raise alert
        |   Fewer than threshold but all agree -> UNVERIFIED (warning shown)
        |
        v   (POST_JOIN)
[Client A] Verify parent_fingerprint (if not first epoch)
        |   Store key with trust level = UNVERIFIED
        |   Attempt to decrypt recent messages -> if success, promote to VERIFIED
        |
        v
[Client A] Can now decrypt messages from this epoch onwards
```

#### Async Flow (no online members)

```
[Client A] Joins persistent channel, sends key-announce
        |
        v
[Server] Broadcasts fancy-pchat-key-request
        |   ...but no other Fancy Mumble members are online
        v
[Server] Queues the key request in pchat_pending_key_requests
        |   (stores requester_hash, requester_public, channel_id, mode)
        v
...later...
        |
[Member B] Connects to the server
        |
        v
[Server] Detects Member B is in a persistent channel
        |   with pending key requests
        |   Delivers pending requests to Member B
        v
[Member B] Client processes pending requests (respecting batch limit)
        |   Generates epoch key / uses existing channel key
        |   Signs each key-exchange with Ed25519 identity key
        |   Encrypts to each requester's public key
        v
[Member B] Sends fancy-pchat-key-exchange for each pending user
        |
        v
[Server] Routes key-exchange to recipients
        |   If recipient is online: delivers immediately
        |   If recipient is offline: queues for next connect
        |   Removes fulfilled requests from pending queue
```

**Timeout**: Pending key requests that are not fulfilled within a
configurable period (e.g. 7 days) are pruned. The affected user can
re-trigger a request by re-joining or re-announcing their key.

**Client-side batch limit**: To prevent cryptographic exhaustion via
a flood of pending key requests, clients MUST enforce a maximum batch
limit (default 50 key requests per connection). If the server delivers
more pending requests than the batch limit, the client processes only
the first N and ignores the rest. The ignored requesters will have
their requests fulfilled by a subsequent member who connects, or they
can re-announce their key. Additionally, clients SHOULD throttle
key-exchange processing to a maximum rate (e.g. 10 per second) to
bound CPU usage from asymmetric cryptographic operations.

### 7.4 Offloading & Re-fetching

```
[Client] Memory pressure or channel switch
        |
        v
[Client] Offload old messages to local encrypted storage
        |   (existing OffloadStore mechanism)
        v
...later...
        |
[Client] User scrolls up, needs old messages
        |
        v
[Client] Check local OffloadStore first
        |   Found? -> decrypt locally, done
        |   Not found (e.g., app reinstall)?
        v
[Client] Send fancy-pchat-fetch { before_id: oldest_id }
        |
        v
[Server] Returns encrypted messages
        |
        v
[Client] Decrypt with stored keys, insert into view
```

---

## 8. Client Architecture

### 8.1 Rust Traits & Abstractions

The persistent chat system is built on trait abstractions so that
legacy (non-persistent) channels and persistent channels share a
common interface.

```rust
/// Channel message provider - abstracts volatile vs persistent channels.
pub trait MessageProvider: Send + Sync {
    /// Retrieve messages visible to the current user.
    /// Returns messages in chronological order.
    fn get_messages(
        &self,
        channel_id: u32,
        range: MessageRange,
    ) -> Result<Vec<StoredMessage>>;

    /// Store a new outgoing message.
    fn store_message(
        &mut self,
        channel_id: u32,
        message: StoredMessage,
    ) -> Result<()>;

    /// Replace a message identified by `replaces_id` from the same
    /// `sender_hash`. Returns true if a match was found and replaced.
    /// Implementations MUST search their local store for a message
    /// whose `message_id` matches `replaces_id` and whose
    /// `sender_hash` matches `replacement.sender_hash`.
    fn replace_message(
        &mut self,
        channel_id: u32,
        replaces_id: &str,
        replacement: StoredMessage,
    ) -> Result<bool>;

    /// Check if more messages are available beyond what is loaded.
    fn has_more(&self, channel_id: u32) -> bool;

    /// The persistence mode for this channel (NONE for legacy).
    fn mode(&self, channel_id: u32) -> PersistenceMode;
}

/// Range for message queries.
pub enum MessageRange {
    /// Latest N messages.
    Latest(usize),
    /// Messages before a cursor (pagination).
    Before { message_id: String, limit: usize },
    /// Messages after a cursor.
    After { message_id: String, limit: usize },
}

/// A message as stored/retrieved.
pub struct StoredMessage {
    pub message_id: String,
    pub channel_id: u32,
    pub timestamp: u64,
    pub sender_hash: String,
    pub sender_name: String,
    pub body: String,
    pub encrypted: bool,       // true = body is ciphertext, needs decryption
    pub epoch: Option<u32>,    // for POST_JOIN
    pub chain_index: Option<u32>,
}

/// Persistence mode for a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistenceMode {
    None,
    PostJoin,
    FullArchive,
    /// Future: server stores plaintext (or server-encrypted) messages.
    /// No client-side key management. See section 3.1.
    ServerManaged,
}
```

### 8.2 Implementations

```rust
/// Volatile provider - standard Mumble behavior.
/// Messages exist only in memory for the current session.
pub struct VolatileMessageProvider {
    messages: HashMap<u32, Vec<StoredMessage>>,
}

/// Persistent provider - backed by server storage + local cache.
/// Handles encryption/decryption transparently.
pub struct PersistentMessageProvider {
    /// Local cache of decrypted messages.
    cache: HashMap<u32, Vec<StoredMessage>>,
    /// Channel configurations received from server.
    configs: HashMap<u32, ChannelPersistConfig>,
    /// Key manager for encryption/decryption.
    key_manager: KeyManager,
    /// Handle to send fetch requests to server.
    client_handle: ClientHandle,
    /// Local offload store for memory management.
    offload: OffloadStore,
}

/// Composite provider that delegates based on channel config.
pub struct CompositeMessageProvider {
    volatile: VolatileMessageProvider,
    persistent: PersistentMessageProvider,
}

impl MessageProvider for CompositeMessageProvider {
    fn get_messages(&self, channel_id: u32, range: MessageRange) -> Result<Vec<StoredMessage>> {
        match self.mode(channel_id) {
            PersistenceMode::None => self.volatile.get_messages(channel_id, range),
            PersistenceMode::PostJoin | PersistenceMode::FullArchive => {
                self.persistent.get_messages(channel_id, range)
            }
            // Future: delegate to PlaintextMessageProvider (no encryption).
            // PersistenceMode::ServerManaged => self.plaintext.get_messages(channel_id, range),
            _ => self.volatile.get_messages(channel_id, range), // fallback until implemented
        }
    }

    fn replace_message(
        &mut self,
        channel_id: u32,
        replaces_id: &str,
        replacement: StoredMessage,
    ) -> Result<bool> {
        // Search BOTH providers: the original may be a plaintext
        // TextMessage in volatile (from real-time delivery) even
        // though the replacement is an encrypted persistent message.
        if self.volatile.replace_message(channel_id, replaces_id, replacement.clone())? {
            return Ok(true);
        }
        self.persistent.replace_message(channel_id, replaces_id, replacement)
    }

    fn mode(&self, channel_id: u32) -> PersistenceMode {
        self.persistent.configs
            .get(&channel_id)
            .map(|c| c.mode)
            .unwrap_or(PersistenceMode::None)
    }
    // ...
}
```

### 8.3 Key Manager

```rust
/// Trust level for a received key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyTrustLevel {
    /// Multi-confirmed or validated by successful message decryption.
    Verified,
    /// Single source, not yet confirmed.
    Unverified,
    /// Conflicting keys received from different members.
    Disputed,
}

pub struct KeyManager {
    /// The identity seed (32 bytes, from OS CSPRNG or BIP39 recovery).
    /// All key pairs are derived from this seed. Never transmitted.
    identity_seed: [u8; 32],
    /// Our X25519 key pair (DH / key agreement), derived from identity_seed.
    identity_dh: X25519KeyPair,
    /// Our Ed25519 key pair (digital signatures), derived from identity_seed.
    identity_sign: Ed25519KeyPair,
    /// Known public keys of other users (cert_hash -> PeerKeyRecord).
    /// PeerKeyRecord includes X25519/Ed25519 public keys AND the highest
    /// observed announce timestamp for anti-rollback (see section 6.8).
    peer_keys: HashMap<String, PeerKeyRecord>,
    /// Channel epoch keys (channel_id -> epoch -> (key, trust_level)).
    /// Retained for the duration of pchat_retention_days to allow
    /// historical message decryption (see section 5.2).
    epoch_keys: HashMap<u32, BTreeMap<u32, (EpochKey, KeyTrustLevel)>>,
    /// Channel archive keys (channel_id -> (key, trust_level)).
    archive_keys: HashMap<u32, (ChannelKey, KeyTrustLevel)>,
    /// Key requests processed this connection (for batch limiting).
    requests_processed: u32,
    /// Maximum key requests to process per connection.
    max_requests_per_connection: u32, // default 50
    /// Pending consensus state: request_id -> collected responses.
    /// Accumulates key-exchange responses during the 10-second window.
    pending_consensus: HashMap<String, ConsensusCollector>,
    /// Channel key originators: channel_id -> cert_hash of the user
    /// who first generated the channel key (tracked locally).
    channel_originators: HashMap<u32, String>,
    /// Pinned key custodian lists per channel (TOFU, see section 5.7).
    /// channel_id -> (pinned_custodians, pending_update).
    /// pending_update is Some if the server sent a different list
    /// that the user has not yet accepted.
    pinned_custodians: HashMap<u32, CustodianPinState>,
    /// Pending epoch fork candidates (POST_JOIN, see section 5.2).
    /// Key: (channel_id, epoch) -> list of (sender_cert_hash, EpochKey,
    /// parent_fingerprint). When a roster change triggers a new epoch,
    /// candidates accumulate here for 2 seconds. After the grace period,
    /// the client applies the deterministic tie-breaker (lowest cert_hash)
    /// and moves the winning key to epoch_keys.
    pending_epoch_candidates: HashMap<(u32, u32), Vec<EpochCandidate>>,
}

/// Tracks the TOFU state of a channel's key custodian list.
pub struct CustodianPinState {
    /// The currently trusted custodian cert hashes.
    pinned: Vec<String>,
    /// Whether the user has explicitly confirmed this custodian list.
    /// On first join with a populated list, this is false. Trust
    /// shortcuts are only granted when confirmed == true.
    /// Set to true when:
    ///   - User clicks "Confirm" on the first-join prompt, OR
    ///   - User clicks "Accept" after a custodian list change, OR
    ///   - The list was empty on first join (no custodians to inject).
    confirmed: bool,
    /// A pending custodian list update from the server, awaiting
    /// user acceptance. None if no pending change.
    pending_update: Option<Vec<String>>,
}

/// A candidate epoch key received during the fork-resolution window.
pub struct EpochCandidate {
    /// cert_hash of the sender who generated this epoch key.
    sender_hash: String,
    /// The epoch key itself.
    epoch_key: EpochKey,
    /// parent_fingerprint from the key-exchange.
    parent_fingerprint: [u8; 8],
    /// epoch_fingerprint from the key-exchange.
    epoch_fingerprint: [u8; 8],
    /// Timestamp when this candidate was received.
    received_at: Instant,
}

/// Cached peer identity keys with anti-rollback timestamp.
pub struct PeerKeyRecord {
    /// Algorithm version from the key announcement (section 6.8).
    /// 1 = X25519 + Ed25519. Peers reject unknown versions.
    algorithm_version: u8,
    /// Key-agreement public key (X25519 for algorithm_version 1).
    dh_public: X25519PublicKey,
    /// Signature public key (Ed25519 for algorithm_version 1).
    signing_public: Ed25519PublicKey,
    /// Highest observed announce timestamp from this peer.
    /// Used for anti-rollback: announcements with timestamp <=
    /// this value are silently discarded (see section 6.8).
    highest_announce_ts: u64,
}

/// Collects key-exchange responses during the consensus window.
pub struct ConsensusCollector {
    /// When the collection window started (first response received).
    window_start: Instant,
    /// Collected responses: sender_hash -> decrypted key bytes.
    responses: HashMap<String, Vec<u8>>,
    /// Request timestamp from fancy-pchat-key-request (for freshness).
    request_timestamp: u64,
    /// Number of Fancy Mumble v2+ members observed in the channel
    /// at the time the request was received (from local ServerState).
    observed_members: u32,
}

impl KeyManager {
    /// Encrypt a message for the given mode.
    /// Computes epoch_fingerprint = SHA-256(key)[0..8] for inclusion
    /// in the fancy-pchat-msg payload.
    pub fn encrypt(
        &self,
        mode: PersistenceMode,
        channel_id: u32,
        message_id: &str,
        timestamp: u64,
        plaintext: &[u8],
    ) -> Result<EncryptedPayload>;

    /// Decrypt a message.
    pub fn decrypt(
        &self,
        mode: PersistenceMode,
        channel_id: u32,
        message_id: &str,
        timestamp: u64,
        payload: &EncryptedPayload,
    ) -> Result<Vec<u8>>;

    /// Verify the Ed25519 signature on a key-exchange payload.
    /// Returns Err if the sender's Ed25519 public key is unknown
    /// or the signature does not verify.
    pub fn verify_key_exchange_signature(
        &self,
        exchange: &KeyExchangePayload,
    ) -> Result<()>;

    /// Process an incoming key exchange message.
    /// 1. Verifies Ed25519 signature (rejects if invalid).
    /// 2. Checks timestamp freshness against request_timestamp.
    /// 3. Decrypts the key.
    /// 4. Verifies epoch_fingerprint matches SHA-256(decrypted_key)[0..8].
    /// 5. For POST_JOIN: verifies parent_fingerprint chain, stores key.
    /// 6. For FULL_ARCHIVE: checks key custodian/originator shortcut,
    ///    then adds to ConsensusCollector for collection window
    ///    evaluation.
    pub fn receive_key_exchange(&mut self, exchange: KeyExchangePayload) -> Result<()>;

    /// Evaluate consensus after the 10-second collection window closes.
    /// Computes required_threshold from observed_members (local count).
    /// Returns the resulting trust level:
    /// - Verified if >= threshold distinct peers agree.
    /// - Disputed if any responses disagree (triggers resolution, see below).
    /// - Unverified if fewer than threshold but all agree.
    ///
    /// On Disputed: applies the prioritised resolution strategy:
    /// 1. Key custodian trust shortcut (checks ChannelState.pchat_key_custodians).
    /// 2. Inline countersignature shortcut.
    /// 3. Falls back to Disputed state for manual OOB resolution.
    pub fn evaluate_consensus(
        &mut self,
        request_id: &str,
        channel_id: u32,
        server_state: &ServerState,
    ) -> KeyTrustLevel;

    /// Check if a key-exchange sender is a trusted authority.
    /// Returns true if ALL conditions hold:
    ///   1. The sender's cert_hash appears in the channel's
    ///      `pchat_key_custodians` (from ChannelState, cached in
    ///      ServerState) or matches the channel key originator, AND
    ///   2. The sender's cert_hash appears in the client's
    ///      TOFU-pinned custodian list for this channel (from
    ///      pinned_custodians). If the server's list differs from
    ///      the pinned list AND the user has not yet accepted the
    ///      change, this method returns false for any cert_hash
    ///      that is not in the pinned list, AND
    ///   3. The pinned custodian list has been **confirmed** by the
    ///      user (CustodianPinState.confirmed == true). On first
    ///      join with a populated list, confirmed is false until
    ///      the user explicitly acknowledges the custodians.
    /// See section 5.7 (Custodian list TOFU).
    pub fn is_trusted_authority(
        &self,
        sender_hash: &str,
        channel_id: u32,
        server_state: &ServerState,
    ) -> bool;

    /// Compare a newly received key against previously received keys
    /// for the same request_id (FULL_ARCHIVE client-side consensus).
    /// Called internally by evaluate_consensus.
    pub fn confirm_key(
        &mut self,
        channel_id: u32,
        mode: PersistenceMode,
        epoch: u32,
        decrypted_key: &[u8],
    ) -> KeyTrustLevel;

    /// Attempt to verify a key by decrypting recent messages.
    /// Used as a supplementary heuristic only (not a primary trust
    /// mechanism). If decryption succeeds for messages from 2+ distinct
    /// senders, logs the result but does NOT promote trust level.
    /// Trust promotion is handled exclusively by consensus, key
    /// custodian shortcut, countersignature, or manual OOB verification.
    pub fn check_key_by_decryption(
        &self,
        channel_id: u32,
        mode: PersistenceMode,
        messages: &[StoredMessage],
    ) -> bool;

    /// Verify an epoch countersignature (standalone or inline).
    /// Checks Ed25519 signature over countersig_data (including
    /// distributor_hash), validates timestamp freshness (5-minute
    /// window), confirms the signer is a known key custodian
    /// (see section 5.7), and verifies distributor_hash matches
    /// the expected distributor. On success, promotes the epoch
    /// key to Verified.
    pub fn verify_countersignature(
        &mut self,
        channel_id: u32,
        epoch: u32,
        epoch_fingerprint: &[u8],
        parent_fingerprint: &[u8],
        signer_hash: &str,
        distributor_hash: &str,
        timestamp: u64,
        countersignature: &[u8],
        server_state: &ServerState,
    ) -> Result<KeyTrustLevel>;

    /// Resolve a Disputed state by selecting a specific peer's key.
    /// Called after manual OOB verification. Discards all other
    /// conflicting keys and promotes the selected key to Manually
    /// Verified.
    pub fn resolve_dispute(
        &mut self,
        channel_id: u32,
        mode: PersistenceMode,
        trusted_sender_hash: &str,
    ) -> Result<()>;

    /// Generate key exchange messages for a new member.
    /// Called when this client responds to a fancy-pchat-key-request.
    /// Signs the payload with our Ed25519 identity key.
    /// Any online member holding the current key can call this -
    /// there is no designated key distributor.
    pub fn distribute_key(
        &self,
        channel_id: u32,
        mode: PersistenceMode,
        recipient_hash: &str,
        recipient_public: &X25519PublicKey,
    ) -> Result<KeyExchangePayload>;

    /// Handle an incoming fancy-pchat-key-request from the server.
    /// Enforces the per-connection batch limit (max_requests_per_connection).
    /// Returns None if the batch limit is reached or this client does
    /// not hold the key for the requested channel.
    pub fn handle_key_request(
        &mut self,
        request: KeyRequestPayload,
    ) -> Result<Option<KeyExchangePayload>>;

    /// Get the trust level for a channel's current key.
    pub fn trust_level(
        &self,
        channel_id: u32,
        mode: PersistenceMode,
    ) -> Option<KeyTrustLevel>;
}
```

### 8.4 Frontend Integration

The React frontend gains awareness of persistence through the existing
store pattern:

```typescript
// New store fields
interface AppState {
  // Existing fields...

  // Persistence metadata per channel
  channelPersistence: Record<number, {
    mode: "NONE" | "POST_JOIN" | "FULL_ARCHIVE";
    maxHistory: number;
    retentionDays: number;
    hasMore: boolean;        // server has more messages to fetch
    isFetching: boolean;     // currently loading history
  }>;

  // Actions
  fetchHistory: (channelId: number, beforeId?: string) => Promise<void>;
  getPersistenceMode: (channelId: number) => PersistenceMode;
}
```

The `ChatView` component renders a persistence info banner at the top
of the channel when `mode != "NONE"`:

```
+-------------------------------------------------------+
| Messages in this channel are stored encrypted on the   |
| server. You can see messages from [mode description].  |
| Retention: 90 days | Stored: 1,247 messages            |
+-------------------------------------------------------+
|                    (message history)                   |
```

The banner text varies by mode:
- **POST_JOIN**: "Messages are visible from the moment you first joined
  this channel."
- **FULL_ARCHIVE**: "All stored messages are visible to channel members."

A "Load more" trigger (intersection observer on scroll-to-top) sends
`fancy-pchat-fetch` requests for pagination.

### 8.5 Module Layout

```
crates/mumble-protocol/src/
  persistent/
    mod.rs                    # PersistenceMode, StoredMessage, MessageRange
    provider.rs               # MessageProvider trait + VolatileMessageProvider
    encryption.rs             # XChaCha20-Poly1305 encrypt/decrypt, envelope format
    keys.rs                   # KeyManager, X25519 key pair management
    wire.rs                   # MessagePack serialization for all fancy-pchat-* payloads
    config.rs                 # ChannelPersistConfig, mode parsing

crates/mumble-tauri/src/
  state/
    persistent/
      mod.rs                  # PersistentMessageProvider, CompositeMessageProvider
      handler.rs              # Handle incoming fancy-pchat-* PluginData messages
      fetcher.rs              # Outbound fetch request logic
      key_store.rs            # Persistent key storage (filesystem)

crates/mumble-tauri/ui/src/
  components/
    PersistenceBanner.tsx      # Channel info banner showing persistence mode
  store.ts                     # Extended with channelPersistence state
  types.ts                     # PersistenceMode type, ChannelPersistConfig
```

---

## 9. Backwards Compatibility

### 9.1 Legacy Server (No Companion)

| Behavior | Effect |
|----------|--------|
| `PluginDataTransmission` | Forwarded to listed sessions, then discarded. No persistence. |
| `ChannelState` extension fields | `pchat_mode` etc. are absent. Client sees all channels as NONE. |
| `TextMessage` | Works exactly as before. |
| **Result** | Client operates in pure volatile mode. No degradation. |

### 9.2 Legacy Client (No Fancy Mumble)

| Behavior | Effect |
|----------|--------|
| `PluginDataTransmission` | Silently ignored by legacy client. |
| `ChannelState` extension fields | Unknown fields silently ignored by protobuf deserialization. |
| `TextMessage` | Received and displayed normally (plaintext body). |
| **Result** | Legacy users see real-time messages but have no history access. |

### 9.3 Mixed Environment

When both Fancy Mumble and legacy clients are in the same channel:

- Fancy clients send **both** a `TextMessage` (plaintext, for live
  delivery) and a `fancy-pchat-msg` (encrypted, for storage).
- Legacy clients see the `TextMessage` in real time.
- On reconnect, only Fancy clients can fetch and decrypt history.
- The server companion stores encrypted messages regardless of who
  sent them, but only Fancy-originated messages include the encrypted
  envelope.
- Legacy clients do not need the persistence config; the protobuf
  extension fields are invisible to them.

### 9.4 Feature Detection

```
[Client] Connects, sends Version { fancy_version: Some(2) }
        |      (v2 = supports persistent chat)
        v
[Client] Receives ChannelState for all channels
        |      (standard Mumble handshake)
        v
[Client] Reads pchat_mode / pchat_max_history / pchat_retention_days
        |      from each ChannelState to discover persistent channels
        v
[Server Companion] Sees fancy_version >= 2
        v
[Server Companion] Begins accepting fancy-pchat-* messages
```

**Admin configuration** uses the same `ChannelState` mechanism as
editing a channel name or description: the admin sends a `ChannelState`
with the desired `pchat_*` fields set. Murmur persists them in its
database and re-broadcasts to all clients.

- `fancy_version: 1` = original Fancy Mumble (message_id, timestamp,
  profiles, polls)
- `fancy_version: 2` = adds persistent chat support

If no `ChannelState` contains `pchat_mode` fields, the
client treats all channels as volatile (no persistence). This is
the default for any legacy server or any server where the admin has
not configured persistence.

---

## 10. Security Considerations

### Threat Model

| Threat | Mitigation |
|--------|------------|
| Server reads message content | E2E encryption - server only sees ciphertext |
| Server tampers with messages | AEAD authentication + AAD binding (channel_id, message_id, timestamp) |
| Server replays old messages | Client tracks seen message_ids (dedup set) |
| Server drops messages | Acknowledged via `fancy-pchat-ack`; client can detect gaps via chain_index (POST_JOIN) |
| Server delivers messages to unauthorized users | Client-enforced: key rotation on roster changes ensures old ciphertext is undecryptable by removed members (see section 3, "Server-Side vs Client-Side Access Control") |
| Compromised user reads future messages | Forward secrecy: POST_JOIN re-keys on epoch change (triggered by any roster change, not just joins) |
| Compromised user reads past messages | POST_JOIN: epoch-level forward secrecy. Intermediate chain keys are deleted after ratcheting, but epoch keys are retained for `pchat_retention_days` to support historical fetching. True per-message forward secrecy within an epoch is not achievable with persistent history (documented trade-off, see section 5.2). Epoch transitions (roster changes) are the primary forward-secrecy boundary. FULL_ARCHIVE: explicitly no forward secrecy (documented trade-off) |
| **Server modifies custodian list** | **Custodian list TOFU (section 5.7)**: client pins the custodian list on first observation and warns on subsequent changes. New custodians are not granted consensus-bypass privileges until the user manually accepts the change. On first join, a populated custodian list requires explicit user confirmation before trust shortcuts are enabled (prevents first-join injection by a compromised server). |
| **Identity rollback via stale key-announce** | **Anti-rollback timestamp tracking (section 6.8)**: clients track the highest observed announce timestamp per peer `cert_hash`. Announcements with timestamp <= the known value are silently discarded. Server SHOULD also enforce monotonicity. |
| **Epoch fork (POST_JOIN race condition)** | **Deterministic tie-breaker (section 5.2)**: when multiple responders simultaneously generate competing epoch keys chained to the same parent, all clients converge on the epoch from the sender with the lexicographically smallest `cert_hash`. Losing forks are discarded. Epoch broadcast (`request_id: null`) propagates the new epoch to all existing members. 2-second outbound buffer after roster changes minimises fork-window messages. |
| **Rogue key distribution** | **Ed25519 signature on every key-exchange (section 6.6); client-enforced multi-confirmation consensus with collection window for FULL_ARCHIVE (section 5.3); parent_fingerprint epoch chain for POST_JOIN (section 5.2); key custodian trust shortcut (section 5.7); creator countersignature on epoch transitions (section 5.6.4); TOFU with visual fingerprint verification (section 5.6); trust levels (section 5.4)** |
| **Server suppresses key-exchange responses** | **Client-enforced consensus**: the client computes its own threshold from locally observed members (not server-provided). If fewer responses arrive than expected, key is marked Unverified (visible to user). Server cannot forge responses (Ed25519 signatures). |
| **Server forwards colluding responses** | **Client-enforced consensus**: the client requires responses from distinct peers it independently recognises (via `ServerState` + `fancy-pchat-key-announce`). Colluding members must be legitimately registered members the client already sees -- reduces attack surface to insider threats. Disputed resolution (section 5.4) applies key custodian trust shortcut and OOB verification fallback rather than hard-locking the channel. |
| Man-in-the-middle key exchange | Identity keys bound to TLS certificate hash via dual-signature key-announce (section 6.8); Ed25519 signatures on key-exchange verified client-side; out-of-band verification via key fingerprints |
| Key-announce spoofing (garbage keys) | Server MUST verify TLS signature and cert_hash match on `fancy-pchat-key-announce`; peers verify Ed25519 signature (section 6.8) |
| Key-exchange impersonation | Server MUST validate `sender_hash == session.cert_hash`; client MUST verify Ed25519 signature independently of server (section 6.6) |
| Key request flooding (DoS) | Server enforces per-user (5) pending request limits with FIFO eviction at per-channel soft cap (100) targeting the heaviest requester; client enforces per-connection batch limit (50) and processing throttle (10/sec) |
| Client-dictated timestamp manipulation | Server enforces timestamp validation: override with server clock or reject if `abs(client_ts - server_ts) > 5000ms` |
| Nonce reuse | XChaCha20's 192-bit nonce with random generation - collision probability negligible |
| Message ID collision/preimage | `message_id` uniqueness bound to `(channel_id, sender_hash, message_id)` - no cross-sender collision possible |
| Epoch fork message state corruption | During fork resolution re-sends, reusing the original `message_id` is rejected by the server's uniqueness constraint; generating a new `message_id` without linkage causes ghost duplicates. Mitigated by `replaces_id` field (section 6.2): re-sent messages carry a new `message_id` with `replaces_id` pointing to the original, allowing clients to deduplicate and the server to mark the original as superseded (section 5.2 step 3) |
| Orphaned replacement via dual-path race | During an epoch fork, a client may receive the plaintext `TextMessage` (real-time) but go offline before the encrypted `PluginDataTransmission` arrives. On reconnect it fetches the replacement (with `replaces_id`) but never stored the encrypted original. Without cross-provider search, the volatile plaintext copy and the persistent replacement coexist as ghost duplicates. Mitigated by `replace_message` searching **both** the volatile in-memory store and the persistent encrypted store (section 6.2, section 8.1/8.2). |
| Identity key loss / device loss | Identity seed backed up via BIP39 mnemonic (24 words); deterministic recovery of both X25519 and Ed25519 key pairs from mnemonic (section 5.1) |
| TLS certificate rotation | E2EE identity is independent of TLS certificate; re-announce with new cert's TLS signature preserves E2EE identity and peer trust (section 6.8) |
| Countersig replay across distributors | `distributor_hash` in countersig signed data binds the countersignature to a specific key distributor; prevents server from transplanting countersigs between key-exchange payloads (section 5.6.4) |
| Algorithm ossification / key misparse | `algorithm_version` field in `fancy-pchat-key-announce` (section 6.8) and `fancy-pchat-key-exchange` (section 6.6) identifies the key-agreement and signature algorithms. Clients reject unknown versions. The field is included in both dual signatures (key-announce) and the key-exchange signature, preventing downgrade by an intermediary. Recipients reject key-exchanges whose `algorithm_version` does not match the sender's announced version. Future algorithm migrations (e.g. post-quantum) increment the version without breaking legacy parsing. |
| Deterministic ciphertext padding leaks message length | Randomized block-aligned padding (section 10.1): plaintext is padded to the next multiple of 256 bytes plus a random 0-255 byte jitter from a CSPRNG. This blurs size-class boundaries and prevents an observer from mapping ciphertext length back to exact plaintext length. Deterministic schemes (e.g. `next_power_of_two`) are explicitly forbidden. |

### Rogue Key Distribution Attack

**Attack scenario**: In the decentralized key distribution model, a
malicious or compromised online member can respond to a
`fancy-pchat-key-request` before legitimate members, distributing a
fake epoch key. The new user would accept this fake key, creating a
split-brain scenario where the malicious user can send messages only
the new user can read, while the new user is locked out of messages
from legitimate members.

**Mitigations** (defense in depth):

1. **Mandatory Ed25519 signatures** (section 6.6): Every key-exchange
   payload MUST be signed with the sender's Ed25519 identity key. The
   recipient verifies independently of the server. While this does
   not prevent a legitimately registered malicious member from signing
   a fake key, it provides authentication (you know WHO sent the key),
   non-repudiation (the sender cannot deny distributing a specific
   key), and prevents the server from injecting forged key material.

2. **Client-enforced multi-confirmation consensus** (FULL_ARCHIVE,
   section 5.3): The receiving client computes its own
   `required_threshold` from channel members it directly observes
   in `ServerState`. It opens a 10-second collection window and
   accumulates all key-exchange responses. The server's `relay_cap`
   is a bandwidth optimization; the security threshold is entirely
   client-side. A compromised server cannot subvert this because:
   - It cannot forge responses (Ed25519 signatures from known peers).
   - It can only suppress responses, leading to Unverified (visible
     to the user), not Verified with a fake key.
   - Forwarding responses from colluding malicious members requires
     those members to be legitimately registered and visible to the
     client in the channel.

3. **Parent fingerprint chain** (POST_JOIN, section 5.2): Each new
   epoch key carries `parent_fingerprint` (truncated hash of the
   previous epoch key). Existing members who receive the new epoch
   verify the chain. If a malicious member forges a new epoch, the
   `parent_fingerprint` will not match the real previous epoch,
   causing legitimate members to reject it and raise a security alert.

4. **Epoch fingerprint cross-check** (section 6.2): Every encrypted
   message includes `epoch_fingerprint` (truncated hash of the
   epoch key used). Recipients compare against their locally held key.
   A mismatch indicates the recipient holds the wrong key.

5. **TOFU with out-of-band verification** (section 5.6): When
   consensus is insufficient (single responder or first-ever join),
   the key is accepted under TOFU. The UI shows a clear "Unverified"
   indicator and prompts the user to verify via visual fingerprints.
   This ensures the user is aware of the reduced guarantee.

6. **Trust level indicators** (section 5.4): The UI displays the key's
   trust level (Manually Verified / Verified / Unverified / Disputed),
   allowing users to make informed decisions about channel security.
   Disputed state triggers the prioritised resolution strategy
   (section 5.4, Disputed Resolution) rather than a hard read-only
   lock.

### Server-Side Access Control is an Optimization

Server-side filtering (POST_JOIN date checks) is a bandwidth
optimization, NOT a security boundary. The cryptographic key model is
the sole enforcer of who can read what. If the server delivered every
stored message to every client, confidentiality would still hold
because clients without the correct epoch key cannot decrypt the
ciphertext.

Clients MUST:

1. Rotate channel keys on **any** roster change (join or leave).
2. Delete intermediate chain keys after ratcheting forward. Retain
   epoch keys locally for `pchat_retention_days` to allow historical
   re-derivation (see section 5.2, item 5).
3. Never distribute old epoch keys to new members (POST_JOIN).
4. Purge epoch keys whose retention period has expired.
5. Track the highest observed announce timestamp per peer and
   silently discard key-announce messages with stale timestamps
   (see section 6.8).

### Key Verification

Key verification follows a **TOFU + optional out-of-band** model
inspired by Signal, Matrix, and WhatsApp (see section 5.6 for full
UX details):

- **Simple mode (default)**: Trust On First Use. Keys are accepted
  immediately with a yellow "Unverified" indicator. Users can ignore
  the indicator for casual use (still protected against passive
  surveillance) or click through to verify.
- **Expert mode**: Visual fingerprints (emoji sequence, word list, or
  hex) for out-of-band comparison. Users compare fingerprints via
  voice chat, in-person, or another secure channel and promote to
  "Manually Verified".
- **Creator countersignature** (POST_JOIN): Key custodians can
  countersign epoch transitions via `fancy-pchat-epoch-countersig`.
  New users who cannot verify `parent_fingerprint` (no previous
  epoch) can instead verify the custodian's countersignature.

Fingerprint computation:
```
full_fingerprint = SHA-256(channel_key || channel_id (4B BE) || mode (1B))
```
The first 8 bytes are displayed as the "short fingerprint" (8 emoji
or 8 words). The full 32 bytes are available as the "full
fingerprint".

Key change detection: if a channel key changes unexpectedly (no
valid `parent_fingerprint` chain), trust resets to Unverified and
a prominent warning is shown (analogous to Signal's "safety number
has changed").

### Timestamp Integrity

Client-provided timestamps cannot be trusted. To prevent timestamp
manipulation (backdating or future-dating messages for ordering
attacks), the server companion MUST either:

1. **Override**: Replace the client-provided timestamp with the
   server's clock at ingestion time (simpler, recommended), or
2. **Enforce delta**: Reject messages where
   `abs(client_timestamp - server_time) > 5000ms`.

The server's `created_at` column provides a trustworthy ordering
baseline regardless of which approach is chosen. Clients SHOULD prefer
server-side timestamps for display ordering when available.

### Message ID Integrity

The `message_id` uniqueness constraint MUST be scoped to
`(channel_id, sender_hash, message_id)` rather than just `message_id`
globally. This prevents a malicious user from preemptively claiming
another user's message IDs.

Alternatively, clients MAY derive message IDs deterministically from
the ciphertext hash: `message_id = SHA-256(envelope_bytes)[0..16]`.
This makes the ID verifiable by the server and resistant to collisions,
at the cost of requiring the full payload before generating the ID.

### Identity Verification at the Server

Two server-side checks are critical for preventing impersonation:

1. **Key announcements** (`fancy-pchat-key-announce`): The server MUST
   verify the `tls_signature` over
   `cert_hash || timestamp || identity_public || signing_public`
   using the TLS certificate public key of the sending session, AND
   confirm that `cert_hash` matches the session's actual TLS
   certificate hash. The Ed25519 `signature` is verified by peers,
   not the server (since the server does not yet know the Ed25519
   public key at announce time). Without TLS signature verification,
   a malicious client can register garbage keys under another user's
   cert_hash, causing DoS (encryption failures for the victim).

2. **Key exchange relay** (`fancy-pchat-key-exchange`): The server MUST
   hard-validate that `sender_hash == current_session.cert_hash`. The
   **client** MUST additionally verify the Ed25519 signature on the
   key-exchange payload using the sender's known Ed25519 public key
   (received via `fancy-pchat-key-announce`). This two-layer check
   (server validates identity, client verifies signature) ensures
   neither a compromised server nor a malicious member can inject
   forged key material undetected.

### Rate Limiting and Abuse Prevention

Rate limiting MUST extend beyond fetch requests:

- **Key announcements**: Strict per-IP and per-session rate limiting on
  `fancy-pchat-key-announce` (e.g. 5 per minute per session). Rapid key
  rotation is a potential DoS vector that forces all peers to re-encrypt.
- **Message storage**: Per-session rate limiting on `fancy-pchat-msg`
  (e.g. 30 per minute).
- **Key requests**: The server MUST enforce strict bounds:
  - Max pending key requests per requester identity: 5 (default).
    Exceeding the per-user limit results in hard rejection with
    `"key_request_limit_exceeded"`.
  - Soft max pending key requests per channel: 100 (default). When
    reached, the server applies FIFO eviction (evicts the oldest
    request from the identity with the most pending slots) rather
    than rejecting the new request. This prevents a single actor
    from monopolising the queue and locking out legitimate users.
- **Client-side key request batch limit**: Clients MUST enforce a
  maximum number of key requests processed per connection (default 50).
  This prevents cryptographic exhaustion via a flood of queued requests
  delivered on connect. Once the limit is reached, additional requests
  are silently ignored. Clients SHOULD also throttle processing rate
  to a maximum of 10 key requests per second.
- **Registration requirement**: Servers SHOULD require Mumble user
  registration (registered accounts with server-stored certificates)
  before allowing persistent chat interactions. Unregistered/guest users
  can still use volatile chat but cannot store persistent messages or
  announce identity keys. This raises the cost of abuse significantly.

### Metadata Privacy

The server companion necessarily sees:

- Who sends messages (by session/cert hash)
- When messages are sent (timestamps)
- Which channel they target
- Message sizes (ciphertext length reveals approximate plaintext length;
  randomized block-aligned padding mitigates but does not eliminate this)

This is inherent to any server-stored E2E system. The message _content_
and sender _display name_ (inside the encrypted envelope) remain
confidential.

#### Ciphertext Padding

To mitigate message-length analysis, clients MUST pad plaintext
messages before encryption using a **randomized block-aligned** scheme.
This prevents an observer from distinguishing "yes" from "absolutely
not, that is unacceptable" based on ciphertext length alone.

**Why not power-of-two buckets?** A deterministic scheme such as
`next_power_of_two(plaintext_length + 1)` creates sharp "cliff"
transitions between size classes (e.g. 64 to 128 to 256 bytes).
Because `MessageEnvelope` overhead is fixed and known, an attacker
can reverse the bucket mapping and narrow down the plaintext length --
especially for very short messages ("Yes", "No", "Ok") that always
land in the smallest bucket with a predictable amount of padding.

**Randomized block-aligned padding** -- the required scheme:

```
BLOCK = 256                                        // bytes
blocks_needed  = ceil((plaintext_length + 2) / BLOCK)
jitter         = random_uniform(0, BLOCK - 1)      // CSPRNG
padded_length  = blocks_needed * BLOCK + jitter
pad_count      = padded_length - plaintext_length   // >= 2
padding        = [0x00] * (pad_count - 2) + big_endian_u16(pad_count)
```

1. The plaintext is rounded up to the next multiple of `BLOCK` (256)
   bytes, guaranteeing a minimum pad of 2 bytes.
2. A **random jitter** of 0 to 255 bytes (drawn from a CSPRNG) is
   added on top. This blurs the exact bucket boundary so two messages
   of identical length produce different ciphertext sizes across sends.
3. The last two bytes store `pad_count` as a big-endian `u16`,
   allowing the receiver to strip padding unambiguously.
4. Maximum overhead per message: `BLOCK - 1` (alignment) + `BLOCK - 1`
   (jitter) = 510 bytes -- negligible for a chat system.

Padding is appended to the serialized `MessageEnvelope` byte array
before encryption, and stripped after decryption (see section 4
Encryption Envelope and section 6.3 for the exact byte layout).

Clients MUST NOT use a deterministic padding function (e.g.
`next_power_of_two`) because it leaks exploitable size-class
information to any observer with access to the ciphertext lengths.

### Audit Recommendations

Before production deployment:

1. Formal review of the XChaCha20-Poly1305 + HKDF key derivation chain.
2. Fuzzing of MessagePack deserialization (both client and server-side
   parsers).
3. Verify that the `ring` / `chacha20poly1305` / `ed25519-dalek` crate
   versions have no known vulnerabilities.
4. Pen-test the key exchange flow for replay, reflection, and
   impersonation attacks - specifically test the rogue key distribution
   scenario (malicious first-responder sending fake epoch keys).
5. Review server-side validation logic for all identity checks
   (cert_hash binding, signature verification, sender_hash validation).
6. Verify Ed25519 signature verification on key-exchange is enforced
   client-side and cannot be bypassed.
7. Test multi-confirmation consensus under adversarial conditions
   (one of N responders sends a different key).
8. Verify client-side batch limits prevent cryptographic exhaustion
   under key-request flooding.
9. Audit metadata storage for privacy compliance with applicable
   regulations (GDPR, etc.).
10. Verify TOFU state transitions: key acceptance, trust promotion
    (Unverified to Manually Verified via OOB), and trust reset on
    unexpected key change.
11. Test visual fingerprint determinism: emoji sequence, word list,
    and hex output must be identical across all supported platforms
    for the same input key material.
12. Verify creator countersignature (`fancy-pchat-epoch-countersig`)
    validation: Ed25519 signature over `channel_id || epoch ||
    epoch_fingerprint || parent_fingerprint || timestamp ||
    distributor_hash` is correctly checked, timestamp freshness is
    enforced (5-minute window), `distributor_hash` matches the
    expected key distributor, and that only key custodian signatures
    are accepted. Also verify inline countersignatures in
    `fancy-pchat-key-exchange` are validated identically to standalone
    countersig messages AND that `distributor_hash` matches the
    key-exchange `sender_hash`.
13. Test key change warning UX: ensure the "safety number changed"
    analogue is shown prominently and cannot be silently dismissed.
14. Verify Disputed resolution: key custodian trust shortcut correctly
    selects the custodian's key and discards attackers' keys; manual
    peer selection via OOB verification works correctly; auto-
    resolution timeout (24h) triggers only when a valid
    parent_fingerprint chain exists from the majority key AND a
    valid key custodian countersignature is present.
15. Verify `pchat_key_custodians` field handling: test that custodian
    list changes via `ChannelState` are processed correctly, that
    trust shortcuts use only the locally cached list, and that a
    tampered custodian list cannot produce valid countersignatures
    without the corresponding Ed25519 private key.
16. Verify identity seed generation uses OS CSPRNG (not derived from
    TLS certificate). Test BIP39 mnemonic round-trip: encoding a seed
    to 24 words and decoding back produces the identical seed. Test
    that X25519 and Ed25519 key pairs derived from a recovered seed
    match the originals.
17. Verify dual-signature key-announce: both Ed25519 and TLS
    signatures are present and valid; server rejects announcements
    with invalid TLS signature or mismatched cert_hash; peers verify
    Ed25519 signature independently.
18. Test FIFO eviction on key request queue: simulate an attacker
    filling the per-channel queue (100 slots) from multiple sessions,
    then verify a legitimate user's request evicts the attacker's
    oldest request (from the heaviest requester) rather than being
    rejected.
19. Test custodian list TOFU: verify the client pins the custodian
    list on first observation, detects server-side changes, shows a
    "Channel Authority Changed" warning, and does NOT grant trust
    shortcuts to new custodians until the user explicitly accepts the
    change. Test empty-to-populated transition and persistence across
    restarts. **First-join injection**: verify that a new user joining
    a channel with a populated custodian list does NOT get trust
    shortcuts until explicit confirmation; keys from unconfirmed
    custodians must be treated as ordinary peer responses (Unverified
    trust level, subject to normal consensus).
20. Test key retention and epoch key lifecycle: verify that epoch keys
    are retained for `pchat_retention_days`, that intermediate chain
    keys are deleted after ratcheting, that historical messages can be
    decrypted by re-deriving chains from the retained epoch key, and
    that epoch keys are purged after their retention period.
21. Test anti-rollback for key-announce: simulate replaying a stale
    `fancy-pchat-key-announce` with a timestamp <= the known value
    for a peer; verify the client silently discards it. Also verify
    the server rejects stale announcements. Test first-time
    announcements (no prior record) are accepted normally.
22. Test epoch fork resolution (POST_JOIN): simulate two members
    responding simultaneously to a `fancy-pchat-key-request`, each
    generating a different epoch key. Verify all clients (including
    the new member and all existing members) converge on the epoch
    from the sender with the lexicographically smallest `cert_hash`.
    Verify that messages encrypted with the losing epoch key are
    logged as "stale epoch" warnings. Test the 2-second outbound
    buffer after roster changes. Test that epoch broadcast
    (`request_id: null`) messages are forwarded by the server without
    relay tracking.
23. Test `replaces_id` deduplication: after epoch fork resolution,
    verify that re-sent messages carry a new `message_id` with
    `replaces_id` referencing the original. Verify receiving clients
    replace the stale-epoch copy in their local store. Verify the
    server marks the original message's `superseded_by` column.
    Verify `sender_hash` binding (only the original sender can set
    `replaces_id` for their own messages). Verify the client rejects
    `replaces_id` unless the replacement targets a different
    `epoch`/`epoch_fingerprint`. Verify superseded messages are
    excluded from server fetch responses.
24. Test randomized ciphertext padding: verify that the same plaintext
    produces different ciphertext lengths across multiple encryptions.
    Verify that padding is always at least 2 bytes and the `pad_count`
    trailer is correctly encoded as big-endian u16. Verify that
    jitter is drawn from a CSPRNG. Verify that a deterministic
    padding function (e.g. `next_power_of_two`) is never used.
25. Test `algorithm_version` in key announcements and key exchanges:
    verify that `algorithm_version` is included in both dual-signature
    signed data (key-announce) and in the key-exchange `signed_data`.
    Verify that clients reject announcements and key-exchanges with
    an unknown `algorithm_version` and display a user-visible upgrade
    warning. Verify the server rejects announcements with unsupported
    versions. Verify that `algorithm_version` is stored in
    `pchat_user_keys` and broadcast to peers. Verify that recipients
    reject key-exchanges whose `algorithm_version` does not match the
    sender's announced version. Verify that an intermediary cannot
    strip or modify `algorithm_version` without invalidating
    signatures.
26. Test cross-provider `replaces_id` deduplication: simulate the
    dual-path race condition where a client receives a plaintext
    `TextMessage` (stored in `VolatileMessageProvider`) but goes
    offline before the encrypted `PluginDataTransmission` arrives.
    On reconnect, verify that fetching a replacement message with
    `replaces_id` correctly finds and overwrites the volatile
    plaintext copy. Verify `CompositeMessageProvider.replace_message`
    searches both providers. Verify no ghost duplicates remain in
    the UI after replacement.
