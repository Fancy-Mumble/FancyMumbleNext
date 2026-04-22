//! Per-speaker audio mixer.
//!
//! Manages one [`AudioDecoder`] per remote speaker (keyed by session
//! ID) so that each Opus stream is decoded independently.  Decoded
//! samples are written into per-speaker ring buffers that the
//! platform playback callback reads, sums, and outputs.
//!
//! This replaces the single-decoder [`InboundPipeline`] approach
//! which was fundamentally broken for multi-speaker scenarios because
//! Opus is a stateful codec.
//!
//! [`InboundPipeline`]: super::pipeline::InboundPipeline

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::audio::decoder::{AudioDecoder, OpusDecoder};
use crate::audio::encoder::EncodedPacket;
use crate::audio::sample::{AudioFormat, SampleFormat};
use crate::error::Result;

/// Number of samples to crossfade at frame boundaries to smooth
/// discontinuities between decoded frames.  0.5 ms at 48 kHz.
const CROSSFADE_LEN: usize = 24;

/// Speakers that have not sent audio for this many seconds are
/// removed to free resources.
const SPEAKER_TIMEOUT_SECS: u64 = 30;

/// Maximum per-speaker sample buffer size.  Capped at 400 ms
/// (19 200 samples at 48 kHz mono) to prevent buffer bloat when
/// the playback callback falls behind (e.g. Android app
/// backgrounded).  Old samples are dropped from the front.
const MAX_SPEAKER_BUFFER_SAMPLES: usize = 19_200;

/// Shared per-speaker sample buffers.
///
/// The mixer writes decoded samples per session, and the platform
/// playback callback reads + mixes them in real time.
pub type SpeakerBuffers = Arc<Mutex<HashMap<u32, VecDeque<f32>>>>;

/// Shared per-speaker volume overrides (0.0 - 2.0, default 1.0).
///
/// Set from the UI when the user adjusts a specific speaker's volume
/// slider.  The playback callback reads these values during mixing.
pub type SpeakerVolumes = Arc<Mutex<HashMap<u32, f32>>>;

/// Number of samples per 10 ms at 48 kHz - the unit of Mumble's
/// `frame_number` field.  A packet that decodes to N samples consumes
/// `N / SAMPLES_PER_SEQ_UNIT` sequence units.
const SAMPLES_PER_SEQ_UNIT: u64 = 480;

/// Per-speaker decoder state.
struct SpeakerDecoder {
    decoder: Box<dyn AudioDecoder>,
    last_seq: Option<u64>,
    /// Sequence number we expect the next packet from this speaker to
    /// carry, computed as `packet.sequence + decoded_samples / 480`
    /// after every successful decode.  Used for sample-accurate gap
    /// detection that works regardless of how many Opus frames the
    /// sender packs into each network packet.
    expected_next_seq: Option<u64>,
    prev_last_sample: Option<f32>,
    /// Set to true when the decoder is fresh (just created or reset)
    /// and the very next decoded frame must be faded in from silence.
    /// Without this fade, the first frame's first sample can start at
    /// near-full amplitude (Opus has no warm-up lookahead), producing
    /// an audible click/pop at the start of every utterance and after
    /// every stream restart.
    needs_fade_in: bool,
    last_activity: Instant,
}

impl SpeakerDecoder {
    fn new(format: AudioFormat) -> Result<Self> {
        let decoder = OpusDecoder::new(format)?;
        Ok(Self {
            decoder: Box::new(decoder),
            last_seq: None,
            expected_next_seq: None,
            prev_last_sample: None,
            needs_fade_in: true,
            last_activity: Instant::now(),
        })
    }
}

/// Manages per-speaker audio decoders and writes decoded PCM into
/// shared per-speaker buffers that the platform playback callback
/// reads and mixes.
pub struct AudioMixer {
    speakers: HashMap<u32, SpeakerDecoder>,
    buffers: SpeakerBuffers,
    format: AudioFormat,
}

impl std::fmt::Debug for AudioMixer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioMixer")
            .field("active_speakers", &self.speakers.len())
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

impl AudioMixer {
    /// Create a new mixer that writes decoded audio into `buffers`.
    pub fn new(buffers: SpeakerBuffers, format: AudioFormat) -> Self {
        Self {
            speakers: HashMap::new(),
            buffers,
            format,
        }
    }

    /// Return a clone of the shared speaker buffers handle.
    pub fn buffers(&self) -> SpeakerBuffers {
        self.buffers.clone()
    }

