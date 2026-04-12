# Copilot Instructions - Fancy Mumble

> Context document for GitHub Copilot.  Kept up-to-date so the assistant
> can skip expensive context-gathering on every request.

## Project overview

**Fancy Mumble** is a modern desktop Mumble (VoIP) client with profile
customisation features (avatar frames, banners, nameplates, effects).
Licensed MIT.  Written in Rust + TypeScript/React.

| Layer | Crate / package | Tech |
|-------|-----------------|------|
| Protocol library | `crates/mumble-protocol` | Rust, tokio, prost (protobuf), rustls, optional Opus codec |
| Tauri backend | `crates/mumble-tauri` | Rust, Tauri 2, cpal (audio I/O, desktop only), rcgen (self-signed certs) |
| Tauri frontend | `crates/mumble-tauri/ui` | React 19, Vite 6, Zustand 5, react-router-dom 7, TypeScript 5 |
| Tauri Android | `crates/mumble-tauri/gen/android` | Gradle/Kotlin, Android API 34, NDK 27 |
| Dioxus GUI (alt) | `crates/mumble-gui` | Rust, Dioxus 0.7 desktop - older/parallel UI, same protocol lib |

## Workspace layout

```
Cargo.toml                          # Rust workspace root (resolver = "2")
ANDROID_DEV.md                      # Android development setup guide
scripts/
  android-dev.ps1                   # Android prerequisites checker & dev launcher (Windows)
crates/
  mumble-protocol/                  # Pure async Mumble client library
    proto/Mumble.proto              # TCP protobuf definitions
    proto/MumbleUDP.proto           # UDP protobuf definitions
    build.rs                        # prost-build code-gen
    src/
      lib.rs                        # re-exports: audio, client, command, error, event, message, proto, state, transport, work_queue
      client.rs                     # Async event-loop orchestrator (ClientConfig, ClientHandle, run())
      state.rs                      # ServerState: tracked users (User), channels (Channel), ConnectionInfo
      event.rs                      # EventHandler trait (on_control_message, on_udp_message, on_connected, on_disconnected)
      error.rs                      # Error enum (thiserror), Result alias
      message.rs                    # ControlMessage / UdpMessage enums, TcpMessageType id mapping
      work_queue.rs                 # Priority work queue: UDP > TCP > user commands
      command/
        mod.rs                      # CommandAction trait, CommandOutput, BoxedCommand
        authenticate.rs  join_channel.rs  send_audio.rs  send_text_message.rs
        set_comment.rs   set_texture.rs   set_self_mute.rs  set_self_deaf.rs
        set_voice_target.rs  channel_listen.rs  disconnect.rs  ban_user.rs
        kick_user.rs  request_blob.rs  request_ban_list.rs  request_user_stats.rs
        send_plugin_data.rs
      transport/
        tcp.rs                      # TcpTransport (TLS via tokio-rustls), TcpConfig
        udp.rs                      # UdpTransport, PlaintextCryptState, UdpConfig
        codec.rs                    # Wire framing helpers
        audio_codec.rs              # Opus-in-UDP framing
      audio/
        mod.rs                      # Pipeline architecture overview
        sample.rs                   # AudioFrame, AudioFormat
        capture.rs                  # AudioCapture trait + SilentCapture
        playback.rs                 # AudioPlayback trait + NullPlayback
        encoder.rs                  # AudioEncoder trait + OpusEncoder
        decoder.rs                  # AudioDecoder trait + OpusDecoder
        pipeline.rs                 # OutboundPipeline / InboundPipeline
        filter/                     # AudioFilter trait, FilterChain, NoiseGate, AutomaticGainControl
    tests/
      integration.rs               # End-to-end tests against a real murmur (via docker-compose)
      audio_quality.rs              # Audio pipeline quality tests

  mumble-tauri/                     # Tauri 2 desktop app
    tauri.conf.json                 # Product "Fancy Mumble", id com.fancymumble.app, 1024×768 frameless
    Cargo.toml                      # depends on mumble-protocol (with opus-codec), tauri 2, cpal, rcgen
    src/
      main.rs                       # Tauri entry point (calls lib::run)
      lib.rs                        # All #[tauri::command] handlers, app bootstrap
      audio.rs                      # CpalCapture / CpalPlayback - OS audio via cpal
      state/
        mod.rs                      # AppState struct, SharedState, query/messaging/channel/profile methods
        types.rs                    # UI value types (ChannelEntry, UserEntry, ChatMessage, etc.), event payloads, config structs
        connection.rs               # connect() / disconnect() lifecycle
        audio.rs                    # Voice pipeline management (enable, mute, deafen, outbound audio loop)
        event_handler.rs            # TauriEventHandler - EventHandler impl bridging protocol events to Tauri
    gen/
      android/                      # Generated Tauri Android project (Gradle/Kotlin, API 34)
    ui/                             # React frontend
      package.json                  # npm deps: @tauri-apps/api 2, react 19, zustand 5, react-router-dom 7
      vite.config.ts                # Vite 6, target esnext, Tauri env prefix
      index.html
      src/
        main.tsx                    # ReactDOM.createRoot, BrowserRouter
        App.tsx                     # Routes: /welcome (first-run), / (connect), /chat, /settings
        store.ts                    # Zustand store: channels, users, messages, voice state, Tauri invoke/listen
        types.ts                    # TS types mirroring Rust: ChannelEntry, UserEntry, ChatMessage, FancyProfile, AudioSettings, etc.
        profileFormat.ts            # FancyMumble profile serialisation (comment marker <!--FANCY:{json}-->), base122 codec, data-URL helpers
        serverStorage.ts            # Persistent saved servers via @tauri-apps/plugin-store
        preferencesStorage.ts       # Persistent user preferences via @tauri-apps/plugin-store
        components/
          chat/                     # Chat view and all related components/hooks
            ChatView.tsx            # Chat message view with rich markdown input, GIF picker, polls
            ChatHeader.tsx          # Channel/group header bar
            ChatComposer.tsx        # Message input area with attachments
            ChatMessageList.tsx     # Message list rendering with date separators
            MessageItem.tsx         # Individual message with reactions, media, quotes
            QuotePreviewStrip.tsx   # Quote preview strip in composer
            MarkdownInput.tsx       # Overlay-based markdown input with live formatting preview
            GifPicker.tsx           # GIF/sticker search popup (Klipy API)
            MediaPreview.tsx        # Image/file preview component
            MessageContextMenu.tsx  # Right-click/long-press message menu
            MobileMessageActionSheet.tsx  # Mobile message action sheet
            MessageSelectionBar.tsx # Bulk message selection toolbar
            MobileCallControls.tsx  # Mobile mic/deaf toggle buttons
            PollCreator.tsx         # Poll creation modal (question, options, checkbox/radio)
            PollCard.tsx            # Poll rendering in chat with interactive voting
            useChatSend.ts          # Message sending hook
            useChatScroll.ts        # Scroll behavior hook
            useMessageSelection.ts  # Selection mode hook
            usePolls.ts             # Poll creation & voting hook
          sidebar/                  # Channel sidebar and user list
            ChannelSidebar.tsx      # Channel tree, user list with hover profile cards
            ModernChannelList.tsx   # Flat channel view with inline members
            SidebarSearchView.tsx   # Search overlay for channels, users, messages
            UserListItem.tsx        # User entry with avatar, mute/deaf badges
            UserContextMenu.tsx     # Right-click user menu (mute, volume, admin)
            ChannelEditorDialog.tsx # Channel creation/edit dialog
            ChannelInfoPanel.tsx    # Channel description, members, permissions
            PchatBadge.tsx          # Protocol indicator badge
            RecordingModal.tsx      # Recording indicator modal
          server/                   # Server connection and management
            ServerList.tsx          # Saved server list (connect page)
            PublicServerList.tsx    # Public server directory
            ServerEditSheet.tsx     # Server edit form
            ServerInfoPanel.tsx     # Server connection metadata panel
            PasswordDialog.tsx      # Server password dialog
          user/                     # User profile display
            UserProfileView.tsx     # Full-height right panel showing user profile
            UserInfoPanel.tsx       # User connection stats
            MobileProfileSheet.tsx  # Mobile user profile bottom sheet
          security/                 # Encryption, keys, persistent chat
            KeyTrustIndicator.tsx   # Trust level icon/badge
            KeyVerificationDialog.tsx  # Fingerprint verification dialog
            KeyShareWarningDialog.tsx  # Key share warning
            CustodianPrompt.tsx     # Custodian mode prompt
            PersistenceBanner.tsx   # Persistent chat info banner
            PersistentChatOverlays.tsx  # Container for security overlays
            InfoBanner.tsx          # Reusable info/warning banner
          layout/                   # App-level layout
            TitleBar.tsx            # Custom frameless title bar
            SuperSearch.tsx         # Global search overlay (Cmd+K)
          elements/                 # Reusable UI primitives
            ConfirmDialog.tsx       # Confirmation dialog
            KebabMenu.tsx           # Dropdown kebab menu
            Lightbox.tsx            # Full-screen media viewer with carousel
            MessageActionBar.tsx    # Message action bar (reactions, quote, etc.)
            MobileBottomSheet.tsx   # Swipe-to-dismiss bottom sheet
            QuoteBlock.tsx          # Quoted message block
            SafeHtml.tsx            # Sanitized HTML renderer
            ExternalLinkGuard.tsx   # External link confirmation guard
            SwipeableCard.tsx       # Swipeable list item
            TabbedPage.tsx          # Tabbed page container
            Toast.tsx               # Toast notification
        pages/
          ConnectPage.tsx           # Server connect / add-server page
          ChatPage.tsx              # Main connected view (sidebar + chat)
          WelcomePage.tsx           # First-run setup wizard
          settings/
            index.ts SettingsPage.tsx  # Tabbed settings container, auto-saves profile locally & to server
            AudioPanel.tsx          # Input device, VAD, gain settings
            VoicePanel.tsx          # Voice state controls
            ProfilePanel.tsx        # FancyMumble profile editor (no manual save button - auto-saved)
            ProfilePreviewCard.tsx  # Live profile preview (renders bio as HTML)
            BioEditor.tsx           # Tiptap WYSIWYG bio editor (bold, italic, underline, colour)
            AdvancedPanel.tsx       # Expert-mode settings
            ShortcutsPanel.tsx      # PTT key binding
            SharedControls.tsx      # Reusable setting controls
            ImageEditor.tsx         # Avatar/banner crop/resize
            imageUtils.ts           # Canvas helpers
            profileData.ts          # Profile save/load logic
            shortcutHelpers.ts      # Key combo helpers
        utils/
          media.ts                  # Media utility helpers
          platform.ts               # Platform detection (isMobilePlatform, isDesktopPlatform)

  mumble-gui/                       # Dioxus desktop GUI (alternative frontend)
    Dioxus.toml
    src/main.rs                     # Dioxus launch, App component, event bridge
    src/state.rs
    src/components/                 # channel_sidebar, chat_view, connect_page
    src/services/                   # mumble_backend (wraps mumble-protocol)
```

