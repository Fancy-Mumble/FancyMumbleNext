//! Outbound and inbound audio pipelines.
//!
//! Each pipeline is a linear composition of the independently swappable
//! stages defined elsewhere in the `audio` module:
//!
//! **Outbound** (capture -> network):
//!   `AudioCapture -> FilterChain -> AudioEncoder -> EncodedPacket`
//!
//! **Inbound** (network -> playback):
//!   `EncodedPacket -> AudioDecoder -> FilterChain -> AudioPlayback`
//!
//! Pipelines do **not** own async tasks - they expose simple `tick`
//! methods that the caller (e.g. the client event loop) drives at the
//! appropriate cadence.

use crate::audio::capture::AudioCapture;
use crate::audio::decoder::AudioDecoder;
use crate::audio::encoder::{AudioEncoder, EncodedPacket};
use crate::audio::filter::FilterChain;
use crate::audio::playback::AudioPlayback;
use crate::audio::sample::SampleFormat;
use crate::error::Result;

/// Number of samples to crossfade at frame boundaries to smooth
/// discontinuities between decoded frames.  0.5 ms at 48 kHz.
const CROSSFADE_LEN: usize = 24;

// -----------------------------------------------------------------------
//  Outbound pipeline
// -----------------------------------------------------------------------

/// Result of a single outbound pipeline tick.
///
/// The audio loop uses this to decide whether to send a packet, send a
/// terminator, or skip.
#[derive(Debug)]
pub enum OutboundTick {
    /// Encoded speech frame ready to send (`is_terminator = false`).
    Audio(EncodedPacket),
    /// Final encoded frame - speech just ended (`is_terminator = true`).
    /// After this, the caller should stop sending until `Audio` appears.
    Terminator(EncodedPacket),
    /// Frame was captured but silenced by the noise gate - keep draining
    /// the capture buffer but do **not** send anything.
    Silence,
    /// Capture buffer is empty - no data available yet.
    NoData,
}

/// Drives the microphone -> network direction.
///
/// ```text
/// capture.read_frame()
///     |
///     v
/// filter_chain.process()   (noise gate -> AGC -> denoiser -> ...)
///     |
///     v
/// encoder.encode()  -> EncodedPacket
/// ```
pub struct OutboundPipeline {
    capture: Box<dyn AudioCapture>,
    filters: FilterChain,
    encoder: Box<dyn AudioEncoder>,
    /// Tracks whether we were sending speech on the previous tick,
    /// so we can emit a terminator when speech ends.
    was_talking: bool,
}

impl std::fmt::Debug for OutboundPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutboundPipeline")
            .field("filters", &self.filters)
            .field("was_talking", &self.was_talking)
            .finish_non_exhaustive()
    }
}

impl OutboundPipeline {
    /// Build a new outbound pipeline from its three stages.
    pub fn new(
        capture: Box<dyn AudioCapture>,
        filters: FilterChain,
        encoder: Box<dyn AudioEncoder>,
    ) -> Self {
        Self {
            capture,
            filters,
            encoder,
            was_talking: false,
        }
    }

    /// Read one frame from capture, process it, and encode it.
    ///
    /// Returns an [`OutboundTick`] variant telling the caller exactly
    /// what to do:
    ///
    /// - `Audio` - send the packet with `is_terminator = false`.
    /// - `Terminator` - send the packet with `is_terminator = true`,
    ///   then stop sending until the next `Audio`.
    /// - `Silence` - frame was silenced; keep draining but don't send.
    /// - `NoData` - capture buffer empty; stop draining for this tick.
    pub fn tick(&mut self) -> Result<OutboundTick> {
        let frame = match self.capture.read_frame() {
            Ok(f) => f,
            Err(crate::error::Error::NotEnoughSamples) => {
                // Expected: not enough samples yet - non-blocking.
                return Ok(OutboundTick::NoData);
            }
            Err(e) => {
                // Genuine device failure - propagate so caller can log.
                return Err(e);
            }
        };

        let mut frame = frame;
        self.filters.process(&mut frame)?;

        if frame.is_silent {
            if self.was_talking {
                // Speech just ended - encode one last (silent) frame as
                // a terminator so the server/receiver know we stopped.
                self.was_talking = false;
                let packet = self.encoder.encode(&frame)?;
                Ok(OutboundTick::Terminator(packet))
            } else {
                // Still silent - don't waste bandwidth.
                Ok(OutboundTick::Silence)
            }
        } else {
            // NOTE: we intentionally do NOT reset the encoder here.
            // The previous code called self.encoder.reset() which creates
            // an entirely new Opus encoder instance (zeroing sequence
            // numbers and prediction state).  That hard reset causes an
            // audible pop/click at every silence-to-speech transition
            // because the decoder sees a discontinuity.  Opus handles
            // silence-to-speech transitions gracefully on its own via
            // its internal state management.
            self.was_talking = true;
            let packet = self.encoder.encode(&frame)?;
            Ok(OutboundTick::Audio(packet))
        }
    }

