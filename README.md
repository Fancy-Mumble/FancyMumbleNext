# Fancy Mumble

A modern desktop [Mumble](https://www.mumble.info/) (VoIP) client with
profile customisation features - avatar frames, banners, nameplates,
effects, and more.

Built with **Rust** (protocol + backend) and **TypeScript / React**
(frontend), packaged as a native app via [Tauri 2](https://v2.tauri.app/).

> **Status:** Early development - expect rough edges.

---

## Features

- Full Mumble protocol support (TCP + UDP, TLS, Opus voice)
- Modern glassmorphic UI with channel tree, chat, and user profiles
- **Rich chat** - Markdown formatting, inline images, GIFs (Klipy), clickable URLs
- **Polls** - Create single- or multi-choice polls delivered via plugin data
- **Profile customisation** - Bio editor (WYSIWYG), avatar frames, banners,
  nameplates, name styles, card backgrounds
- **Voice controls** - Push-to-talk & voice activity detection, per-channel
  listen, mute/deafen
- **Audio pipeline** - Noise gate, automatic gain control, Opus codec
- Self-signed TLS client certificates
- Persistent saved servers & user preferences
- Cross-platform: Windows and Linux

## Architecture

| Layer | Location | Tech |
|-------|----------|------|
| Protocol library | `crates/mumble-protocol` | Rust, tokio, prost, rustls |
| Tauri backend | `crates/mumble-tauri` | Rust, Tauri 2, cpal |
| Frontend | `crates/mumble-tauri/ui` | React 19, Vite 6, Zustand 5, TypeScript 5 |

See [`crates/mumble-protocol/doc/`](crates/mumble-protocol/doc/) for
detailed protocol library documentation.

## Getting Started

### Prerequisites

- **Rust** (stable, edition 2021) - [rustup.rs](https://rustup.rs/)
- **Node.js 22+** - [nodejs.org](https://nodejs.org/)
- **Tauri CLI** - `cargo install tauri-cli --version "^2"`
- **System dependencies** (Linux only):
  ```bash
  sudo apt-get install -y \
    libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf \
    libasound2-dev libgtk-3-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev
  ```

### Development

```bash
# Install frontend dependencies
cd crates/mumble-tauri/ui
npm install
cd ../../..

# Start the Tauri dev server (hot-reloads both Rust and JS)
cd crates/mumble-tauri
cargo tauri dev
```

### Building

```bash
cd crates/mumble-tauri
cargo tauri build
```

Installers will be in `target/release/bundle/`.

### Running Tests

```bash
# Frontend unit tests
cd crates/mumble-tauri/ui
npm test

# Rust unit tests
cargo test --package mumble-protocol --features opus-codec --lib

# Integration tests (requires Docker)
cd crates/mumble-protocol
docker compose -f docker-compose.test.yml up -d --wait
cargo test --package mumble-protocol --test integration
docker compose -f docker-compose.test.yml down
```

## Contributing

Contributions are welcome! Please read [CONTRIBUTING.md](.github/CONTRIBUTING.md)
before submitting a pull request.

## License

This project is licensed under the [MIT License](LICENSE).

## Links

- [GitHub Repository](https://github.com/Fancy-Mumble/FancyMumbleNext)
- [Mumble Protocol Documentation](https://mumble-protocol.readthedocs.io/)
- [Tauri 2 Documentation](https://v2.tauri.app/)
