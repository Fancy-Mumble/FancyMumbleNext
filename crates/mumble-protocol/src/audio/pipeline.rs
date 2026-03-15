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
            Err(crate::error::Error::InvalidState(_)) => {
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
        if let Some(prev) = self.last_seq {
            if let Some(step) = self.seq_step {
                // Step is known: detect gaps as multiples of the step.
                if step > 0 {
                    let expected = prev + step;
                    if packet.sequence > expected {
                        let gap = ((packet.sequence - expected) / step).min(3);
                        for _ in 0..gap {
                            let _ = self.tick_lost();
                        }
                    }
                }
            } else if packet.sequence > prev {
                // Second packet: learn the step between consecutive
                // sequence numbers (1 for our encoder, 480/960 for
                // the official Mumble client, etc.).
                self.seq_step = Some(packet.sequence - prev);
            }
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
            if !samples.is_empty() {
                let correction = prev_val - samples[0];
                // Only apply if there is a meaningful discontinuity.
                if correction.abs() > 0.002 {
                    let cf_len = CROSSFADE_LEN.min(samples.len());
                    for (i, sample) in samples.iter_mut().take(cf_len).enumerate() {
                        let t = i as f32 / cf_len as f32;
                        // Raised cosine: 1.0 at t=0, 0.0 at t=1.
                        let decay = 0.5 * (1.0 + (std::f32::consts::PI * t).cos());
                        *sample += correction * decay;
                    }
                }
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

impl OutboundPipelineBuilder {
    pub fn new() -> Self {
        Self {
            capture: None,
            filters: FilterChain::new(),
            encoder: None,
        }
    }

    pub fn capture(mut self, capture: Box<dyn AudioCapture>) -> Self {
        self.capture = Some(capture);
        self
    }

    pub fn filter(mut self, filter: Box<dyn crate::audio::filter::AudioFilter>) -> Self {
        self.filters.push(filter);
        self
    }

    pub fn encoder(mut self, encoder: Box<dyn AudioEncoder>) -> Self {
        self.encoder = Some(encoder);
        self
    }

    pub fn build(self) -> OutboundPipeline {
        OutboundPipeline::new(
            self.capture.expect("capture source is required"),
            self.filters,
            self.encoder.expect("encoder is required"),
        )
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

impl InboundPipelineBuilder {
    pub fn new() -> Self {
        Self {
            decoder: None,
            filters: FilterChain::new(),
            playback: None,
        }
    }

    pub fn decoder(mut self, decoder: Box<dyn AudioDecoder>) -> Self {
        self.decoder = Some(decoder);
        self
    }

    pub fn filter(mut self, filter: Box<dyn crate::audio::filter::AudioFilter>) -> Self {
        self.filters.push(filter);
        self
    }

    pub fn playback(mut self, playback: Box<dyn AudioPlayback>) -> Self {
        self.playback = Some(playback);
        self
    }

    pub fn build(self) -> InboundPipeline {
        InboundPipeline::new(
            self.decoder.expect("decoder is required"),
            self.filters,
            self.playback.expect("playback sink is required"),
        )
    }
}

impl Default for InboundPipelineBuilder {
    fn default() -> Self {
        Self::new()
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
            .build();

        pipeline.start()?;
        // SilentCapture produces all-zero frames. Without a noise gate
        // the `is_silent` flag stays false, so this should yield Audio.
        let result = pipeline.tick()?;
        assert!(matches!(result, OutboundTick::Audio(_)));
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
            .build();

        pipeline.start()?;
        pipeline.tick(&packet)?;
        Ok(())
    }
}