    /// Start the capture source.
    pub fn start(&mut self) -> Result<()> {
        self.capture.start()
    }

    /// Stop the capture source.
    pub fn stop(&mut self) -> Result<()> {
        self.capture.stop()
    }

    /// Reset all internal state (filters + encoder).
    pub fn reset(&mut self) {
        self.filters.reset();
        self.encoder.reset();
        self.was_talking = false;
    }

    /// Mutable access to the filter chain (enable/disable at runtime).
    pub fn filters_mut(&mut self) -> &mut FilterChain {
        &mut self.filters
    }
}

// -----------------------------------------------------------------------
//  Inbound pipeline
// -----------------------------------------------------------------------

/// Drives the network -> speaker direction.
///
/// Tracks sequence numbers to detect packet gaps and invoke Opus PLC
/// (packet-loss concealment) for missing frames, following the same
/// approach as the official Mumble C++ client's `AudioOutputSpeech`.
///
/// ```text
/// EncodedPacket
///     |
///     v
/// [gap detection - PLC for missing frames]
///     |
///     v
/// decoder.decode()
///     |
///     v
/// filter_chain.process()   (volume -> ...)
///     |
///     v
/// playback.write_frame()
/// ```
pub struct InboundPipeline {
    decoder: Box<dyn AudioDecoder>,
    filters: FilterChain,
    playback: Box<dyn AudioPlayback>,
    /// Last sequence number successfully decoded, used for gap detection.
    last_seq: Option<u64>,
    /// Auto-detected step between consecutive sequence numbers.
    /// Different Mumble clients use different numbering: some increment
    /// by 1 per frame, others by `frame_size` (480, 960, etc.).
    /// `None` until two consecutive packets establish the pattern.
    seq_step: Option<u64>,
    /// Last sample value from the end of the previous decoded frame,
    /// used to apply a short correction ramp at frame boundaries.
    prev_last_sample: Option<f32>,
}

impl std::fmt::Debug for InboundPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InboundPipeline")
            .field("filters", &self.filters)
            .field("last_seq", &self.last_seq)
            .field("seq_step", &self.seq_step)
            .finish_non_exhaustive()
    }
}

impl InboundPipeline {
    /// Build a new inbound pipeline from its three stages.
    pub fn new(
        decoder: Box<dyn AudioDecoder>,
        filters: FilterChain,
        playback: Box<dyn AudioPlayback>,
    ) -> Self {
        Self {
            decoder,
            filters,
            playback,
            last_seq: None,
            seq_step: None,
            prev_last_sample: None,
        }
    }

    /// Decode a received packet, filter it, and send it to playback.
    ///
    /// Automatically detects sequence gaps and generates PLC
    /// (packet-loss concealment) frames for each missing packet before
    /// decoding the current one.  Caps PLC to 3 frames to avoid
    /// filling the buffer with concealment data during long gaps (e.g.
    /// silence between speech turns).
    ///
    /// The gap detection auto-learns the sequence step from the first
    /// two packets so it works regardless of whether the sender
    /// increments by 1 (our encoder) or by `frame_size` (official Mumble
    /// C++ client).  Crossfade correction is applied at every frame
    /// boundary to smooth discontinuities.
    pub fn tick(&mut self, packet: &EncodedPacket) -> Result<()> {
        // Detect and conceal gaps in the sequence.
        let gap = detect_sequence_gap(self.last_seq, self.seq_step, packet.sequence, &mut self.seq_step);
        for _ in 0..gap.unwrap_or(0) {
            let _ = self.tick_lost();
        }
        self.last_seq = Some(packet.sequence);

        let mut frame = self.decoder.decode(packet)?;
        self.filters.process(&mut frame)?;
        self.apply_boundary_crossfade(&mut frame);
        self.playback.write_frame(&frame)
    }

    /// Handle a missing packet (packet-loss concealment).
    pub fn tick_lost(&mut self) -> Result<()> {
        let mut frame = self.decoder.decode_lost()?;
        self.filters.process(&mut frame)?;
        self.apply_boundary_crossfade(&mut frame);
        self.playback.write_frame(&frame)
    }