    /// Decode an incoming audio packet from `session` and queue the
    /// decoded samples in the corresponding speaker buffer.
    pub fn feed(&mut self, session: u32, packet: &EncodedPacket) -> Result<()> {
        // Detect stream restart: if the incoming sequence is much lower
        // than the last seen, the sender started a new voice stream.
        // Drop the stale decoder so Opus state from the old stream does
        // not contaminate the new one (handles lost terminators).
        if let Some(speaker) = self.speakers.get(&session) {
            if let Some(prev) = speaker.last_seq {
                if prev > packet.sequence && prev - packet.sequence > 10 {
                    tracing::debug!(
                        "stream restart detected: session {session} seq {prev} -> {}, resetting decoder",
                        packet.sequence,
                    );
                    drop(self.speakers.remove(&session));
                }
            }
        }

        let speaker = match self.speakers.entry(session) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(SpeakerDecoder::new(self.format)?)
            }
        };
        speaker.last_activity = Instant::now();

        // Conservative gap handling: only insert silence padding when
        // we can be CERTAIN packets were lost.  We compute the expected
        // next seq from the previous packet's decoded sample count
        // (in 10 ms units, the protocol's sequence unit).  Anything
        // beyond a generous tolerance is treated as real loss; small
        // discrepancies (jitter, frames-per-packet variation) are
        // ignored - libopus's internal state handles those gracefully
        // on the next decode.
        let silence_units = detect_certain_gap(speaker.expected_next_seq, packet.sequence);

        if silence_units > 0 {
            insert_silence(&self.buffers, session, silence_units, self.format);
        }

        let speaker = self
            .speakers
            .get_mut(&session)
            .ok_or_else(|| crate::error::Error::InvalidState("speaker removed during gap fill".into()))?;
        speaker.last_seq = Some(packet.sequence);

        // After a silence padding insertion, the buffer ends in 0.0 -
        // arm the crossfade so the next decoded frame ramps up smoothly
        // from silence rather than jumping in at full amplitude.
        if silence_units > 0 {
            speaker.prev_last_sample = Some(0.0);
        }

        let mut frame = speaker.decoder.decode(packet)?;
        let consumed_units = frame_seq_units(&frame, self.format);
        speaker.expected_next_seq = Some(packet.sequence + consumed_units);

        // Cold-start fade-in: a fresh decoder has no warm-up state,
        // and Opus's first decoded sample can be at full speech
        // amplitude.  Pushing that straight into the buffer creates
        // a step from silence to ~0.9 - audible as a pop at the
        // start of every utterance.  Apply a 5 ms cosine fade-in to
        // the first frame so the buffer ramps smoothly out of
        // silence.  Subsequent frames from the same decoder are
        // continuous (libopus is stateful) and need no further
        // intervention.
        if speaker.needs_fade_in {
            apply_cold_start_fade_in(&mut frame);
            speaker.needs_fade_in = false;
        } else if speaker.prev_last_sample.is_some() {
            // Only apply the boundary crossfade after a real
            // discontinuity event (silence padding).  libopus's
            // stateful decode is naturally continuous between
            // consecutive packets, so applying a crossfade on every
            // frame would distort the first 24 samples of every
            // 10 ms window - audible as a constant 100 Hz buzz
            // riding on top of loud audio.
            apply_boundary_crossfade(&mut frame, &mut speaker.prev_last_sample);
        }
        // For continuous decode, do NOT track prev_last_sample - we
        // want the crossfade dormant until the next discontinuity.
        speaker.prev_last_sample = None;
        push_samples(&self.buffers, session, &frame);
        Ok(())
    }

    /// Generate a PLC (packet-loss concealment) frame for `session`.
    ///
    /// Currently unused by [`feed`] - kept for the recording path and
    /// future jitter-buffer integration.
    #[allow(dead_code, reason = "kept for future jitter-buffer integration and external callers")]
    fn feed_lost(&mut self, session: u32) -> Result<()> {
        let speaker = self
            .speakers
            .get_mut(&session)
            .ok_or_else(|| crate::error::Error::InvalidState("unknown speaker".into()))?;

        let mut frame = speaker.decoder.decode_lost()?;
        apply_boundary_crossfade(&mut frame, &mut speaker.prev_last_sample);
        push_samples(&self.buffers, session, &frame);
        Ok(())
    }

    /// Reset the decoder for a speaker whose audio stream has ended
    /// (e.g. terminator received).  The sample buffer is kept so the
    /// playback callback can drain remaining audio.  A fresh decoder
    /// will be created automatically when the next stream arrives.
    pub fn reset_speaker(&mut self, session: u32) {
        drop(self.speakers.remove(&session));
    }

    /// Remove speakers that have not sent audio recently to free
    /// decoder resources and buffer memory.
    pub fn remove_inactive_speakers(&mut self) {
        let timeout = std::time::Duration::from_secs(SPEAKER_TIMEOUT_SECS);
        let now = Instant::now();
        let stale: Vec<u32> = self
            .speakers
            .iter()
            .filter(|(_, s)| now.duration_since(s.last_activity) > timeout)
            .map(|(&id, _)| id)
            .collect();
        for id in &stale {
            let _ = self.speakers.remove(id);
            if let Ok(mut bufs) = self.buffers.lock() {
                let _ = bufs.remove(id);
            }
        }
    }

    /// Reset all state (all speakers removed).
    pub fn reset(&mut self) {
        self.speakers.clear();
        if let Ok(mut bufs) = self.buffers.lock() {
            bufs.clear();
        }
    }
}

/// Push decoded F32 samples into the shared per-speaker buffer.
/// Detect a *certain* loss gap between the expected next sequence and
/// the incoming packet's sequence.
///
/// Returns the number of 10 ms units of silence to insert before the
/// new packet.  The threshold is intentionally generous so that normal
/// jitter, frames-per-packet variation, and packet reordering do NOT
/// cause spurious gap fills (which were the source of the
/// crackle/click artifacts heard on multi-frame-per-packet senders).
///
/// Capped at [`MAX_SILENCE_FILL_UNITS`] (matches the per-speaker
/// buffer capacity) so that an inserted gap never displaces real
/// decoded audio that has not been played yet.
fn detect_certain_gap(expected: Option<u64>, incoming: u64) -> u64 {
    /// Tolerance in 10 ms units.  Up to this many missing units are
    /// silently absorbed; beyond it we treat the gap as real loss.
    const GAP_TOLERANCE: u64 = 8;

    let Some(expected) = expected else { return 0 };
    if incoming <= expected + GAP_TOLERANCE {
        return 0;
    }
    (incoming - expected).min(MAX_SILENCE_FILL_UNITS)
}

