//! Audio device enumeration, audio settings, voice toggles, mic test,
//! latency test and recording.

use crate::audio;
use crate::state::{self, AppState, AudioDevice, AudioSettings, VoiceState};

/// List available audio input devices (microphones).
/// Only available on desktop (cpal is not supported on Android).
#[cfg(not(target_os = "android"))]
#[tauri::command]
pub(crate) fn get_audio_devices() -> Vec<AudioDevice> {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|d| {
            d.description()
                .ok()
                .map(|desc| desc.name().to_string())
        });

    host.input_devices()
        .map(|devices| {
            devices
                .filter_map(|d| {
                    let name = d
                        .description()
                        .ok()
                        .map(|desc| desc.name().to_string())?;
                    Some(AudioDevice {
                        name: name.clone(),
                        is_default: default_name.as_deref() == Some(&name),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Stub: on Android, return an empty device list.
#[cfg(target_os = "android")]
#[tauri::command]
pub(crate) fn get_audio_devices() -> Vec<AudioDevice> {
    Vec::new()
}

/// List available audio output devices (speakers/headphones).
/// Only available on desktop (cpal is not supported on Android).
#[cfg(not(target_os = "android"))]
#[tauri::command]
pub(crate) fn get_output_devices() -> Vec<AudioDevice> {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = cpal::default_host();
    let default_name = host
        .default_output_device()
        .and_then(|d| {
            d.description()
                .ok()
                .map(|desc| desc.name().to_string())
        });

    host.output_devices()
        .map(|devices| {
            devices
                .filter_map(|d| {
                    let name = d
                        .description()
                        .ok()
                        .map(|desc| desc.name().to_string())?;
                    Some(AudioDevice {
                        name: name.clone(),
                        is_default: default_name.as_deref() == Some(&name),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Stub: on Android, return an empty device list.
#[cfg(target_os = "android")]
#[tauri::command]
pub(crate) fn get_output_devices() -> Vec<AudioDevice> {
    Vec::new()
}

/// Get current audio settings.
#[tauri::command]
pub(crate) fn get_audio_settings(state: tauri::State<'_, AppState>) -> AudioSettings {
    state.audio_settings()
}

/// List the tunable knobs the given denoiser algorithm exposes.
///
/// The UI uses this to render dynamic sliders in advanced/expert
/// mode without hard-coding the knob list per algorithm.
#[tauri::command]
pub(crate) fn get_denoiser_param_specs(
    algorithm: mumble_protocol::audio::filter::denoiser::NoiseSuppressionAlgorithm,
) -> Vec<mumble_protocol::audio::filter::denoiser::DenoiserParamSpec> {
    mumble_protocol::audio::filter::denoiser::algorithm_param_specs(algorithm).to_vec()
}

/// List the noise-suppression algorithms whose backends are actually
/// compiled into this build.
#[tauri::command]
pub(crate) fn get_available_denoiser_algorithms()
 -> Vec<mumble_protocol::audio::filter::denoiser::NoiseSuppressionAlgorithm>
{
    mumble_protocol::audio::filter::denoiser::NoiseSuppressionAlgorithm::available()
}

/// Update audio settings.
///
/// If any pipeline-relevant setting changes while voice is active, the
/// capture/playback pipelines are automatically restarted as needed.
#[tauri::command]
pub(crate) async fn set_audio_settings(
    state: tauri::State<'_, AppState>,
    settings: AudioSettings,
) -> Result<(), String> {
    let force_tcp = settings.force_tcp_audio;
    let (needs_outbound, needs_inbound, force_tcp_changed) = state
        .set_audio_settings(settings)
        .unwrap_or((false, false, false));

    if needs_outbound {
        state.restart_outbound()?;
    }
    if needs_inbound {
        state.restart_inbound()?;
    }
    if force_tcp_changed {
        if let Ok(inner) = state.inner.snapshot().lock() {
            if let Some(ref handle) = inner.conn.client_handle {
                handle.set_force_tcp(force_tcp);
            }
        }
    }

    Ok(())
}

/// Switch the desktop audio backend at runtime.
///
/// `true` selects the rodio backend (default), `false` selects the
/// legacy cpal backend. Takes effect on the next voice toggle.
#[tauri::command]
pub(crate) fn set_audio_backend(use_rodio: bool) {
    audio::set_use_rodio_backend(use_rodio);
}

/// Returns `true` if the rodio backend is selected, `false` for legacy cpal.
#[tauri::command]
pub(crate) fn get_audio_backend() -> bool {
    audio::is_rodio_backend()
}

/// Get the current voice state.
#[tauri::command]
pub(crate) fn get_voice_state(state: tauri::State<'_, AppState>) -> VoiceState {
    state.voice_state()
}

/// Enable voice calling for the current channel.
/// Sends unmute/undeaf to the server.
#[tauri::command]
pub(crate) async fn enable_voice(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.enable_voice().await
}

/// Disable voice calling (go back to deaf+muted).
#[tauri::command]
pub(crate) async fn disable_voice(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.disable_voice().await
}

/// Toggle mute (mic on/off, still hearing).
#[tauri::command]
pub(crate) async fn toggle_mute(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.toggle_mute().await
}

/// Toggle deafen (all audio on/off).
#[tauri::command]
pub(crate) async fn toggle_deafen(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.toggle_deafen().await
}

/// Set the local playback volume for a specific remote user.
///
/// `volume` is a multiplier (0.0 = muted, 1.0 = normal, 2.0 = 200%).
#[tauri::command]
pub(crate) fn set_user_volume(state: tauri::State<'_, AppState>, session: u32, volume: f32) {
    state.set_user_volume(session, volume);
}

/// Start monitoring the microphone and emitting amplitude events.
#[tauri::command]
pub(crate) fn start_mic_test(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.start_mic_test()
}

/// Stop monitoring the microphone.
#[tauri::command]
pub(crate) fn stop_mic_test(state: tauri::State<'_, AppState>) {
    state.stop_mic_test();
}

/// Calibrate the voice activation threshold by measuring the ambient
/// noise floor for ~2 seconds (with AGC applied).  Returns the new threshold.
#[tauri::command]
pub(crate) async fn calibrate_voice_threshold(state: tauri::State<'_, AppState>) -> Result<f32, String> {
    state.calibrate_voice_threshold().await
}

/// Start periodic TCP pings for live latency measurement.
#[tauri::command]
pub(crate) fn start_latency_test(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.start_latency_test()
}

/// Stop the latency test.
#[tauri::command]
pub(crate) fn stop_latency_test(state: tauri::State<'_, AppState>) {
    state.stop_latency_test();
}

/// Start recording inbound audio to a file.
#[tauri::command]
pub(crate) fn start_recording(
    state: tauri::State<'_, AppState>,
    directory: String,
    filename: String,
    format: state::recording::RecordingFormat,
) -> Result<String, String> {
    state.start_recording(directory, filename, format)
}

/// Stop the current recording and finalize the file.
#[tauri::command]
pub(crate) fn stop_recording(state: tauri::State<'_, AppState>) -> Result<String, String> {
    state.stop_recording()
}

/// Get the current recording state.
#[tauri::command]
pub(crate) fn get_recording_state(state: tauri::State<'_, AppState>) -> state::recording::RecordingState {
    state.recording_state()
}