    /// Apply a short correction ramp at the start of each decoded frame
    /// to smooth any sample-level discontinuity at the boundary.
    ///
    /// Instead of a full overlap-add (which would duplicate samples),
    /// this adds a decaying correction: at sample 0 the output equals
    /// the previous frame's last sample (continuity), and across
    /// `CROSSFADE_LEN` samples the correction fades to zero via a
    /// raised-cosine window.  This removes clicks without smearing
    /// the signal.
    fn apply_boundary_crossfade(&mut self, frame: &mut crate::audio::sample::AudioFrame) {
        if frame.format.sample_format != SampleFormat::F32 {
            return;
        }

        if let Some(prev_val) = self.prev_last_sample {
            let samples = frame.as_f32_samples_mut();
            let correction = match samples.first() {
                Some(&first) if (prev_val - first).abs() > 0.002 => prev_val - first,
                _ => 0.0,
            };
            if correction != 0.0 {
                let cf_len = CROSSFADE_LEN.min(samples.len());
                apply_crossfade_correction(samples, correction, cf_len);
            }
        }

        // Store the last sample for the next boundary.
        let samples = frame.as_f32_samples();
        self.prev_last_sample = samples.last().copied();
    }

    /// Start the playback sink.
    pub fn start(&mut self) -> Result<()> {
        self.playback.start()
    }

    /// Stop the playback sink.
    pub fn stop(&mut self) -> Result<()> {
        self.playback.stop()
    }

    /// Reset all internal state (decoder + filters + sequence tracking).
    pub fn reset(&mut self) {
        self.decoder.reset();
        self.filters.reset();
        self.last_seq = None;
        self.seq_step = None;
        self.prev_last_sample = None;
    }

    /// Mutable access to the filter chain.
    pub fn filters_mut(&mut self) -> &mut FilterChain {
        &mut self.filters
    }
}

// -----------------------------------------------------------------------
//  Builder helpers
// -----------------------------------------------------------------------

/// Convenience builder for [`OutboundPipeline`].
pub struct OutboundPipelineBuilder {
    capture: Option<Box<dyn AudioCapture>>,
    filters: FilterChain,
    encoder: Option<Box<dyn AudioEncoder>>,
}

impl std::fmt::Debug for OutboundPipelineBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutboundPipelineBuilder")
            .field("filters", &self.filters)
            .finish_non_exhaustive()
    }
}

impl OutboundPipelineBuilder {
    /// Create a new empty builder.
    pub fn new() -> Self {
        Self {
            capture: None,
            filters: FilterChain::new(),
            encoder: None,
        }
    }

    /// Set the capture source.
    pub fn capture(mut self, capture: Box<dyn AudioCapture>) -> Self {
        self.capture = Some(capture);
        self
    }

    /// Append a filter to the processing chain.
    pub fn filter(mut self, filter: Box<dyn crate::audio::filter::AudioFilter>) -> Self {
        self.filters.push(filter);
        self
    }

    /// Set the encoder.
    pub fn encoder(mut self, encoder: Box<dyn AudioEncoder>) -> Self {
        self.encoder = Some(encoder);
        self
    }

    /// Build the pipeline.
    ///
    /// # Errors
    /// Returns an error if a capture source or encoder has not been set.
    pub fn build(self) -> Result<OutboundPipeline> {
        Ok(OutboundPipeline::new(
            self.capture
                .ok_or_else(|| crate::error::Error::InvalidState("capture source is required".into()))?,
            self.filters,
            self.encoder
                .ok_or_else(|| crate::error::Error::InvalidState("encoder is required".into()))?,
        ))
    }
}

impl Default for OutboundPipelineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience builder for [`InboundPipeline`].
pub struct InboundPipelineBuilder {
    decoder: Option<Box<dyn AudioDecoder>>,
    filters: FilterChain,
    playback: Option<Box<dyn AudioPlayback>>,
}

impl std::fmt::Debug for InboundPipelineBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InboundPipelineBuilder")
            .field("filters", &self.filters)
            .finish_non_exhaustive()
    }
}

impl InboundPipelineBuilder {
    /// Create a new empty builder.
    pub fn new() -> Self {
        Self {
            decoder: None,
            filters: FilterChain::new(),
            playback: None,
        }
    }

    /// Set the decoder.
    pub fn decoder(mut self, decoder: Box<dyn AudioDecoder>) -> Self {
        self.decoder = Some(decoder);
        self
    }

    /// Append a filter to the processing chain.
    pub fn filter(mut self, filter: Box<dyn crate::audio::filter::AudioFilter>) -> Self {
        self.filters.push(filter);
        self
    }

    /// Set the playback sink.
    pub fn playback(mut self, playback: Box<dyn AudioPlayback>) -> Self {
        self.playback = Some(playback);
        self
    }