/// Maximum silence-padding insertion in 10 ms units.  Matches the
/// per-speaker buffer cap so that a gap fill cannot displace real
/// already-decoded audio waiting to be played.
const MAX_SILENCE_FILL_UNITS: u64 =
    (MAX_SPEAKER_BUFFER_SAMPLES as u64) / SAMPLES_PER_SEQ_UNIT;

/// Number of 10 ms sequence units the given decoded frame represents.
fn frame_seq_units(frame: &crate::audio::sample::AudioFrame, format: AudioFormat) -> u64 {
    let bytes_per_sample = format.sample_format.byte_width().max(1) as u64;
    let channels = format.channels.max(1) as u64;
    let total_samples = frame.data.len() as u64 / bytes_per_sample / channels;
    (total_samples / SAMPLES_PER_SEQ_UNIT).max(1)
}

/// Append `units * 10 ms` of silence to the speaker buffer to keep
/// real-time alignment after a confirmed packet-loss gap.
///
/// Inserts at most [`MAX_SPEAKER_BUFFER_SAMPLES`] minus the current
/// buffer length so that the cap-eviction at the end of `push_*`
/// helpers never has to discard already-decoded real audio that has
/// not been played yet.  Discarding real audio in favour of silence
/// caused 100 - 400 ms perceptible dropouts every time a moderate
/// gap was detected, sustained underrun in the playback mixer, and
/// repeated re-prime cycles in the rodio source.
fn insert_silence(
    buffers: &SpeakerBuffers,
    session: u32,
    units: u64,
    format: AudioFormat,
) {
    let requested = (units as usize) * (SAMPLES_PER_SEQ_UNIT as usize) * format.channels as usize;
    if requested == 0 {
        return;
    }
    if let Ok(mut bufs) = buffers.lock() {
        let buf = bufs
            .entry(session)
            .or_insert_with(|| VecDeque::with_capacity(MAX_SPEAKER_BUFFER_SAMPLES));
        let remaining = MAX_SPEAKER_BUFFER_SAMPLES.saturating_sub(buf.len());
        let to_insert = requested.min(remaining);
        for _ in 0..to_insert {
            buf.push_back(0.0);
        }
        if to_insert < requested {
            tracing::debug!(
                "insert_silence: clamped {requested} samples to {to_insert} for session {session} (buffer near cap, refusing to evict real audio)"
            );
        }
    }
}

fn push_samples(
    buffers: &SpeakerBuffers,
    session: u32,
    frame: &crate::audio::sample::AudioFrame,
) {
    let samples = frame.as_f32_samples();
    if let Ok(mut bufs) = buffers.lock() {
        let buf = bufs
            .entry(session)
            .or_insert_with(|| VecDeque::with_capacity(MAX_SPEAKER_BUFFER_SAMPLES));
        buf.extend(samples.iter().copied());
        // Drop oldest samples when the buffer exceeds the cap so
        // stale audio never accumulates beyond ~400 ms.  This is the
        // last-resort overflow behaviour for live decoded audio
        // arriving faster than the playback can drain it (e.g. on
        // Android when the app is backgrounded); it should not happen
        // in steady state on desktop.
        if buf.len() > MAX_SPEAKER_BUFFER_SAMPLES {
            let excess = buf.len() - MAX_SPEAKER_BUFFER_SAMPLES;
            tracing::debug!(
                "push_samples: dropped {excess} oldest samples for session {session} (buffer overflow, playback falling behind)"
            );
            let _ = buf.drain(..excess);
        }
    }
}

/// Apply a short correction ramp at the start of a decoded frame to
/// smooth sample-level discontinuities at the boundary (same algorithm
/// as `InboundPipeline::apply_boundary_crossfade`).
fn apply_boundary_crossfade(
    frame: &mut crate::audio::sample::AudioFrame,
    prev_last_sample: &mut Option<f32>,
) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static FRAME_COUNT: AtomicU64 = AtomicU64::new(0);
    static CORRECTED_COUNT: AtomicU64 = AtomicU64::new(0);

    if frame.format.sample_format != SampleFormat::F32 {
        return;
    }

    let count = FRAME_COUNT.fetch_add(1, Ordering::Relaxed) + 1;

    if let Some(prev_val) = *prev_last_sample {
        let samples = frame.as_f32_samples_mut();
        if !samples.is_empty() {
            let correction = prev_val - samples[0];
            if correction.abs() > 0.002 {
                let corrected = CORRECTED_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
                let cf_len = CROSSFADE_LEN.min(samples.len());
                if count.is_multiple_of(100) {
                    tracing::debug!(
                        "crossfade: frame={count}, corrected={corrected}/{count} ({:.0}%), delta={correction:.4}, cf_len={cf_len}",
                        corrected as f64 / count as f64 * 100.0,
                    );
                }
                for (i, sample) in samples.iter_mut().take(cf_len).enumerate() {
                    let t = i as f32 / cf_len as f32;
                    let decay = 0.5 * (1.0 + (std::f32::consts::PI * t).cos());
                    *sample += correction * decay;
                }
            }
        }
    }

    let samples = frame.as_f32_samples();
    *prev_last_sample = samples.last().copied();
}

