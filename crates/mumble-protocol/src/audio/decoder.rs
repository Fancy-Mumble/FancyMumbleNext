//! Audio decoder trait and Opus implementation.
//!
//! An [`AudioDecoder`] takes compressed network packets and produces
//! raw PCM [`AudioFrame`]s for playback. Like the encoder trait, it
//! is codec-agnostic.

use crate::audio::sample::{AudioFormat, AudioFrame};
use crate::audio::encoder::EncodedPacket;
use crate::error::Result;

#[cfg(feature = "opus-codec")]
use crate::error::Error;

/// Decodes compressed audio packets into raw PCM frames.
pub trait AudioDecoder: Send + 'static {
    /// The PCM format produced by this decoder.
    fn output_format(&self) -> AudioFormat;

    /// Decode a single packet into a PCM frame.
    fn decode(&mut self, packet: &EncodedPacket) -> Result<AudioFrame>;

    /// Generate a concealment frame when a packet is lost.
    /// The default implementation returns silence.
    fn decode_lost(&mut self) -> Result<AudioFrame> {
        let format = self.output_format();
        // One 10 ms frame of silence at the decoder's output format.
        let samples = (format.sample_rate / 100) as usize * format.channels as usize;
        let bytes = samples * format.sample_format.byte_width();
        Ok(AudioFrame {
            data: vec![0u8; bytes],
            format,
            sequence: 0,
            is_silent: false,
        })
    }

    /// Reset decoder state (e.g. on stream restart).
    fn reset(&mut self);
}

// ---------------------------------------------------------------------------
//  Real Opus decoder (requires the `opus-codec` feature)
// ---------------------------------------------------------------------------

/// Opus decoder wrapping [`opus::Decoder`].
///
/// Produces 48 kHz mono or stereo F32 PCM frames from
/// Opus-compressed packets received over the network.
#[cfg(feature = "opus-codec")]
pub struct OpusDecoder {
    format: AudioFormat,
    inner: opus::Decoder,
    /// Pre-allocated decode buffer (avoids per-frame allocation).
    out_buf: Vec<f32>,
    /// Frame size in samples per channel used for decoding.
    frame_size: usize,
}

#[cfg(feature = "opus-codec")]
impl OpusDecoder {
    /// Construct a new Opus decoder.
    ///
    /// `format` describes the desired PCM output format (sample rate
    /// and channel count must match valid Opus configurations).
    pub fn new(format: AudioFormat) -> Result<Self> {
        let channels = match format.channels {
            1 => opus::Channels::Mono,
            2 => opus::Channels::Stereo,
            n => {
                return Err(Error::InvalidState(format!(
                    "Opus only supports 1 or 2 channels, got {n}"
                )));
            }
        };

        let inner = opus::Decoder::new(format.sample_rate, channels)
            .map_err(|e| Error::OpusCodec(e.to_string()))?;

        // 120 ms is the maximum Opus frame duration
        let max_frame_size =
            (format.sample_rate as usize / 1000) * 120 * format.channels as usize;
        let out_buf = vec![0.0f32; max_frame_size];

        // Default frame size: 10 ms @ configured sample rate
        let frame_size = (format.sample_rate as usize / 1000) * 10;

        Ok(Self {
            format,
            inner,
            out_buf,
            frame_size,
        })
    }
}

#[cfg(feature = "opus-codec")]
impl AudioDecoder for OpusDecoder {
    fn output_format(&self) -> AudioFormat {
        self.format
    }

    fn decode(&mut self, packet: &EncodedPacket) -> Result<AudioFrame> {
        // Let libopus determine the actual frame size from the packet.
        // Pass the maximum buffer so it can decode any valid frame duration.
        let decoded_samples = self
            .inner
            .decode_float(&packet.data, &mut self.out_buf, false)
            .map_err(|e| Error::OpusCodec(e.to_string()))?;

        let total_samples = decoded_samples * self.format.channels as usize;
        let pcm = &self.out_buf[..total_samples];

        // Convert f32 slice to raw bytes (native-endian)
        let data: Vec<u8> = pcm.iter().flat_map(|s| s.to_ne_bytes()).collect();

        Ok(AudioFrame {
            data,
            format: self.format,
            sequence: packet.sequence,
            is_silent: false,
        })
    }

    fn decode_lost(&mut self) -> Result<AudioFrame> {
        // Use Opus built-in packet-loss concealment (PLC) by passing
        // an empty slice to decode_float.  libopus internally calls
        // opus_decode_float(dec, NULL, 0, ...) which generates a smooth
        // concealment frame based on the previous packet's state.
        // This is the same approach the official Mumble C++ client uses
        // (AudioOutputSpeech::needSamples -> opus_decode_float with NULL).
        let frame_size = self.frame_size;
        let needed = frame_size * self.format.channels as usize;
        if self.out_buf.len() < needed {
            self.out_buf.resize(needed, 0.0);
        }

        let decoded_samples = self
            .inner
            .decode_float(&[], &mut self.out_buf[..needed], false)
            .map_err(|e| Error::OpusCodec(e.to_string()))?;

        let total_samples = decoded_samples * self.format.channels as usize;
        let pcm = &self.out_buf[..total_samples];
        let data: Vec<u8> = pcm.iter().flat_map(|s| s.to_ne_bytes()).collect();

        Ok(AudioFrame {
            data,
            format: self.format,
            sequence: 0,
            is_silent: false,
        })
    }

    fn reset(&mut self) {
        // Create a fresh decoder to reset state.
        let channels = if self.format.channels == 1 {
            opus::Channels::Mono
        } else {
            opus::Channels::Stereo
        };
        if let Ok(fresh) = opus::Decoder::new(self.format.sample_rate, channels) {
            self.inner = fresh;
        }
    }
}

// ---------------------------------------------------------------------------
//  Stub decoder (no Opus feature)
// ---------------------------------------------------------------------------

/// Passthrough decoder used when the `opus-codec` feature is disabled.
///
/// Returns the raw payload bytes as-is - only useful for testing
/// with uncompressed audio or matching the encoder stub.
#[cfg(not(feature = "opus-codec"))]
pub struct OpusDecoder {
    format: AudioFormat,
}

#[cfg(not(feature = "opus-codec"))]
impl OpusDecoder {
    pub fn new(format: AudioFormat) -> Result<Self> {
        Ok(Self { format })
    }
}

#[cfg(not(feature = "opus-codec"))]
impl AudioDecoder for OpusDecoder {
    fn output_format(&self) -> AudioFormat {
        self.format
    }

    fn decode(&mut self, packet: &EncodedPacket) -> Result<AudioFrame> {
        Ok(AudioFrame {
            data: packet.data.clone(),
            format: self.format,
            sequence: packet.sequence,
            is_silent: false,
        })
    }

    fn reset(&mut self) {}
}
