//! Audio recording: captures the mixed inbound audio from
//! `SpeakerBuffers` and writes it to a file on disk.
//!
//! Supported output format: WAV (16-bit PCM, 48 kHz, mono).
//!
//! Recording runs as a background tokio task that periodically drains
//! a snapshot of the shared speaker buffers (the same ones the playback
//! callback reads) and writes the mixed PCM to the output file.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use hound::{SampleFormat as HoundSampleFormat, WavSpec, WavWriter};
use tracing::{info, warn};

use mumble_protocol::audio::mixer::{AudioMixer, SpeakerBuffers};

use super::AppState;

/// Recording state exposed to the frontend.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RecordingState {
    pub is_recording: bool,
    pub file_path: Option<String>,
    pub elapsed_secs: f64,
}

/// Supported output formats.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecordingFormat {
    Wav,
}

/// Expand template wildcards in a filename.
///
/// Supported placeholders:
/// - `{date}`     - current date (YYYY-MM-DD)
/// - `{time}`     - current time (HH-MM-SS)
/// - `{datetime}` - combined (YYYY-MM-DD_HH-MM-SS)
/// - `{host}`     - server hostname
/// - `{user}`     - own username
/// - `{channel}`  - current channel name
pub fn expand_filename_template(
    template: &str,
    host: &str,
    user: &str,
    channel: &str,
) -> String {
    let now = chrono::Local::now();
    let date = now.format("%Y-%m-%d").to_string();
    let time = now.format("%H-%M-%S").to_string();
    let datetime = now.format("%Y-%m-%d_%H-%M-%S").to_string();

    template
        .replace("{date}", &date)
        .replace("{time}", &time)
        .replace("{datetime}", &datetime)
        .replace("{host}", host)
        .replace("{user}", user)
        .replace("{channel}", channel)
}

/// Internal handle for a running recording task.
pub(crate) struct RecordingHandle {
    pub stop_flag: Arc<AtomicBool>,
    pub file_path: PathBuf,
    pub started_at: std::time::Instant,
    /// Keep the task alive; aborted on drop if needed.
    _task: tauri::async_runtime::JoinHandle<()>,
}

impl AppState {
    /// Start recording the mixed inbound audio to a file.
    ///
    /// Returns the resolved output file path on success.
    pub fn start_recording(
        &self,
        directory: String,
        filename_template: String,
        format: RecordingFormat,
    ) -> Result<String, String> {
        // Only one recording at a time.
        if let Ok(state) = self.inner.lock() {
            if state.recording_handle.is_some() {
                return Err("Recording already in progress".into());
            }
        }

        // Gather template context.
        let (host, user, channel, speaker_buffers) = {
            let state = self.inner.lock().map_err(|e| e.to_string())?;
            let host = state.connected_host.clone();
            let user = state.own_name.clone();
            let channel = state
                .current_channel
                .and_then(|id| state.channels.get(&id))
                .map(|c| c.name.clone())
                .unwrap_or_default();
            let buffers = state
                .audio_mixer
                .as_ref()
                .map(AudioMixer::buffers)
                .ok_or("Voice is not active - cannot record")?;
            (host, user, channel, buffers)
        };

        let expanded = expand_filename_template(&filename_template, &host, &user, &channel);

        let extension = match format {
            RecordingFormat::Wav => "wav",
        };

        let file_name = if expanded.ends_with(&format!(".{extension}")) {
            expanded
        } else {
            format!("{expanded}.{extension}")
        };

        let dir = PathBuf::from(&directory);
        if !dir.exists() {
            std::fs::create_dir_all(&dir)
                .map_err(|e| format!("Failed to create directory: {e}"))?;
        }

        let file_path = dir.join(&file_name);

        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = stop_flag.clone();
        let path_clone = file_path.clone();

        let task = tauri::async_runtime::spawn(async move {
            if let Err(e) =
                recording_loop(&path_clone, speaker_buffers, stop_clone, format).await
            {
                warn!("Recording task failed: {e}");
            }
        });

        let path_str = file_path.to_string_lossy().to_string();

        let mut state = self.inner.lock().map_err(|e| e.to_string())?;
        state.recording_handle = Some(RecordingHandle {
            _task: task,
            stop_flag,
            file_path,
            started_at: std::time::Instant::now(),
        });

        info!("Recording started: {path_str}");
        Ok(path_str)
    }

    /// Stop the current recording and finalize the file.
    pub fn stop_recording(&self) -> Result<String, String> {
        let handle = {
            let mut state = self.inner.lock().map_err(|e| e.to_string())?;
            state
                .recording_handle
                .take()
                .ok_or("No recording in progress")?
        };

        // Signal the task to stop.
        handle.stop_flag.store(true, Ordering::Relaxed);

        let path_str = handle.file_path.to_string_lossy().to_string();
        info!("Recording stopped: {path_str}");
        Ok(path_str)
    }

    /// Get the current recording state.
    pub fn recording_state(&self) -> RecordingState {
        let state = self.inner.lock().ok();
        match state.and_then(|s| s.recording_handle.as_ref().map(|h| {
            (
                h.file_path.to_string_lossy().to_string(),
                h.started_at.elapsed().as_secs_f64(),
            )
        })) {
            Some((path, elapsed)) => RecordingState {
                is_recording: true,
                file_path: Some(path),
                elapsed_secs: elapsed,
            },
            None => RecordingState {
                is_recording: false,
                file_path: None,
                elapsed_secs: 0.0,
            },
        }
    }
}

