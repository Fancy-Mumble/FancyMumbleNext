//! Audio sample conversion utilities.

/// Convert an `i16` PCM sample to `f32` normalised to [-1.0, 1.0].
pub fn i16_to_f32(sample: i16) -> f32 {
    sample as f32 / i16::MAX as f32
}

/// Convert a normalised `f32` sample back to `i16`.
///
/// The input is clamped to [-1.0, 1.0] before conversion.
pub fn f32_to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i16_f32_roundtrip() {
        let original: i16 = 16000;
        let converted = f32_to_i16(i16_to_f32(original));
        assert_eq!(original, converted);
    }

    #[test]
    fn zero_roundtrip() {
        assert_eq!(f32_to_i16(i16_to_f32(0)), 0);
    }

    #[test]
    fn clamp_above_one() {
        assert_eq!(f32_to_i16(1.5), i16::MAX);
    }

    #[test]
    fn clamp_below_neg_one() {
        assert_eq!(f32_to_i16(-1.5), -i16::MAX);
    }

    #[test]
    fn max_i16_to_f32() {
        let f = i16_to_f32(i16::MAX);
        assert!((f - 1.0).abs() < f32::EPSILON);
    }
}
