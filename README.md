<div align="center">

# Fancy Mumble

### A Modern, Feature-Rich Mumble Client

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=flat&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![TypeScript](https://img.shields.io/badge/typescript-%23007ACC.svg?style=flat&logo=typescript&logoColor=white)](https://www.typescriptlang.org/)
[![React](https://img.shields.io/badge/react-%2320232a.svg?style=flat&logo=react&logoColor=%2361DAFB)](https://reactjs.org/)
[![Tauri](https://img.shields.io/badge/tauri-%2324C8DB.svg?style=flat&logo=tauri&logoColor=%23FFFFFF)](https://tauri.app/)

**Fancy Mumble** brings modern UI/UX design and powerful customization features to the legendary [Mumble](https://www.mumble.info/) voice chat platform.

Built with **Rust** for rock-solid performance and **React** for a sleek, responsive interface.

[Features](#features) • [Screenshots](#screenshots) • [Getting Started](#getting-started) • [Building](#building) • [Server](#related-projects)

</div>

---

## Overview

Fancy Mumble is a next-generation desktop client for Mumble that combines the reliability of the battle-tested Mumble protocol with modern features users expect in 2026. Whether you're coordinating with your gaming guild, hosting a podcast, or running a community server, Fancy Mumble delivers crystal-clear voice communication with style.

> **Status:** Active development - Core features are functional, but expect some rough edges as we polish the experience.

---

## Features

- **Crystal-clear voice** - Opus codec with AI-powered noise suppression (DeepFilterNet3), AGC, and noise gate
- **Rich chat** - Markdown formatting, inline images, GIF picker, and interactive polls
- **Profile customization** - Custom avatar frames, banners, nameplates, and WYSIWYG bio editor
- **Modern glassmorphic UI** - Responsive design for desktop and Android
- **Flexible voice controls** - Push-to-talk, voice activity detection, per-channel listening
- **Secure** - TLS encryption, self-signed certificates, no telemetry
- **Cross-platform** - Windows, Linux, and Android support

---

## Screenshots

### Main Interface
![Main Chat Interface](https://via.placeholder.com/800x500/1a1a2e/eaeaea?text=Main+Chat+Interface+-+Replace+with+actual+screenshot)
*The main view - channels on the left, chat in the middle, user info on the right*

### Profile Customization
![Profile Editor](https://via.placeholder.com/800x500/16213e/eaeaea?text=Profile+Editor+-+Replace+with+actual+screenshot)
*Customize your profile with frames, banners, and a bio editor*

### Rich Chat Features
![Rich Chat](https://via.placeholder.com/800x500/0f3460/eaeaea?text=Rich+Chat+Features+-+Replace+with+actual+screenshot)
*Send formatted messages, images, GIFs, and even create polls*

### Audio Settings
![Audio Pipeline](https://via.placeholder.com/800x500/533483/eaeaea?text=Audio+Settings+-+Replace+with+actual+screenshot)
*Configure your mic settings and enable AI noise suppression*

---

## Architecture

Fancy Mumble is built as a Rust workspace with multiple crates:

| Crate | Purpose |
|-------|---------|
| [`mumble-protocol`](crates/mumble-protocol) | Core Mumble protocol implementation - TCP/UDP, TLS, Opus, audio pipeline |
| [`mumble-tauri`](crates/mumble-tauri) | Tauri desktop app - native audio I/O, backend commands, state management |
| [`mumble-tauri/ui`](crates/mumble-tauri/ui) | React frontend - chat UI, profile editor, settings |
| [`fancy-denoiser-deepfilter`](crates/fancy-denoiser-deepfilter) | AI noise suppression using DeepFilterNet3 |
| [`fancy-utils`](crates/fancy-utils) | Shared utility functions |

**Tech Stack:** Rust + Tauri 2 + React 19 + TypeScript 5 + Tokio async runtime

For detailed documentation, see [`crates/mumble-protocol/doc/`](crates/mumble-protocol/doc/).

---

## Getting Started

### Prerequisites

- **Rust** (stable, edition 2021 or later) - [Install via rustup](https://rustup.rs/)
- **Node.js** (v22 or later) - [Download from nodejs.org](https://nodejs.org/)
- **Tauri CLI** - Install with: `cargo install tauri-cli --version "^2"`

#### Platform-Specific Dependencies

**Linux:**
```bash
sudo apt-get update
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev \
  libappindicator3-dev \
  librsvg2-dev \
  patchelf \
  libasound2-dev \
  libgtk-3-dev \
  libsoup-3.0-dev \
  libjavascriptcoregtk-4.1-dev
```

**Android:**
See [ANDROID_DEV.md](ANDROID_DEV.md) for complete Android development setup instructions.

### Quick Start

1. **Clone the repository**
   ```bash
   git clone https://github.com/Fancy-Mumble/FancyMumbleNext.git
   cd FancyMumbleNext
   ```

2. **Install frontend dependencies**
   ```bash
   cd crates/mumble-tauri/ui
   npm install
   cd ../../..
   ```

3. **Run the development server**
   ```bash
   cd crates/mumble-tauri
   cargo tauri dev
   ```

The app will launch with hot-reloading enabled for both Rust and TypeScript changes.

---

## Building

### Desktop (Windows/Linux)

```bash
cd crates/mumble-tauri
cargo tauri build
```

Production installers will be generated in `target/release/bundle/`:
- **Windows:** `.exe` installer, `.msi` package
- **Linux:** `.deb`, `.AppImage`, `.rpm`

### Android

```bash
cd crates/mumble-tauri
cargo tauri android build
```

APK/AAB files will be in `gen/android/app/build/outputs/`.

For development with hot-reload:
```bash
cargo tauri android dev
```

Or use the helper script (Windows):
```powershell
.\scripts\android-dev.ps1 -Run
```

---

## Testing

### Frontend Unit Tests
```bash
cd crates/mumble-tauri/ui
npm test              # Single run
npm run test:watch    # Watch mode
```

### Rust Unit Tests
```bash
cargo test --package mumble-protocol --features opus-codec --lib
```

### Integration Tests

Integration tests run against a real Mumble server in Docker:

```bash
cd crates/mumble-protocol

# Start test server
docker compose -f docker-compose.test.yml up -d --wait

# Run tests
cargo test --package mumble-protocol --test integration

# Run specific test
cargo test --package mumble-protocol --test integration -- test_plugin_data_transmission_between_two_clients

# Cleanup
docker compose -f docker-compose.test.yml down
```

**Note:** Docker must be running, and port 64738 (TCP+UDP) must be available.

---

## Related Projects

### Server Implementation

Fancy Mumble works with any standard Mumble server. We maintain an enhanced fork with additional features:

**SetZero/mumble-server** - [github.com/SetZero/mumble-server](https://github.com/SetZero/mumble-server)

Key server components:
- [**Protocol Implementation**](https://github.com/SetZero/mumble-server/tree/1.6.x/src) - C++ server core (Mumble.proto, MumbleUDP.proto)
- [**User Management**](https://github.com/SetZero/mumble-server/blob/1.6.x/src/User.cpp) - User state, authentication, and permissions
- [**Channel System**](https://github.com/SetZero/mumble-server/blob/1.6.x/src/Channel.cpp) - Channel hierarchy and ACL
- [**ACL Engine**](https://github.com/SetZero/mumble-server/blob/1.6.x/src/ACL.cpp) - Access control lists and groups
- [**HTML Filtering**](https://github.com/SetZero/mumble-server/blob/1.6.x/src/HTMLFilter.cpp) - Safe HTML rendering in comments/messages
- [**Ban Management**](https://github.com/SetZero/mumble-server/blob/1.6.x/src/Ban.cpp) - Server ban system

### Official Resources

- [Mumble Official Website](https://www.mumble.info/)
- [Mumble Protocol Documentation](https://mumble-protocol.readthedocs.io/)
- [Mumble GitHub](https://github.com/mumble-voip/mumble)

---

## Contributing

We welcome contributions! Whether you're fixing bugs, adding features, improving documentation, or suggesting ideas, your help is appreciated.

**Before contributing:**
1. Check existing issues and pull requests to avoid duplicates
2. Read the [CONTRIBUTING.md](.github/CONTRIBUTING.md) guidelines
3. Review the [Copilot Instructions](.github/copilot-instructions.md) for coding conventions
4. Follow the Boy Scout Rule: leave code cleaner than you found it

**Development workflow:**
1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Make your changes with clear, descriptive commits
4. Run tests and linters (`cargo clippy`, `cargo test`, `npm test`)
5. Push to your fork and open a pull request

---

## License

This project is licensed under the **MIT License** - see the [LICENSE](LICENSE) file for details.

---

## Acknowledgments

- **Mumble Team** - For creating and maintaining the excellent Mumble protocol
- **Tauri Team** - For the amazing cross-platform framework
- **Rust Community** - For the incredible ecosystem and tools
- All contributors who help make Fancy Mumble better

---

<div align="center">

**Built with ❤️ by the Fancy Mumble Team**

[Report Bug](https://github.com/Fancy-Mumble/FancyMumbleNext/issues) • [Request Feature](https://github.com/Fancy-Mumble/FancyMumbleNext/issues) • [Join Discussion](https://github.com/Fancy-Mumble/FancyMumbleNext/discussions)

</div>
