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

/// Shared per-speaker sample buffers.
///
/// The mixer writes decoded samples per session, and the platform
/// playback callback reads + mixes them in real time.
pub type SpeakerBuffers = Arc<Mutex<HashMap<u32, VecDeque<f32>>>>;

/// Per-speaker decoder state.
struct SpeakerDecoder {
    decoder: Box<dyn AudioDecoder>,
    last_seq: Option<u64>,
    seq_step: Option<u64>,
    prev_last_sample: Option<f32>,
    last_activity: Instant,
}

impl SpeakerDecoder {
    fn new(format: AudioFormat) -> Result<Self> {
        let decoder = OpusDecoder::new(format)?;
        Ok(Self {
            decoder: Box::new(decoder),
            last_seq: None,
            seq_step: None,
            prev_last_sample: None,
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
        let speaker = match self.speakers.entry(session) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(SpeakerDecoder::new(self.format)?)
            }
        };
        speaker.last_activity = Instant::now();

        // Detect and conceal gaps in the sequence (same logic as
        // InboundPipeline but per-speaker).
        let gap_plc = if let Some(prev) = speaker.last_seq {
            if let Some(step) = speaker.seq_step {
                if step > 0 {
                    let expected = prev + step;
                    if packet.sequence > expected {
                        let gap = ((packet.sequence - expected) / step).min(3);
                        Some(gap)
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                if packet.sequence > prev {
                    speaker.seq_step = Some(packet.sequence - prev);
                }
                None
            }
        } else {
            None
        };

        // Generate PLC frames for detected gaps, then decode the
        // current packet. This two-phase approach avoids a mutable
        // re-borrow after calling feed_lost.
        if let Some(gap) = gap_plc {
            for _ in 0..gap {
                self.feed_lost(session)?;
            }
            let speaker = self
                .speakers
                .get_mut(&session)
                .ok_or_else(|| crate::error::Error::InvalidState("speaker removed during PLC".into()))?;
            speaker.last_seq = Some(packet.sequence);
            let mut frame = speaker.decoder.decode(packet)?;
            apply_boundary_crossfade(
                &mut frame,
                &mut speaker.prev_last_sample,
            );
            push_samples(&self.buffers, session, &frame);
            return Ok(());
        }

        // No gap: borrow is still live from the entry above.
        let speaker = self
            .speakers
            .get_mut(&session)
            .ok_or_else(|| crate::error::Error::InvalidState("speaker removed unexpectedly".into()))?;
        speaker.last_seq = Some(packet.sequence);

        let mut frame = speaker.decoder.decode(packet)?;
        apply_boundary_crossfade(&mut frame, &mut speaker.prev_last_sample);
        push_samples(&self.buffers, session, &frame);
        Ok(())
    }

    /// Generate a PLC (packet-loss concealment) frame for `session`.
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
fn push_samples(
    buffers: &SpeakerBuffers,
    session: u32,
    frame: &crate::audio::sample::AudioFrame,
) {
    let samples = frame.as_f32_samples();
    if let Ok(mut bufs) = buffers.lock() {
        let buf = bufs
            .entry(session)
            .or_insert_with(|| VecDeque::with_capacity(96_000));
        buf.extend(samples.iter().copied());
        // Cap per-speaker to ~2 seconds at 48 kHz mono.
        if buf.len() > 96_000 {
            let excess = buf.len() - 96_000;
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
    if frame.format.sample_format != SampleFormat::F32 {
        return;
    }

    if let Some(prev_val) = *prev_last_sample {
        let samples = frame.as_f32_samples_mut();
        if !samples.is_empty() {
            let correction = prev_val - samples[0];
            if correction.abs() > 0.002 {
                let cf_len = CROSSFADE_LEN.min(samples.len());
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
    fn gap_in_sequence_triggers_plc() {
        // Feeding packets with a sequence gap should produce *more*
        // samples than two contiguous packets because PLC frames are
        // generated for the missing packets.
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

        // Packet 1: sequence 0
        let pkt1 = enc.encode(&silent).unwrap();
        mixer.feed(1, &pkt1).unwrap();
        let after_first = bufs.lock().unwrap()[&1].len();

        // Packet 2: sequence 960 (normal step)
        let pkt2 = EncodedPacket {
            data: pkt1.data.clone(),
            sequence: 960,
            frame_samples: pkt1.frame_samples,
        };
        mixer.feed(1, &pkt2).unwrap();
        let after_second = bufs.lock().unwrap()[&1].len();
        let contiguous_added = after_second - after_first;

        // Packet 3: sequence 2880 (skip one frame at sequence 1920)
        let pkt3 = EncodedPacket {
            data: pkt1.data.clone(),
            sequence: 2880,
            frame_samples: pkt1.frame_samples,
        };
        mixer.feed(1, &pkt3).unwrap();
        let after_gap = bufs.lock().unwrap()[&1].len();
        let gap_added = after_gap - after_second;

        // Gap should produce more samples (concealment frame + normal frame).
        assert!(
            gap_added > contiguous_added,
            "Expected PLC to produce extra samples: gap_added={gap_added}, contiguous_added={contiguous_added}"
        );
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
}