    /// Build the pipeline.
    ///
    /// # Errors
    /// Returns an error if a decoder or playback sink has not been set.
    pub fn build(self) -> Result<InboundPipeline> {
        Ok(InboundPipeline::new(
            self.decoder
                .ok_or_else(|| crate::error::Error::InvalidState("decoder is required".into()))?,
            self.filters,
            self.playback
                .ok_or_else(|| crate::error::Error::InvalidState("playback sink is required".into()))?,
        ))
    }
}

impl Default for InboundPipelineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

fn detect_sequence_gap(
    last_seq: Option<u64>,
    current_step: Option<u64>,
    incoming_seq: u64,
    seq_step: &mut Option<u64>,
) -> Option<u64> {
    let prev = last_seq?;
    if let Some(step) = current_step {
        if step == 0 {
            return None;
        }
        let expected = prev + step;
        if incoming_seq > expected {
            Some(((incoming_seq - expected) / step).min(3))
        } else {
            None
        }
    } else {
        if incoming_seq > prev {
            *seq_step = Some(incoming_seq - prev);
        }
        None
    }
}

fn apply_crossfade_correction(samples: &mut [f32], correction: f32, cf_len: usize) {
    for (i, sample) in samples.iter_mut().take(cf_len).enumerate() {
        let t = i as f32 / cf_len as f32;
        let decay = 0.5 * (1.0 + (std::f32::consts::PI * t).cos());
        *sample += correction * decay;
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "opus-codec")]
    use super::*;

    #[cfg(feature = "opus-codec")]
    use crate::audio::filter::volume::VolumeFilter;
    #[cfg(feature = "opus-codec")]
    use crate::audio::playback::NullPlayback;
    #[cfg(feature = "opus-codec")]
    use crate::audio::sample::{AudioFormat, AudioFrame};

    #[cfg(feature = "opus-codec")]
    #[test]
    fn outbound_roundtrip() -> Result<()> {
        use crate::audio::capture::SilentCapture;
        use crate::audio::encoder::{OpusEncoder, OpusEncoderConfig};

        let fmt = AudioFormat::MONO_48KHZ_F32;
        let mut pipeline = OutboundPipelineBuilder::new()
            .capture(Box::new(SilentCapture::new(fmt, 480)))
            .filter(Box::new(VolumeFilter::new(1.0)))
            .encoder(Box::new(OpusEncoder::new(OpusEncoderConfig::default(), fmt)?))
            .build()?;

        pipeline.start()?;
        // SilentCapture produces all-zero frames. Without a noise gate
        // the `is_silent` flag stays false, so this should yield Audio.
        let result = pipeline.tick()?;
        assert!(matches!(result, OutboundTick::Audio(_)));
        Ok(())
    }

    /// A test capture source that produces frames with a controllable
    /// amplitude to exercise the noise gate at different levels.
    #[cfg(feature = "opus-codec")]
    struct ToneCapture {
        format: AudioFormat,
        frame_size: usize,
        amplitude: f32,
        sequence: u64,
    }

    #[cfg(feature = "opus-codec")]
    impl ToneCapture {
        fn new(format: AudioFormat, frame_size: usize, amplitude: f32) -> Self {
            Self {
                format,
                frame_size,
                amplitude,
                sequence: 0,
            }
        }
    }

    #[cfg(feature = "opus-codec")]
    impl AudioCapture for ToneCapture {
        fn format(&self) -> AudioFormat {
            self.format
        }
        fn read_frame(&mut self) -> Result<AudioFrame> {
            let n = self.frame_size;
            let mut data = Vec::with_capacity(n * 4);
            for i in 0..n {
                // Simple sine wave at the given amplitude.
                let sample = self.amplitude
                    * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48000.0).sin();
                data.extend_from_slice(&sample.to_ne_bytes());
            }
            self.sequence += 1;
            Ok(AudioFrame {
                data,
                format: self.format,
                sequence: self.sequence,
                is_silent: false,
            })
        }
        fn start(&mut self) -> Result<()> {
            Ok(())
        }
        fn stop(&mut self) -> Result<()> {
            Ok(())
        }
    }

    #[cfg(feature = "opus-codec")]
    #[test]
    fn noise_gate_silences_silent_capture() -> Result<()> {
        use crate::audio::capture::SilentCapture;
        use crate::audio::encoder::{OpusEncoder, OpusEncoderConfig};
        use crate::audio::filter::noise_gate::{NoiseGate, NoiseGateConfig};

        let fmt = AudioFormat::MONO_48KHZ_F32;
        let mut pipeline = OutboundPipelineBuilder::new()
            .capture(Box::new(SilentCapture::new(fmt, 960)))
            .filter(Box::new(NoiseGate::new(NoiseGateConfig {
                open_threshold: 0.01,
                close_threshold: 0.008,
                hold_frames: 5,
                ..NoiseGateConfig::default()
            })))
            .encoder(Box::new(OpusEncoder::new(OpusEncoderConfig::default(), fmt)?))
            .build()?;

        pipeline.start()?;
        // Silent input + noise gate = should be silenced.
        for _ in 0..10 {
            let tick = pipeline.tick()?;
            assert!(
                matches!(tick, OutboundTick::Silence),
                "Silent input should produce Silence ticks, got {tick:?}",
            );
        }
        Ok(())
    }

    #[cfg(feature = "opus-codec")]
    #[test]
    fn noise_gate_at_max_threshold_blocks_all_audio() -> Result<()> {
        use crate::audio::encoder::{OpusEncoder, OpusEncoderConfig};
        use crate::audio::filter::noise_gate::{NoiseGate, NoiseGateConfig};

        let fmt = AudioFormat::MONO_48KHZ_F32;
        // Amplitude 0.3 -> RMS ~0.21 (for a sine wave, RMS = amp / sqrt(2))
        let mut pipeline = OutboundPipelineBuilder::new()
            .capture(Box::new(ToneCapture::new(fmt, 960, 0.3)))
            .filter(Box::new(NoiseGate::new(NoiseGateConfig {
                open_threshold: 1.0, // Maximum threshold - should block everything
                close_threshold: 0.8,
                hold_frames: 5,
                ..NoiseGateConfig::default()
            })))
            .encoder(Box::new(OpusEncoder::new(OpusEncoderConfig::default(), fmt)?))
            .build()?;

        pipeline.start()?;
        // Even with a 0.3 amplitude signal, threshold of 1.0 should block.
        for _ in 0..10 {
            let tick = pipeline.tick()?;
            assert!(
                matches!(tick, OutboundTick::Silence),
                "Max threshold should produce Silence for moderate signal, got {tick:?}",
            );
        }
        Ok(())
    }

    #[cfg(feature = "opus-codec")]
    #[test]
    fn noise_gate_at_low_threshold_passes_audio() -> Result<()> {
        use crate::audio::encoder::{OpusEncoder, OpusEncoderConfig};
        use crate::audio::filter::noise_gate::{NoiseGate, NoiseGateConfig};

        let fmt = AudioFormat::MONO_48KHZ_F32;
        // Amplitude 0.3 -> RMS ~0.21, well above threshold of 0.01.
        let mut pipeline = OutboundPipelineBuilder::new()
            .capture(Box::new(ToneCapture::new(fmt, 960, 0.3)))
            .filter(Box::new(NoiseGate::new(NoiseGateConfig {
                open_threshold: 0.01,
                close_threshold: 0.008,
                hold_frames: 5,
                ..NoiseGateConfig::default()
            })))
            .encoder(Box::new(OpusEncoder::new(OpusEncoderConfig::default(), fmt)?))
            .build()?;

        pipeline.start()?;
        // A 0.3 amplitude signal should pass through at threshold 0.01.
        let tick = pipeline.tick()?;
        assert!(
            matches!(tick, OutboundTick::Audio(_)),
            "Low threshold should produce Audio for moderate signal, got {tick:?}",
        );
        Ok(())
    }

    #[cfg(feature = "opus-codec")]
    #[test]
    fn inbound_roundtrip() -> Result<()> {
        use crate::audio::decoder::OpusDecoder;
        use crate::audio::encoder::{OpusEncoder, OpusEncoderConfig};

        let fmt = AudioFormat::MONO_48KHZ_F32;

        // Produce a real Opus packet via the encoder so the decoder
        // receives valid compressed data.
        let config = OpusEncoderConfig::default();
        let frame_size = config.frame_size;
        let mut encoder = OpusEncoder::new(config, fmt)?;
        let silent_frame = AudioFrame {
            data: vec![0u8; frame_size * fmt.channels as usize * fmt.sample_format.byte_width()],
            format: fmt,
            sequence: 0,
            is_silent: false,
        };
        let packet = encoder.encode(&silent_frame)?;

        let mut pipeline = InboundPipelineBuilder::new()
            .decoder(Box::new(OpusDecoder::new(fmt)?))
            .filter(Box::new(VolumeFilter::new(1.0)))
            .playback(Box::new(NullPlayback::new(fmt)))
            .build()?;

        pipeline.start()?;
        pipeline.tick(&packet)?;
        Ok(())
    }
}
