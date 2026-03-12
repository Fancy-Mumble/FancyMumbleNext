# Android Development Setup

Quick guide for building and debugging Fancy Mumble on Android.

## Prerequisites

| Tool | Version | Download |
|------|---------|----------|
| Android Studio | Latest | https://developer.android.com/studio |
| JDK | 17+ | Bundled with Android Studio |
| Rust Android targets | stable | See below |
| Tauri CLI | 2.x | `cargo install tauri-cli --version "^2"` |

## One-time setup

### 1. Install Android Studio

Download and install Android Studio. During setup, ensure these are
installed via **SDK Manager** (`Settings > Languages & Frameworks > Android SDK`):

- **SDK Platforms**: Android 14 (API 34)
- **SDK Tools**:
  - Android SDK Build-Tools 34
  - Android SDK Command-line Tools
  - Android Emulator
  - NDK (Side by side) - version **27.0.12077973**

### 2. Environment variables

Set these in your system environment (PowerShell example):

```powershell
# Typical Android Studio paths on Windows
[System.Environment]::SetEnvironmentVariable("JAVA_HOME", "C:\Program Files\Android\Android Studio\jbr", "User")
[System.Environment]::SetEnvironmentVariable("ANDROID_HOME", "$env:LOCALAPPDATA\Android\Sdk", "User")
[System.Environment]::SetEnvironmentVariable("NDK_HOME", "$env:LOCALAPPDATA\Android\Sdk\ndk\27.0.12077973", "User")
```

Add to PATH:
```powershell
# Add platform-tools (adb) and emulator to PATH
$sdkPath = "$env:LOCALAPPDATA\Android\Sdk"
$newPath = "$sdkPath\platform-tools;$sdkPath\emulator;$env:PATH"
[System.Environment]::SetEnvironmentVariable("PATH", $newPath, "User")
```

Restart your terminal after setting these.

### 3. Rust Android targets

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android i686-linux-android
```

### 4. Create an emulator (AVD)

Open Android Studio > **Device Manager** > **Create Virtual Device**:

- Device: **Pixel 7** (or any phone)
- System image: **API 34** (x86_64 for Intel/AMD, arm64 for ARM)
- RAM: 2048 MB+
- Storage: 2048 MB+

Alternatively via command line:
```bash
# List available system images
sdkmanager --list | grep "system-images;android-34"

# Install a system image
sdkmanager "system-images;android-34;google_apis;x86_64"

# Create AVD
avdmanager create avd -n FancyMumble -k "system-images;android-34;google_apis;x86_64" -d pixel_7
```

## Running

### Start the emulator

```bash
# List available AVDs
emulator -list-avds

# Launch (replace FancyMumble with your AVD name)
emulator -avd FancyMumble
```

Or open Android Studio > Device Manager > Play button.

### Dev mode (hot-reload)

```bash
cd crates/mumble-tauri
cargo tauri android dev
```

This will:
1. Start the Vite dev server (hot-reload for frontend)
2. Compile Rust code for the connected device/emulator architecture
3. Install and launch the app on the emulator

### Release build

```bash
cd crates/mumble-tauri
cargo tauri android build --apk
```

APK output: `gen/android/app/build/outputs/apk/universal/release/app-universal-release-unsigned.apk`

## Troubleshooting

### "No connected devices"

Make sure the emulator is running or a device is connected:
```bash
adb devices
```

### Slow first build

The first Android build compiles all Rust dependencies for the target
architecture. Subsequent builds use the cargo cache and are much faster.

### Port conflicts

The Tauri/Vite dev server runs on port 1420 by default. If it conflicts,
update `devUrl` in `tauri.conf.json` and use the same port in your Vite
config (`server.port`).

For Android, ensure the dev server is reachable from the emulator/device:

- Bind Vite to all interfaces (`server.host: true` or `0.0.0.0`)
- Or use `TAURI_DEV_HOST` when running `cargo tauri android dev`

### NDK not found

Verify `NDK_HOME` points to the correct NDK installation:
```powershell
ls $env:NDK_HOME
# Should show: build, meta, ndk-build, prebuilt, ...
```

### Audio not available

Audio capture/playback is not yet implemented on Android. Voice-related
controls will show an error message when used on Android.