/// Apply a cosine fade-in to the start of a frame produced by a fresh
/// decoder.  Opus has no warm-up lookahead, so the very first decoded
/// sample after creating a new decoder can be at full speech amplitude
/// (e.g. ~0.9).  Pushing that straight into the speaker buffer creates
/// a step from silence to ~0.9 - audible as a pop at the start of every
/// utterance and after every stream restart.  A 5 ms cosine fade-in is
/// short enough to be inaudible to the listener (1/4 of a phoneme) but
/// long enough to remove the broadband click.
fn apply_cold_start_fade_in(frame: &mut crate::audio::sample::AudioFrame) {
    if frame.format.sample_format != SampleFormat::F32 {
        return;
    }
    /// 5 ms at 48 kHz - short enough to be inaudible perceptually
    /// but long enough to spread the spectral energy of the onset
    /// below the click range.
    const FADE_LEN: usize = 240;

    let samples = frame.as_f32_samples_mut();
    let n = FADE_LEN.min(samples.len());
    for (i, sample) in samples.iter_mut().take(n).enumerate() {
        let t = i as f32 / n as f32;
        // Equal-power cosine fade: 0.5 - 0.5*cos(pi*t) goes 0 -> 1
        // with zero derivative at both endpoints.
        let w = 0.5 - 0.5 * (std::f32::consts::PI * t).cos();
        *sample *= w;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
    use super::*;
    use crate::audio::sample::AudioFormat;

    fn make_buffers() -> SpeakerBuffers {
        Arc::new(Mutex::new(HashMap::new()))
    }

    #[test]
    fn new_mixer_has_no_speakers() {
        let bufs = make_buffers();
        let mixer = AudioMixer::new(bufs.clone(), AudioFormat::MONO_48KHZ_F32);
        assert_eq!(mixer.speakers.len(), 0);
        assert!(bufs.lock().unwrap().is_empty());
    }

    #[cfg(feature = "opus-codec")]
    #[test]
    fn feed_creates_speaker_and_buffers_samples() {
        let bufs = make_buffers();
        let mut mixer = AudioMixer::new(bufs.clone(), AudioFormat::MONO_48KHZ_F32);

        // Encode a silent frame to get valid Opus data.
        use crate::audio::encoder::{AudioEncoder, OpusEncoder, OpusEncoderConfig};
        let config = OpusEncoderConfig::default();
        let frame_size = config.frame_size;
        let mut enc = OpusEncoder::new(config, AudioFormat::MONO_48KHZ_F32).unwrap();
        let silent = crate::audio::sample::AudioFrame {
            data: vec![0u8; frame_size * 4],
            format: AudioFormat::MONO_48KHZ_F32,
            sequence: 0,
            is_silent: false,
        };
        let packet = enc.encode(&silent).unwrap();

        mixer.feed(42, &packet).unwrap();
        assert_eq!(mixer.speakers.len(), 1);
        let locked = bufs.lock().unwrap();
        assert!(locked.contains_key(&42));
        assert!(!locked[&42].is_empty());
    }

    #[cfg(feature = "opus-codec")]
    #[test]
    fn two_speakers_have_independent_buffers() {
        let bufs = make_buffers();
        let mut mixer = AudioMixer::new(bufs.clone(), AudioFormat::MONO_48KHZ_F32);

        use crate::audio::encoder::{AudioEncoder, OpusEncoder, OpusEncoderConfig};
        let config = OpusEncoderConfig::default();
        let frame_size = config.frame_size;
        let mut enc = OpusEncoder::new(config, AudioFormat::MONO_48KHZ_F32).unwrap();
        let silent = crate::audio::sample::AudioFrame {
            data: vec![0u8; frame_size * 4],
            format: AudioFormat::MONO_48KHZ_F32,
            sequence: 0,
            is_silent: false,
        };
        let pkt1 = enc.encode(&silent).unwrap();
        let pkt2 = EncodedPacket {
            data: pkt1.data.clone(),
            sequence: 0,
            frame_samples: pkt1.frame_samples,
        };

        mixer.feed(10, &pkt1).unwrap();
        mixer.feed(20, &pkt2).unwrap();

        assert_eq!(mixer.speakers.len(), 2);
        let locked = bufs.lock().unwrap();
        assert!(locked.contains_key(&10));
        assert!(locked.contains_key(&20));
    }

    #[cfg(feature = "opus-codec")]
    #[test]
    fn reset_clears_everything() {
        let bufs = make_buffers();
        let mut mixer = AudioMixer::new(bufs.clone(), AudioFormat::MONO_48KHZ_F32);

        use crate::audio::encoder::{AudioEncoder, OpusEncoder, OpusEncoderConfig};
        let config = OpusEncoderConfig::default();
        let frame_size = config.frame_size;
        let mut enc = OpusEncoder::new(config, AudioFormat::MONO_48KHZ_F32).unwrap();
        let silent = crate::audio::sample::AudioFrame {
            data: vec![0u8; frame_size * 4],
            format: AudioFormat::MONO_48KHZ_F32,
            sequence: 0,
            is_silent: false,
        };
        let pkt = enc.encode(&silent).unwrap();
        mixer.feed(42, &pkt).unwrap();

        mixer.reset();
        assert_eq!(mixer.speakers.len(), 0);
        assert!(bufs.lock().unwrap().is_empty());
    }

    #[cfg(feature = "opus-codec")]
    #[test]
    fn certain_gap_inserts_silence_padding() {
        // A large, undeniable sequence gap should produce extra samples
        // (silence padding) so the playback timeline stays aligned.
        let bufs = make_buffers();
        let mut mixer = AudioMixer::new(bufs.clone(), AudioFormat::MONO_48KHZ_F32);

        use crate::audio::encoder::{AudioEncoder, OpusEncoder, OpusEncoderConfig};
        let config = OpusEncoderConfig::default();
        let frame_size = config.frame_size;
        let mut enc = OpusEncoder::new(config, AudioFormat::MONO_48KHZ_F32).unwrap();
        let silent = crate::audio::sample::AudioFrame {
            data: vec![0u8; frame_size * 4],
            format: AudioFormat::MONO_48KHZ_F32,
            sequence: 0,
            is_silent: false,
        };

        // 20 ms frames -> seq increments by 2 per packet in protocol units.
        let pkt1 = enc.encode(&silent).unwrap();
        mixer.feed(1, &pkt1).unwrap();
        let after_first = bufs.lock().unwrap()[&1].len();

        // Packet 2: contiguous (seq = 2).
        let pkt2 = EncodedPacket {
            data: pkt1.data.clone(),
            sequence: 2,
            frame_samples: pkt1.frame_samples,
        };
        mixer.feed(1, &pkt2).unwrap();
        let after_second = bufs.lock().unwrap()[&1].len();
        let contiguous_added = after_second - after_first;

        // Packet 3: large gap (seq = 20, expected = 4) - 16 units of loss
        // well above the 8-unit tolerance.
        let pkt3 = EncodedPacket {
            data: pkt1.data.clone(),
            sequence: 20,
            frame_samples: pkt1.frame_samples,
        };
        mixer.feed(1, &pkt3).unwrap();
        let after_gap = bufs.lock().unwrap()[&1].len();
        let gap_added = after_gap - after_second;

        assert!(
            gap_added > contiguous_added,
            "Expected silence padding for a large gap: gap_added={gap_added}, contiguous_added={contiguous_added}"
        );
    }

    #[cfg(feature = "opus-codec")]
    #[test]
    fn multi_frame_per_packet_does_not_inject_silence() {
        // Regression: senders that pack multiple Opus frames per
        // network packet make the sequence number jump by more than 1
        // per packet.  The previous heuristic learned step=1 from the
        // first pair and then injected fake PLC frames at every
        // multi-frame packet, causing audible clicks.  The new
        // sample-accurate detector must absorb this without inserting
        // any silence.
        let bufs = make_buffers();
        let mut mixer = AudioMixer::new(bufs.clone(), AudioFormat::MONO_48KHZ_F32);

        use crate::audio::encoder::{AudioEncoder, OpusEncoder, OpusEncoderConfig};
        let config = OpusEncoderConfig::default();
        let frame_size = config.frame_size;
        let mut enc = OpusEncoder::new(config, AudioFormat::MONO_48KHZ_F32).unwrap();
        let silent = crate::audio::sample::AudioFrame {
            data: vec![0u8; frame_size * 4],
            format: AudioFormat::MONO_48KHZ_F32,
            sequence: 0,
            is_silent: false,
        };
        let template = enc.encode(&silent).unwrap();

        // First packet: seq = 0 (1 packet = 20 ms = 2 protocol units).
        mixer.feed(7, &template).unwrap();
        let after_first = bufs.lock().unwrap()[&7].len();
        // 20 ms decoded = 960 samples.
        assert_eq!(after_first, 960);

        // Subsequent packets: seq advances by 2 per packet (matching
        // the 20 ms frame size).  No silence should ever be inserted.
        let mut prev_len = after_first;
        for i in 1..10_u64 {
            let pkt = EncodedPacket {
                data: template.data.clone(),
                sequence: i * 2,
                frame_samples: template.frame_samples,
            };
            mixer.feed(7, &pkt).unwrap();
            let len = bufs.lock().unwrap()[&7].len();
            let added = len - prev_len;
            assert_eq!(
                added, 960,
                "iteration {i}: each packet must decode to exactly 960 samples \
                 with no silence padding (added={added})"
            );
            prev_len = len;
        }
    }

    #[cfg(feature = "opus-codec")]
    #[test]
    fn continuous_decode_does_not_arm_crossfade() {
        // Regression: applying a boundary crossfade on every successful
        // decode produces a 100 Hz buzz on top of loud audio because
        // the first 24 samples of each 10 ms frame are warped toward
        // the previous frame's last sample.  Continuous decode flow
        // must leave `prev_last_sample` cleared so the crossfade stays
        // dormant until a real discontinuity (silence padding).
        let bufs = make_buffers();
        let mut mixer = AudioMixer::new(bufs.clone(), AudioFormat::MONO_48KHZ_F32);

        use crate::audio::encoder::{AudioEncoder, OpusEncoder, OpusEncoderConfig};
        let config = OpusEncoderConfig::default();
        let frame_size = config.frame_size;
        let mut enc = OpusEncoder::new(config, AudioFormat::MONO_48KHZ_F32).unwrap();
        let silent = crate::audio::sample::AudioFrame {
            data: vec![0u8; frame_size * 4],
            format: AudioFormat::MONO_48KHZ_F32,
            sequence: 0,
            is_silent: false,
        };
        let template = enc.encode(&silent).unwrap();

        for i in 0..5_u64 {
            let pkt = EncodedPacket {
                data: template.data.clone(),
                sequence: i * 2,
                frame_samples: template.frame_samples,
            };
            mixer.feed(11, &pkt).unwrap();
            let speaker = mixer.speakers.get(&11).unwrap();
            assert!(
                speaker.prev_last_sample.is_none(),
                "iteration {i}: continuous decode must leave prev_last_sample = None, \
                 found {:?}", speaker.prev_last_sample
            );
        }
    }

    #[cfg(feature = "opus-codec")]
    #[test]
    fn silence_padding_arms_crossfade_for_next_frame() {
        // After a confirmed gap, silence is appended and the next real
        // decode should be smoothed in (not jumped to full amplitude).
        // Verified indirectly by checking prev_last_sample is set to 0.0
        // after silence insertion.
        let bufs = make_buffers();
        let mut mixer = AudioMixer::new(bufs.clone(), AudioFormat::MONO_48KHZ_F32);

        use crate::audio::encoder::{AudioEncoder, OpusEncoder, OpusEncoderConfig};
        let config = OpusEncoderConfig::default();
        let frame_size = config.frame_size;
        let mut enc = OpusEncoder::new(config, AudioFormat::MONO_48KHZ_F32).unwrap();
        let silent = crate::audio::sample::AudioFrame {
            data: vec![0u8; frame_size * 4],
            format: AudioFormat::MONO_48KHZ_F32,
            sequence: 0,
            is_silent: false,
        };
        let template = enc.encode(&silent).unwrap();

        // Prime: one normal packet (seq=0).
        mixer.feed(13, &template).unwrap();
        assert!(mixer.speakers.get(&13).unwrap().prev_last_sample.is_none());

        // Large gap: seq jumps by 50 protocol units (above tolerance).
        let pkt2 = EncodedPacket {
            data: template.data.clone(),
            sequence: 50,
            frame_samples: template.frame_samples,
        };
        mixer.feed(13, &pkt2).unwrap();

        // After the silence-then-decode, prev_last_sample is cleared
        // again because the decoded frame consumed it via the crossfade.
        assert!(mixer.speakers.get(&13).unwrap().prev_last_sample.is_none());

        // The buffer should contain padding samples from the silence
        // insertion plus the two real frames.
        let len = bufs.lock().unwrap()[&13].len();
        assert!(len > 2 * 960, "expected silence + two frames worth of samples, got {len}");
    }

    #[test]
    fn cold_start_fade_in_attenuates_first_240_samples() {
        // Regression: a fresh decoder's first frame can begin at full
        // speech amplitude (Opus has no warm-up lookahead).  Feeding
        // that straight into the buffer creates a silence -> ~0.9 step,
        // audible as a pop at the start of every utterance.  The
        // cold-start fade-in must attenuate the first 5 ms of the very
        // first frame.
        use crate::audio::sample::AudioFrame;
        let mut frame = AudioFrame {
            data: vec![0u8; 960 * 4],
            format: AudioFormat::MONO_48KHZ_F32,
            sequence: 0,
            is_silent: false,
        };
        // Fill with constant 0.8 amplitude (worst case onset).
        for chunk in frame.data.chunks_exact_mut(4) {
            chunk.copy_from_slice(&0.8_f32.to_le_bytes());
        }

        apply_cold_start_fade_in(&mut frame);

        let samples = frame.as_f32_samples();
        // First sample must be exactly zero (fade starts at w=0).
        assert!(
            samples[0].abs() < 1e-6,
            "first sample after cold-start fade must be 0, got {}", samples[0]
        );
        // Sample at the 240-sample fade endpoint should be near 0.8
        // (cosine fade reaches w=1 at t=1).
        assert!(
            (samples[239] - 0.8).abs() < 0.05,
            "sample at end of fade should be ~0.8, got {}", samples[239]
        );
        // Samples after the fade must be untouched.
        for &s in &samples[240..480] {
            assert!(
                (s - 0.8).abs() < 1e-6,
                "samples past fade window must be unchanged, got {s}"
            );
        }
        // Monotonically non-decreasing through the fade window so we
        // know there is no overshoot or wobble.
        for i in 1..240 {
            assert!(
                samples[i] + 1e-6 >= samples[i - 1],
                "fade must be monotonic non-decreasing at {i}"
            );
        }
    }

    #[cfg(feature = "opus-codec")]
    #[test]
    fn fresh_speaker_decoder_marks_needs_fade_in() {
        // The needs_fade_in flag must be true on creation and false
        // after the first feed, so subsequent frames are continuous
        // and never re-faded in mid-utterance.
        let bufs = make_buffers();
        let mut mixer = AudioMixer::new(bufs.clone(), AudioFormat::MONO_48KHZ_F32);

        use crate::audio::encoder::{AudioEncoder, OpusEncoder, OpusEncoderConfig};
        let config = OpusEncoderConfig::default();
        let frame_size = config.frame_size;
        let mut enc = OpusEncoder::new(config, AudioFormat::MONO_48KHZ_F32).unwrap();
        let silent = crate::audio::sample::AudioFrame {
            data: vec![0u8; frame_size * 4],
            format: AudioFormat::MONO_48KHZ_F32,
            sequence: 0,
            is_silent: false,
        };
        let template = enc.encode(&silent).unwrap();

        mixer.feed(17, &template).unwrap();
        assert!(
            !mixer.speakers.get(&17).unwrap().needs_fade_in,
            "needs_fade_in must be cleared after the first decode"
        );

        // Subsequent feeds keep the flag false.
        for i in 1..3_u64 {
            let pkt = EncodedPacket {
                data: template.data.clone(),
                sequence: i * 2,
                frame_samples: template.frame_samples,
            };
            mixer.feed(17, &pkt).unwrap();
            assert!(
                !mixer.speakers.get(&17).unwrap().needs_fade_in,
                "needs_fade_in must stay false on iteration {i}"
            );
        }
    }

    #[cfg(feature = "opus-codec")]
    #[test]
    fn interleaved_speakers_produce_independent_outputs() {
        // Regression: the old single-decoder design would corrupt
        // decoder state when packets from different speakers were
        // interleaved. This test verifies that interleaving is safe.
        let bufs = make_buffers();
        let mut mixer = AudioMixer::new(bufs.clone(), AudioFormat::MONO_48KHZ_F32);

        use crate::audio::encoder::{AudioEncoder, OpusEncoder, OpusEncoderConfig};
        let config = OpusEncoderConfig::default();
        let frame_size = config.frame_size;
        let mut enc = OpusEncoder::new(config, AudioFormat::MONO_48KHZ_F32).unwrap();
        let silent = crate::audio::sample::AudioFrame {
            data: vec![0u8; frame_size * 4],
            format: AudioFormat::MONO_48KHZ_F32,
            sequence: 0,
            is_silent: false,
        };
        let pkt = enc.encode(&silent).unwrap();

        // Interleave packets from 3 speakers.
        for i in 0..5_u64 {
            let p = EncodedPacket {
                data: pkt.data.clone(),
                sequence: i * 960,
                frame_samples: pkt.frame_samples,
            };
            mixer.feed(100, &p).unwrap();
            mixer.feed(200, &p).unwrap();
            mixer.feed(300, &p).unwrap();
        }

        assert_eq!(mixer.speakers.len(), 3);
        let locked = bufs.lock().unwrap();
        // All three speakers should have the same number of samples
        // since they received the same number of packets.
        let len_100 = locked[&100].len();
        let len_200 = locked[&200].len();
        let len_300 = locked[&300].len();
        assert_eq!(len_100, len_200);
        assert_eq!(len_200, len_300);
        assert!(len_100 > 0);
    }

    #[test]
    fn speaker_buffer_caps_at_max_samples() {
        // Regression: the speaker buffer must not grow beyond
        // MAX_SPEAKER_BUFFER_SAMPLES. Excess old samples are
        // dropped from the front (oldest-first).
        let bufs = make_buffers();
        let count = MAX_SPEAKER_BUFFER_SAMPLES + 5_000;
        let data: Vec<u8> = (0..count)
            .flat_map(|i| (i as f32 * 0.001).to_ne_bytes())
            .collect();
        let frame = crate::audio::sample::AudioFrame {
            data,
            format: AudioFormat::MONO_48KHZ_F32,
            sequence: 0,
            is_silent: false,
        };
        push_samples(&bufs, 1, &frame);

        let locked = bufs.lock().unwrap();
        assert_eq!(
            locked[&1].len(),
            MAX_SPEAKER_BUFFER_SAMPLES,
            "buffer should be capped at MAX_SPEAKER_BUFFER_SAMPLES"
        );
        // The kept samples are the newest; verify the first kept
        // sample corresponds to the expected index.
        let first_kept_idx = count - MAX_SPEAKER_BUFFER_SAMPLES;
        let expected = first_kept_idx as f32 * 0.001;
        let actual = locked[&1][0];
        assert!(
            (actual - expected).abs() < 1e-4,
            "oldest kept sample should be index {first_kept_idx}: expected ~{expected}, got {actual}"
        );
    }

    #[cfg(feature = "opus-codec")]
    #[test]
    fn backward_sequence_jump_resets_decoder() {
        // When the sequence number jumps backwards (new voice stream),
        // the decoder must be reset so stale Opus state does not
        // contaminate the new stream.
        let bufs = make_buffers();
        let mut mixer = AudioMixer::new(bufs.clone(), AudioFormat::MONO_48KHZ_F32);

        use crate::audio::encoder::{AudioEncoder, OpusEncoder, OpusEncoderConfig};
        let config = OpusEncoderConfig::default();
        let frame_size = config.frame_size;
        let mut enc = OpusEncoder::new(config, AudioFormat::MONO_48KHZ_F32).unwrap();
        let silent = crate::audio::sample::AudioFrame {
            data: vec![0u8; frame_size * 4],
            format: AudioFormat::MONO_48KHZ_F32,
            sequence: 0,
            is_silent: false,
        };
        let encoded = enc.encode(&silent).unwrap();

        // Feed packet at seq=100 to establish the speaker.
        let pkt1 = EncodedPacket {
            data: encoded.data.clone(),
            sequence: 100,
            frame_samples: 960,
        };
        mixer.feed(42, &pkt1).unwrap();
        let after_first = bufs.lock().unwrap()[&42].len();

        // Feed packet at seq=0 — large backward jump triggers reset.
        let pkt2 = EncodedPacket {
            data: encoded.data.clone(),
            sequence: 0,
            frame_samples: 960,
        };
        mixer.feed(42, &pkt2).unwrap();

        // Speaker still exists and both frames produced samples.
        assert_eq!(mixer.speakers.len(), 1);
        let total = bufs.lock().unwrap()[&42].len();
        assert!(
            total >= after_first + frame_size,
            "both frames should produce samples: total={total}, after_first={after_first}"
        );
    }

    #[cfg(feature = "opus-codec")]
    #[test]
    fn reset_speaker_clears_decoder_but_keeps_buffer() {
        let bufs = make_buffers();
        let mut mixer = AudioMixer::new(bufs.clone(), AudioFormat::MONO_48KHZ_F32);

        use crate::audio::encoder::{AudioEncoder, OpusEncoder, OpusEncoderConfig};
        let config = OpusEncoderConfig::default();
        let frame_size = config.frame_size;
        let mut enc = OpusEncoder::new(config, AudioFormat::MONO_48KHZ_F32).unwrap();
        let silent = crate::audio::sample::AudioFrame {
            data: vec![0u8; frame_size * 4],
            format: AudioFormat::MONO_48KHZ_F32,
            sequence: 0,
            is_silent: false,
        };
        let pkt = enc.encode(&silent).unwrap();
        mixer.feed(42, &pkt).unwrap();
        assert_eq!(mixer.speakers.len(), 1);

        // Reset simulates a terminator being received.
        mixer.reset_speaker(42);
        assert_eq!(mixer.speakers.len(), 0);

        // Sample buffer is preserved for the playback callback to drain.
        let locked = bufs.lock().unwrap();
        assert!(
            locked.contains_key(&42),
            "sample buffer should survive reset_speaker"
        );
        assert!(
            !locked[&42].is_empty(),
            "previously buffered samples should still be available"
        );
    }

    #[test]
    fn insert_silence_does_not_evict_buffered_real_audio() {
        // Regression: a gap fill (insert_silence) used to push up to
        // 100 * 480 = 48_000 zero samples into a buffer capped at
        // MAX_SPEAKER_BUFFER_SAMPLES (19_200), causing the cap-eviction
        // to discard 28_800 samples of REAL decoded audio that had not
        // yet been played.  This was audible as a 100 - 400 ms dropout
        // every time `detect_certain_gap` fired and was the root cause
        // of sustained underruns + repeated re-prime cycles in the
        // rodio mixer source under network jitter.
        let bufs = make_buffers();

        // Pre-fill with a recognisable real signal at the maximum
        // possible level (sentinel value 1.0) so we can verify it
        // survives the silence insertion.
        let real_samples: Vec<f32> = vec![1.0; MAX_SPEAKER_BUFFER_SAMPLES / 2];
        let frame = crate::audio::sample::AudioFrame {
            data: real_samples.iter().flat_map(|s| s.to_ne_bytes()).collect(),
            format: AudioFormat::MONO_48KHZ_F32,
            sequence: 0,
            is_silent: false,
        };
        push_samples(&bufs, 1, &frame);
        let before_len = bufs.lock().unwrap()[&1].len();
        assert_eq!(before_len, MAX_SPEAKER_BUFFER_SAMPLES / 2);

        // Request a gap fill that, naively inserted, would overflow
        // the buffer by a large margin (100 units = 48_000 samples,
        // current free space is only ~9_600 samples).
        insert_silence(&bufs, 1, 100, AudioFormat::MONO_48KHZ_F32);

        let locked = bufs.lock().unwrap();
        let buf = &locked[&1];
        // Buffer must not exceed the cap.
        assert!(
            buf.len() <= MAX_SPEAKER_BUFFER_SAMPLES,
            "buffer overflowed cap: len={}, cap={MAX_SPEAKER_BUFFER_SAMPLES}",
            buf.len(),
        );
        // The original real samples must still be present at the
        // front of the buffer (they were the oldest, queued for
        // imminent playback).
        let real_count = real_samples.len();
        for (i, &s) in buf.iter().take(real_count).enumerate() {
            assert!(
                (s - 1.0).abs() < f32::EPSILON,
                "real sample {i} was overwritten or evicted: got {s}, expected 1.0",
            );
        }
    }

    #[test]
    fn insert_silence_into_full_buffer_is_a_noop() {
        // When the buffer is already at capacity, inserting silence
        // must not evict any real audio.  Without the cap-aware fix,
        // this call would replace 100 % of the buffer contents with
        // zeros - the worst-case dropout.
        let bufs = make_buffers();
        let real_samples: Vec<f32> = (0..MAX_SPEAKER_BUFFER_SAMPLES)
            .map(|i| (i as f32).sin())
            .collect();
        let frame = crate::audio::sample::AudioFrame {
            data: real_samples.iter().flat_map(|s| s.to_ne_bytes()).collect(),
            format: AudioFormat::MONO_48KHZ_F32,
            sequence: 0,
            is_silent: false,
        };
        push_samples(&bufs, 1, &frame);

        insert_silence(&bufs, 1, 100, AudioFormat::MONO_48KHZ_F32);

        let locked = bufs.lock().unwrap();
        let buf = &locked[&1];
        assert_eq!(buf.len(), MAX_SPEAKER_BUFFER_SAMPLES);
        for (i, &s) in buf.iter().enumerate() {
            let expected = real_samples[i];
            assert!(
                (s - expected).abs() < f32::EPSILON,
                "sample {i} was overwritten by silence: got {s}, expected {expected}",
            );
        }
    }

    #[test]
    fn detect_certain_gap_capped_at_buffer_capacity() {
        // The maximum gap fill must not exceed the buffer capacity in
        // 10 ms units, so that a single gap fill can never displace
        // real already-decoded audio.
        let huge_jump = detect_certain_gap(Some(0), 100_000);
        assert!(
            huge_jump <= MAX_SILENCE_FILL_UNITS,
            "gap fill {huge_jump} exceeds buffer capacity {MAX_SILENCE_FILL_UNITS} units",
        );
        // Ensure samples produced by the cap fit in the buffer.
        let max_samples = (huge_jump as usize) * (SAMPLES_PER_SEQ_UNIT as usize);
        assert!(max_samples <= MAX_SPEAKER_BUFFER_SAMPLES);
    }
}