## Key architecture decisions

1. **Command pattern** - every user action (join channel, send message, set
   texture, etc.) is a self-contained struct implementing `CommandAction`.
   Adding a new command = new file + struct, no central dispatcher changes.

2. **Priority work queue** - the client event loop drains UDP (audio) first,
   then TCP (control), then user commands.  Audio is never starved.

3. **Event handler trait** - `EventHandler` has default no-op methods so
   consumers override only what they need.

4. **Backend ↔ Frontend bridge** - the Tauri backend exposes `#[tauri::command]`
   functions called via `invoke()` from React, and pushes updates via
   Tauri events consumed by `listen()` in the Zustand store.

5. **FancyMumble profile format** - profile customisation data is stored in
   the Mumble user `comment` field as `<!--FANCY:{"v":1,...}-->` followed by
   the bio HTML.  Legacy clients ignore the HTML comment.  Avatar bytes use
   the standard `UserState.texture` protobuf field.

6. **Base122 encoding** - binary data inside the profile comment (which must
   be valid UTF-8) is encoded with a custom base122 codec
   (`profileFormat.ts`: `b122Encode` / `b122Decode`).  The alphabet is
   ASCII 0–127 minus 6 illegal chars (NUL, LF, CR, `"`, `&`, `\`).  ~14 %
   smaller than base64.  Data URLs use `;base122,` as the encoding tag.
   `dataUrlToBytes` reads both `;base122` and legacy `;base64` for backwards
   compatibility.

7. **Persistent storage** - saved servers and user preferences are stored
   via `@tauri-apps/plugin-store` as JSON files (`servers.json`,
   `preferences.json`).  TLS client certificates are PEM files under
   `{app_data_dir}/certs/`.

8. **Audio pipeline** - trait-based: `AudioCapture` → `FilterChain` (noise
   gate, AGC) → `OpusEncoder` → network; inbound is the reverse.
   OS audio I/O uses `cpal` (desktop only, gated with
   `#[cfg(not(target_os = "android"))]`).

9. **Android platform gating** - `cpal` and `tauri-plugin-global-shortcut`
   are desktop-only dependencies.  Audio commands return stub errors on
   Android.  The `mod audio` (cpal wrappers), audio pipeline fields
   (`inbound_pipeline`, `outbound_task_handle`), and event handler audio
   callbacks are all behind `#[cfg(not(target_os = "android"))]`.

10. **Responsive UI** - CSS breakpoint at 768 px separates mobile from
    desktop.  On mobile: sidebar becomes a slide-in drawer with hamburger
    toggle, touch targets are 44 px minimum, TitleBar is hidden (Android
    has its own status bar), and `UserProfileView` is hidden.  Platform
    detection uses `isMobilePlatform()` from `utils/platform.ts`.

## Boy Scout Rule

> **"Always leave the campground cleaner than you found it."**

When working in any file, apply this rule proactively:

- **Fix issues you encounter** - if you spot a compiler warning, a lint
  error, an unused import, or a `TODO`/`FIXME` comment that has an obvious
  fix, clean it up as part of your change rather than leaving it for later.
- **Fix TypeScript errors** - unused variables, missing types, incorrect
  `noUnusedLocals` violations and similar issues must be resolved whenever
  they are found in a file being edited.
- **Fix SonarQube / linter suggestions** - style suggestions reported by
  SonarQube or other linters must also be fixed in files being edited.
  This includes, but is not limited to:
  - Cognitive complexity violations - refactor functions that exceed the
    allowed complexity threshold (e.g. extract helpers, simplify branches).
  - `String#replaceAll()` preference - replace `String#replace()` with a
    global regex with `String#replaceAll()` where applicable.
  - Any other "code smell" or maintainability issue flagged by the linter.
- **Fix Rust warnings** - dead code, unused imports, `clippy` warnings and
  similar diagnostics should be addressed whenever they appear in touched
  files.
- **Fix build-log warnings immediately** - any warning that appears in the
  output of a build (`cargo build`, `cargo check`, `cargo test`, `npm run
  build`, `tsc`, etc.) must be resolved before the task is considered
  complete.  Do not leave warnings for the next developer to clean up.
- **Suggest improvements proactively** - if you notice a code smell,
  a performance issue, or a pattern inconsistent with the rest of the
  codebase while working in a file, point it out and offer (or apply) a
  fix.  Examples: duplicate logic that could be extracted, a `clone()` that
  could be avoided, a React hook dependency array that is stale.
- **Scope**: apply the rule to files you are *already editing*.  Do not
  refactor unrelated files without being asked - only clean up what you
  touch.
- **pre-existing issues** - if you encounter an issue that predates your
  change and is non-trivial to fix, it's okay to leave a `TODO` comment with
  a brief description of the problem and (optionally) a link to an issue or PR
  for tracking.  The key is to avoid introducing new issues and to clean up what
  you can in the files you edit. But if you can easily fix an existing issue
  while working in the file, please do so.

## Quality gates after every implementation

After completing any non-trivial feature or bug fix, always perform these
steps before declaring the task done:

1. **Maximum File length** - if your change adds more than 200 lines to a file, consider
   whether it can be split into multiple smaller files.  If a file exceeds
   600 lines after your change, refactor it to reduce the length (e.g.
   extract helper modules, split frontend components into separate files).
2. **Run Clippy** (`cargo clippy --all-targets -- -D warnings`) and fix
   every diagnostic it reports.  Clippy failures are treated as build
   failures. Do not add `#[allow]` attributes to silence warnings -
   fix the underlying issue.
3. **Write regression tests** - add at least one automated test that
   exercises the new behaviour and would fail if the feature were removed
   or regressed.  Place Rust unit tests in the same file as the code under
   test (in a `#[cfg(test)] mod tests { ... }` block) or in the relevant
   integration test file under `tests/`.  Frontend tests go in
   `crates/mumble-tauri/ui/src/components/__tests__/`.
4. **Fix all build-log warnings** - run `cargo build` (or the appropriate
   build command) and resolve every warning before finishing.  A clean,
   warning-free build is a hard requirement.

## Coding conventions

### Rust
- Edition 2021, `resolver = "2"`
- Workspace-wide Clippy lints: `correctness = deny`, `suspicious/style/perf = warn`
- `thiserror` for error enums, `Result<T>` type alias per crate
- `tracing` for logging (not `log`)
- Async runtime: `tokio` with `rt-multi-thread`
- TLS: `rustls` with `ring` crypto provider
- Protobuf: `prost` + `prost-build`
- **Utility functions** - place new helper/utility functions in the `utils`
  module (i.e. `src/utils.rs` or `src/utils/`) of the crate being worked in.
  If a utility is general-purpose enough to benefit multiple crates, add it
  to the `fancy-utility` crate instead and depend on it from the consuming
  crate. Before creating a new utility function, check if one already exists
  in the `fancy-utils` crate to avoid duplication.
- When you need to add a comment  to explain what your code is doing, ask yourself:
  "Could I rewrite this code in a clearer way that doesn't require a comment?"
  If the answer is yes, refactor the code to be self-explanatory instead of adding
  a comment.  If the answer is no and a comment is truly necessary, keep it concise
  and focused on the "why" rather than the "what".  Avoid comments that simply restate
  what the code does or explain "how" - the code itself should make that clear.  Strive
  for code that is clean and readable enough to minimize the need for comments. Also,
  try to use function and variable names that convey intent clearly, which can further
  reduce the need for explanatory comments.

### TypeScript / React
- React 19 with function components and hooks
- State: Zustand 5 (`create` store, not context-based)
- Routing: react-router-dom 7 (`Routes`/`Route`)
- Styling: CSS Modules (`*.module.css`)
- Build: Vite 6, target `esnext`
- No ESLint config currently in repo
- `type` imports preferred (`import type { ... }`)
- **Reusable UI elements** - new primitive UI components (buttons, inputs,
  badges, tables, tooltips, modals, etc.) must be placed in
  `ui/src/components/elements/`.  If you encounter an existing component
  elsewhere in the codebase that is clearly a generic, reusable primitive,
  move it into that folder as part of your change (Boy Scout Rule).

### General
- MIT license
- CI: GitHub Actions (`.github/workflows/ci.yml`) - lint, test, build,
  Android APK build, auto-release on `main`
- Workspace managed with `cargo` (Rust) and `npm` (frontend, in `crates/mumble-tauri/ui`)
- **No non-ASCII characters in code comments** (Rust `//`/`/* */`, TypeScript `//`/`/* */`).
  Box-drawing characters and other Unicode glyphs (e.g. `┌──┐`, `│`, `└──┘`, `▼`, `─`)
  are forbidden in source-code comments.  They are fine in Markdown documentation
  files (`.md`).

## Common workflows

```bash
# Run the Tauri dev server (auto-starts Vite + cargo build)
cd crates/mumble-tauri
cargo tauri dev

# Run the Tauri Android dev server (requires emulator/device)
# See ANDROID_DEV.md for prerequisites
cd crates/mumble-tauri
cargo tauri android dev

# Android development helper script (Windows only)
# scripts/android-dev.ps1 - validates all prerequisites (JDK, ANDROID_HOME,
# NDK_HOME, Rust targets, Tauri CLI, ADB, emulator AVDs) and optionally
# launches the dev server or sets up WebView DevTools port-forwarding.
# It also auto-detects JAVA_HOME from common install locations and injects
# NDK CMake/ninja env vars so cargo builds find the toolchain.
#
# Prerequisites checked: Windows Developer Mode, JAVA_HOME, ANDROID_HOME,
# NDK_HOME (NDK 27), Rust targets aarch64-linux-android + x86_64-linux-android,
# Tauri CLI (cargo-tauri), ADB, emulator AVDs.
#
# Usage:
.\scripts\android-dev.ps1                        # Check prerequisites only
.\scripts\android-dev.ps1 -Run                   # Check + start dev server (cargo tauri android dev)
.\scripts\android-dev.ps1 -Emulator              # Launch an emulator (pick AVD interactively)
.\scripts\android-dev.ps1 -Emulator -Run         # Launch emulator, then start dev server
.\scripts\android-dev.ps1 -Inspect               # Set up WebView DevTools forwarding for chrome://inspect
.\scripts\android-dev.ps1 -Inspect -Serial emulator-5554  # Target a specific device
#
# To perform a release APK/AAB build instead of the dev server:
cd crates/mumble-tauri
cargo tauri android build

# Run the Dioxus dev server
cd crates/mumble-gui
dx serve

# Build the protocol library only
cargo build -p mumble-protocol

# Run unit tests (no Docker required)
cargo test --package mumble-protocol --features opus-codec --lib

# Run integration tests (requires Docker - see section below)
cd crates/mumble-protocol
docker compose -f docker-compose.test.yml up -d --wait
cargo test --package mumble-protocol --test integration
docker compose -f docker-compose.test.yml down

# Frontend only (standalone Vite dev server for UI iteration)
cd crates/mumble-tauri/ui
npm run dev

# Run frontend unit tests
cd crates/mumble-tauri/ui
npm test
```

## Integration tests

The protocol library includes integration tests that run against a real
Mumble server in Docker.  They live in
`crates/mumble-protocol/tests/integration.rs`.

### Prerequisites

- **Docker** (or Docker Desktop) must be running.
- No other process may occupy port **64738** (TCP + UDP).
- **Windows / Hyper-V note**: Hyper-V may reserve port 64738.  Check with
  `netsh interface ipv4 show excludedportrange protocol=udp`.  If blocked,
  set `MUMBLE_TEST_PORT` to a free port (e.g. `63738`) before starting
  Docker Compose and running tests.  Both `docker-compose.test.yml` and
  `integration.rs` read this env var (default 64738).

### Server configuration

The Docker Compose file
(`crates/mumble-protocol/docker-compose.test.yml`) starts a
`mumblevoip/mumble-server:latest` container configured via
`MUMBLE_CONFIG_*` environment variables that relax limits for testing:

| Setting | Value | Purpose |
|---------|-------|---------|
| `textmessagelength` | 128 KiB | Allow large text messages |
| `imagemessagelength` | 10 MiB | Allow large inline images |
| `allowhtml` | true | HTML-formatted messages |
| `certrequired` | false | No client certs needed |
| `autobanAttempts` | 0 | Disable auto-banning |

### Running

```bash
# 1. Start the test server (waits for healthy status)
cd crates/mumble-protocol
docker compose -f docker-compose.test.yml up -d --wait

# On Windows with Hyper-V port conflict, set an alternative port first:
# export MUMBLE_TEST_PORT=63738   # bash
# $env:MUMBLE_TEST_PORT="63738"   # PowerShell

# 2. Run all integration tests
cargo test --package mumble-protocol --test integration

# 3. Run a specific integration test
cargo test --package mumble-protocol --test integration -- test_plugin_data_transmission_between_two_clients

# 4. Tear down
docker compose -f docker-compose.test.yml down
```

### Test coverage

| Test | What it verifies |
|------|-----------------|
| `test_tcp_connect_and_version_exchange` | TLS handshake and version exchange |
| `test_full_authentication_flow` | Authenticate → ServerSync, session ID assigned |
| `test_send_text_message` | Send and (optionally) receive text message echo |
| `test_send_large_image_message` | Large base64 image within server limits |
| `test_set_self_mute_and_deaf` | Self-mute/deaf UserState round-trip |
| `test_set_comment` | Comment set and echoed by server |
| `test_ping_keepalive` | TCP ping/pong |
| `test_multiple_concurrent_connections` | Two clients see each other |
| `test_server_config_has_large_limits` | Server config matches test-mumble.ini |
| `test_plugin_data_transmission_between_two_clients` | Client A sends `PluginDataTransmission` → Client B receives it with correct payload and sender session |
| `test_plugin_data_empty_receivers_not_delivered` | Empty `receiver_sessions` → message not forwarded (confirms Mumble server behaviour) |
| `test_poll_roundtrip_create_and_vote` | Full poll flow: create poll → deliver → vote → deliver vote back |
| `test_poll_bidirectional_sending` | Both A→B and B→A poll delivery works (not one-directional) |
| `test_poll_multiple_senders_same_channel` | Three users each send polls, all receive each other's |
| `test_poll_cross_channel_is_delivered` | PluginData IS delivered to explicitly-listed sessions across channels |
| `test_poll_mixed_channels_only_same_channel_receives` | Mixed-channel scenario: all explicitly-listed targets receive regardless of channel |

> **Note:** The Mumble server delivers `PluginDataTransmission` to ALL
> explicitly listed `receiver_sessions` regardless of channel membership.
> Channel-scoped poll delivery is enforced by the UI (only listing
> same-channel users as targets).

### Graceful skip

All tests call `ensure_server_available()` first.  If the server is
unreachable they print a warning and return early instead of failing,
so `cargo test` still passes without Docker.

## Frontend tests

The React frontend has unit tests using **Vitest** + **@testing-library/react**,
located in `crates/mumble-tauri/ui/src/components/__tests__/`.

```bash
cd crates/mumble-tauri/ui
npm test          # single run
npm run test:watch # watch mode
```

| Test file | What it covers |
|-----------|---------------|
| `PollCard.test.ts` | Module-level stores: `pollStore`, `voteStore`, `localVotes`, `PollPayload` wire format |
| `ChatViewPolls.test.ts` | Poll logic: channel filtering, poll-marker regex, dedup, target computation, payload round-trips, bidirectional delivery simulation |
| `PollCardRender.test.tsx` | React component rendering: question/options display, voting interactions, single/multi choice indicators, vote percentages |
| `StorePollProcessing.test.ts` | Regression: Zustand store `addPoll` action, simulated plugin-data event processing, creator name resolution, multi-sender scenarios, reset resilience |

## Tauri commands (Rust → JS bridge)

| Command | Parameters | Returns | Purpose |
|---------|-----------|---------|---------|
| `connect` | host, port, username, cert_label? | `()` | Connect to server |
| `disconnect` | - | `()` | Disconnect |
| `get_status` | - | `ConnectionStatus` | Current connection status |
| `get_channels` | - | `ChannelEntry[]` | All channels |
| `get_users` | - | `UserEntry[]` | All users |
| `get_messages` | channel_id | `ChatMessage[]` | Messages for channel |
| `send_message` | channel_id, body | `()` | Send text message |
| `select_channel` | channel_id | `()` | UI selection |
| `join_channel` | channel_id | `()` | Move to channel |
| `toggle_listen` | channel_id | `bool` | Listen/unlisten |
| `get_listened_channels` | - | `u32[]` | Listened channel IDs |
| `get_unread_counts` | - | `Map<u32,u32>` | Unread per channel |
| `mark_channel_read` | channel_id | - | Clear unread |
| `get_server_config` | - | `ServerConfig` | Server limits |
| `ping_server` | host, port | `PingResult` | Ping latency |
| `generate_certificate` | label | `()` | Create self-signed cert |
| `list_certificates` | - | `string[]` | Cert labels |
| `delete_certificate` | label | `()` | Remove cert |
| `get_audio_devices` | - | `AudioDevice[]` | List mics |
| `get_audio_settings` | - | `AudioSettings` | Current audio config |
| `set_audio_settings` | settings | - | Update audio config |
| `get_voice_state` | - | `VoiceState` | Mute/deaf state |
| `enable_voice` | - | `()` | Unmute+undeaf |
| `disable_voice` | - | `()` | Go deaf+muted |
| `toggle_mute` | - | `()` | Toggle mic |
| `toggle_deafen` | - | `()` | Toggle deaf |
| `set_user_comment` | comment | `()` | Set profile comment |
| `set_user_texture` | texture (`Vec<u8>`) | `()` | Set avatar |
| `send_plugin_data` | receiver_sessions, data (`Vec<u8>`), data_id | `()` | Send plugin data (polls, etc.) - receiver_sessions must list each target explicitly |
| `get_own_session` | - | `u32 \| null` | Our own session ID (after connect) |
| `reset_app_data` | - | `()` | Factory reset |

## Key types (TypeScript)

```typescript
// types.ts
ChannelEntry    { id, parent_id, name, description, user_count }
UserEntry       { session, name, channel_id, texture: number[]|null, comment: string|null }
ChatMessage     { sender_session, sender_name, body, channel_id, is_own }
SavedServer     { id, label, host, port, username, cert_label }
UserPreferences { userMode, hasCompletedSetup, defaultUsername }
AudioSettings   { selected_device, auto_gain, vad_threshold, max_gain_db, noise_gate_close_ratio, hold_frames, push_to_talk, push_to_talk_key }
FancyProfile    { v, decoration, nameplate, effect, banner: {color,image}, nameStyle: {font,color,gradient,glow,bold,italic}, cardBackground, cardBackgroundCustom, avatarBorder, avatarBorderCustom, status }
VoiceState      = "inactive" | "active" | "muted"
ConnectionStatus = "disconnected" | "connecting" | "connected"
```

## Key types (Rust - mumble-protocol)

```rust
// state.rs
User     { session, name, channel_id, mute, deaf, self_mute, self_deaf, comment, texture, hash }
Channel  { channel_id, parent_id, name, description, position, temporary, max_users }
ServerState { connection: ConnectionInfo, users: HashMap<u32,User>, channels: HashMap<u32,Channel> }

// client.rs
ClientConfig { tcp: TcpConfig, udp: UdpConfig, ping_interval }
ClientHandle { send<C: CommandAction>(cmd) }

// error.rs
Error { Io, Tls, Decode, Encode, UnknownMessageType, Rejected, ConnectionClosed, QueueClosed, InvalidState, OpusCodec, Other }
```
