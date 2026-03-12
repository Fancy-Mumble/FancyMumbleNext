//! Outbound and inbound audio pipelines.
//!
//! Each pipeline is a linear composition of the independently swappable
//! stages defined elsewhere in the `audio` module:
//!
//! **Outbound** (capture → network):
//!   `AudioCapture → FilterChain → AudioEncoder → EncodedPacket`
//!
//! **Inbound** (network → playback):
//!   `EncodedPacket → AudioDecoder → FilterChain → AudioPlayback`
//!
//! Pipelines do **not** own async tasks - they expose simple `tick`
//! methods that the caller (e.g. the client event loop) drives at the
//! appropriate cadence.

use crate::audio::capture::AudioCapture;
use crate::audio::decoder::AudioDecoder;
use crate::audio::encoder::{AudioEncoder, EncodedPacket};
use crate::audio::filter::FilterChain;
use crate::audio::playback::AudioPlayback;
use crate::error::Result;

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

/// Drives the microphone → network direction.
///
/// ```text
/// capture.read_frame()
///     │
///     ▼
/// filter_chain.process()   (noise gate → AGC → denoiser → …)
///     │
///     ▼
/// encoder.encode()  → EncodedPacket
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
            if !self.was_talking {
                // Starting a new speech segment - reset encoder to avoid
                // prediction artefacts from stale state.
                self.encoder.reset();
            }
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

/// Drives the network → speaker direction.
///
/// ```text
/// EncodedPacket
///     │
///     ▼
/// decoder.decode()
///     │
///     ▼
/// filter_chain.process()   (volume → …)
///     │
///     ▼
/// playback.write_frame()
/// ```
pub struct InboundPipeline {
    decoder: Box<dyn AudioDecoder>,
    filters: FilterChain,
    playback: Box<dyn AudioPlayback>,
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
        }
    }

    /// Decode a received packet, filter it, and send it to playback.
    pub fn tick(&mut self, packet: &EncodedPacket) -> Result<()> {
        let mut frame = self.decoder.decode(packet)?;
        self.filters.process(&mut frame)?;
        self.playback.write_frame(&frame)
    }

    /// Handle a missing packet (packet-loss concealment).
    pub fn tick_lost(&mut self) -> Result<()> {
        let mut frame = self.decoder.decode_lost()?;
        self.filters.process(&mut frame)?;
        self.playback.write_frame(&frame)
    }

    /// Start the playback sink.
    pub fn start(&mut self) -> Result<()> {
        self.playback.start()
    }

    /// Stop the playback sink.
    pub fn stop(&mut self) -> Result<()> {
        self.playback.stop()
    }

    /// Reset all internal state (decoder + filters).
    pub fn reset(&mut self) {
        self.decoder.reset();
        self.filters.reset();
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
