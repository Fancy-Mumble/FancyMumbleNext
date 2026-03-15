//! Common audio sample types shared across the entire pipeline.
//!
//! Every pipeline stage speaks in terms of [`AudioFrame`] - a time-stamped
//! buffer of PCM samples with associated format metadata. This keeps all
//! stages decoupled: they only depend on this shared vocabulary, never on
//! each other.

/// PCM sample format used throughout the pipeline.
///
/// Mumble/Opus operates on 16-bit signed integer PCM internally, but
/// some filters prefer f32. Both are supported; conversion helpers are
/// provided.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SampleFormat {
    /// 16-bit signed integer (native Opus input).
    I16,
    /// 32-bit float, normalised to [-1.0, 1.0].
    F32,
}

impl SampleFormat {
    /// Number of bytes used by a single sample in this format.
    pub const fn byte_width(self) -> usize {
        match self {
            Self::I16 => 2,
            Self::F32 => 4,
        }
    }
}

/// Describes the shape of an audio buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AudioFormat {
    /// Sample rate in Hz (typically 48 000 for Opus).
    pub sample_rate: u32,
    /// Number of channels (1 = mono, 2 = stereo).
    pub channels: u16,
    /// Per-sample format.
    pub sample_format: SampleFormat,
}

impl AudioFormat {
    pub const MONO_48KHZ_F32: Self = Self {
        sample_rate: 48_000,
        channels: 1,
        sample_format: SampleFormat::F32,
    };

    pub const MONO_48KHZ_I16: Self = Self {
        sample_rate: 48_000,
        channels: 1,
        sample_format: SampleFormat::I16,
    };
}

/// A single frame of PCM audio data flowing through the pipeline.
///
/// Frames always carry their format so that each stage can validate or
/// convert as needed without hidden assumptions.
#[derive(Debug, Clone)]
pub struct AudioFrame {
    /// The PCM sample buffer.
    ///
    /// - [`SampleFormat::I16`]: each sample is 2 bytes, little-endian.
    /// - [`SampleFormat::F32`]: each sample is 4 bytes, native-endian.
    ///
    /// Interleaved when multi-channel (L R L R ...).
    pub data: Vec<u8>,
    /// Format describing the samples in `data`.
    pub format: AudioFormat,
    /// Monotonically increasing frame sequence number.
    pub sequence: u64,
    /// Set by a voice-activity filter (e.g. [`NoiseGate`]) when this
    /// frame was silenced.  The outbound pipeline uses this to suppress
    /// transmission and send terminator packets at end-of-speech.
    pub is_silent: bool,
}

impl AudioFrame {
    /// Number of samples **per channel** in this frame.
    pub fn sample_count(&self) -> usize {
        let bytes_per_sample = match self.format.sample_format {
            SampleFormat::I16 => 2,
            SampleFormat::F32 => 4,
        };
        self.data.len() / (bytes_per_sample * self.format.channels as usize)
    }

    /// Duration of this frame in seconds.
    pub fn duration_secs(&self) -> f64 {
        self.sample_count() as f64 / self.format.sample_rate as f64
    }

    /// View the sample buffer as `&[f32]` (only valid when format is F32).
    ///
    /// # Panics
    /// Panics if the sample format is not `F32`.
    pub fn as_f32_samples(&self) -> &[f32] {
        assert_eq!(self.format.sample_format, SampleFormat::F32);
        bytemuck_cast_slice(&self.data)
    }

    /// View the sample buffer as `&mut [f32]` (only valid when format is F32).
    ///
    /// # Panics
    /// Panics if the sample format is not `F32`.
    pub fn as_f32_samples_mut(&mut self) -> &mut [f32] {
        assert_eq!(self.format.sample_format, SampleFormat::F32);
        bytemuck_cast_slice_mut(&mut self.data)
    }

    /// View the sample buffer as `&[i16]` (only valid when format is I16).
    ///
    /// # Panics
    /// Panics if the sample format is not `I16`.
    pub fn as_i16_samples(&self) -> &[i16] {
        assert_eq!(self.format.sample_format, SampleFormat::I16);
        bytemuck_cast_slice(&self.data)
    }
}

// -- Minimal safe byte-casting (avoids adding a `bytemuck` dep) -----

#[allow(unsafe_code)]
fn bytemuck_cast_slice<T: Copy>(bytes: &[u8]) -> &[T] {
    let len = bytes.len() / size_of::<T>();
    assert_eq!(bytes.len() % size_of::<T>(), 0);
    unsafe { std::slice::from_raw_parts(bytes.as_ptr() as *const T, len) }
}

#[allow(unsafe_code)]
fn bytemuck_cast_slice_mut<T: Copy>(bytes: &mut [u8]) -> &mut [T] {
    let len = bytes.len() / size_of::<T>();
    assert_eq!(bytes.len() % size_of::<T>(), 0);
    unsafe { std::slice::from_raw_parts_mut(bytes.as_mut_ptr() as *mut T, len) }
}

// -- Conversion helpers ---------------------------------------------

/// Convert an `i16` PCM sample to `f32` normalised to [-1.0, 1.0].
pub fn i16_to_f32(sample: i16) -> f32 {
    sample as f32 / i16::MAX as f32
}

/// Convert a normalised `f32` sample back to `i16`.
pub fn f32_to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_count_mono_f32() {
        let frame = AudioFrame {
            data: vec![0u8; 4 * 480], // 480 f32 samples
            format: AudioFormat::MONO_48KHZ_F32,
            sequence: 0,
            is_silent: false,
        };
        assert_eq!(frame.sample_count(), 480);
    }

    #[test]
    fn i16_f32_roundtrip() {
        let original: i16 = 16000;
        let converted = f32_to_i16(i16_to_f32(original));
        assert_eq!(original, converted);
    }
}