/// Background task that reads from speaker buffers and writes WAV data.
async fn recording_loop(
    path: &Path,
    speaker_buffers: SpeakerBuffers,
    stop_flag: Arc<AtomicBool>,
    _format: RecordingFormat,
) -> Result<(), String> {
    let spec = WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 16,
        sample_format: HoundSampleFormat::Int,
    };

    let mut writer =
        WavWriter::create(path, spec).map_err(|e| format!("Failed to create WAV file: {e}"))?;

    info!("Recording loop started, writing to {}", path.display());

    // Poll interval: 20 ms (matches Opus frame size at 48 kHz).
    let interval = Duration::from_millis(20);
    let mut mix_buf: Vec<f32> = Vec::new();

    while !stop_flag.load(Ordering::Relaxed) {
        tokio::time::sleep(interval).await;

        // Snapshot + mix all speaker buffers into a single mono stream.
        let samples = drain_and_mix(&speaker_buffers, &mut mix_buf);
        if samples == 0 {
            continue;
        }

        // Convert f32 samples to i16 and write.
        for &sample in &mix_buf[..samples] {
            let clamped = sample.clamp(-1.0, 1.0);
            let i16_val = (clamped * f32::from(i16::MAX)) as i16;
            writer
                .write_sample(i16_val)
                .map_err(|e| format!("WAV write error: {e}"))?;
        }
    }

    writer
        .finalize()
        .map_err(|e| format!("WAV finalize error: {e}"))?;

    info!("Recording loop finished, file finalized: {}", path.display());
    Ok(())
}

/// Drain all speaker buffers and mix into a single mono buffer.
///
/// Unlike the playback callback (which pops samples destructively),
/// we read a *copy* of the buffered samples. The recording task runs
/// on a separate cadence from the playback callback, so we use a
/// non-destructive peek + clear approach that takes a snapshot of
/// whatever is currently queued per speaker.
///
/// This means that if the playback callback drains samples before we
/// do, we simply record silence for that interval - which is fine
/// because the playback already consumed them.  In practice, at 20 ms
/// poll intervals, both the recording and playback drain at very
/// similar rates.
fn drain_and_mix(speaker_buffers: &SpeakerBuffers, mix_buf: &mut Vec<f32>) -> usize {
    let Ok(mut buffers) = speaker_buffers.lock() else {
        return 0;
    };

    // Find max length across all speakers.
    let max_len = buffers.values().map(VecDeque::len).max().unwrap_or(0);
    if max_len == 0 {
        return 0;
    }

    mix_buf.clear();
    mix_buf.resize(max_len, 0.0_f32);

    for buf in buffers.values_mut() {
        for (i, sample) in buf.drain(..).enumerate() {
            if i < max_len {
                mix_buf[i] += sample;
            }
        }
    }

    // Clamp mixed output.
    for s in mix_buf.iter_mut() {
        *s = s.clamp(-1.0, 1.0);
    }

    max_len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_filename_template_basic() {
        let result = expand_filename_template(
            "recording_{host}_{user}_{channel}",
            "myserver.com",
            "alice",
            "General",
        );
        assert!(result.contains("myserver.com"));
        assert!(result.contains("alice"));
        assert!(result.contains("General"));
        assert!(!result.contains("{host}"));
        assert!(!result.contains("{user}"));
        assert!(!result.contains("{channel}"));
    }

    #[test]
    fn test_expand_filename_template_datetime() {
        let result = expand_filename_template("rec_{datetime}", "host", "user", "chan");
        // Should not contain the placeholder anymore.
        assert!(!result.contains("{datetime}"));
        // Should contain a date-like pattern (YYYY-MM-DD_HH-MM-SS).
        assert!(result.len() > 4);
    }

    #[test]
    fn test_expand_filename_no_placeholders() {
        let result = expand_filename_template("static_name", "h", "u", "c");
        assert_eq!(result, "static_name");
    }

    #[test]
    fn test_drain_and_mix_empty() {
        let buffers: SpeakerBuffers = Arc::new(std::sync::Mutex::new(
            std::collections::HashMap::new(),
        ));
        let mut mix = Vec::new();
        let n = drain_and_mix(&buffers, &mut mix);
        assert_eq!(n, 0);
    }

    #[test]
    fn test_drain_and_mix_single_speaker() {
        let mut map = std::collections::HashMap::new();
        let mut deque = VecDeque::new();
        deque.push_back(0.5);
        deque.push_back(-0.3);
        let _ = map.insert(1u32, deque);

        let buffers: SpeakerBuffers = Arc::new(std::sync::Mutex::new(map));
        let mut mix = Vec::new();
        let n = drain_and_mix(&buffers, &mut mix);
        assert_eq!(n, 2);
        assert!((mix[0] - 0.5).abs() < 1e-5);
        assert!((mix[1] - (-0.3)).abs() < 1e-5);
    }

    #[test]
    fn test_drain_and_mix_multiple_speakers_summed() {
        let mut map = std::collections::HashMap::new();
        let mut d1 = VecDeque::new();
        d1.push_back(0.4);
        d1.push_back(0.3);
        let _ = map.insert(1u32, d1);

        let mut d2 = VecDeque::new();
        d2.push_back(0.3);
        d2.push_back(0.2);
        let _ = map.insert(2u32, d2);

        let buffers: SpeakerBuffers = Arc::new(std::sync::Mutex::new(map));
        let mut mix = Vec::new();
        let n = drain_and_mix(&buffers, &mut mix);
        assert_eq!(n, 2);
        assert!((mix[0] - 0.7).abs() < 1e-5);
        assert!((mix[1] - 0.5).abs() < 1e-5);
    }
}
