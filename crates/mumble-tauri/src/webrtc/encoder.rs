//! Frame encoding trait for compressing captured frames.

use crate::webrtc::capture::CapturedFrame;

/// Encoded frame ready for streaming.
pub struct EncodedFrame {
    /// Compressed bytes (e.g. JPEG, VP8).
    pub data: Vec<u8>,
}

/// Errors that can occur during encoding.
#[derive(Debug)]
pub struct EncodeError(pub String);

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "encode error: {}", self.0)
    }
}

impl std::error::Error for EncodeError {}

/// Encodes raw captured frames into a compressed format.
pub trait FrameEncoder: Send + Sync + 'static {
    /// Encode a single RGBA frame.
    fn encode(&self, frame: &CapturedFrame) -> Result<EncodedFrame, EncodeError>;
}

/// Simple JPEG encoder using the `image` crate.
pub struct JpegEncoder {
    quality: u8,
}

impl JpegEncoder {
    pub fn new(quality: u8) -> Self {
        Self { quality }
    }
}

impl FrameEncoder for JpegEncoder {
    fn encode(&self, frame: &CapturedFrame) -> Result<EncodedFrame, EncodeError> {
        use std::io::Cursor;

        let rgb = rgba_to_rgb(&frame.data);

        let img = image::RgbImage::from_raw(frame.width, frame.height, rgb)
            .ok_or_else(|| EncodeError("invalid frame dimensions".into()))?;

        let mut buf = Cursor::new(Vec::new());
        let encoder =
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, self.quality);

        img.write_with_encoder(encoder)
            .map_err(|e| EncodeError(e.to_string()))?;

        Ok(EncodedFrame {
            data: buf.into_inner(),
        })
    }
}

fn rgba_to_rgb(rgba: &[u8]) -> Vec<u8> {
    let pixel_count = rgba.len() / 4;
    let mut rgb = Vec::with_capacity(pixel_count * 3);
    for chunk in rgba.chunks_exact(4) {
        rgb.push(chunk[0]); // R
        rgb.push(chunk[1]); // G
        rgb.push(chunk[2]); // B
    }
    rgb
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
    use super::*;
    use crate::webrtc::capture::CapturedFrame;

    fn solid_red_frame(width: u32, height: u32) -> CapturedFrame {
        let pixel_count = (width as usize) * (height as usize);
        let mut data = Vec::with_capacity(pixel_count * 4);
        for _ in 0..pixel_count {
            data.extend_from_slice(&[255, 0, 0, 255]); // RGBA: red
        }
        CapturedFrame { width, height, data }
    }

    #[test]
    fn rgba_to_rgb_drops_alpha() {
        let result = rgba_to_rgb(&[10, 20, 30, 40]);
        assert_eq!(result, vec![10, 20, 30]);
    }

    #[test]
    fn rgba_to_rgb_multiple_pixels() {
        let rgba = vec![
            255, 0, 0, 255, // red in RGBA
            0, 255, 0, 255, // green in RGBA
        ];
        let rgb = rgba_to_rgb(&rgba);
        assert_eq!(rgb, vec![
            255, 0, 0,   // red in RGB
            0, 255, 0,   // green in RGB
        ]);
    }

    #[test]
    fn jpeg_encoder_produces_valid_jpeg() {
        let frame = solid_red_frame(4, 4);
        let encoder = JpegEncoder::new(80);
        let encoded = encoder.encode(&frame).unwrap();
        assert!(encoded.data.len() > 2, "JPEG output too small");
        assert_eq!(&encoded.data[..2], &[0xFF, 0xD8], "missing JPEG SOI marker");
    }

    #[test]
    fn jpeg_encoder_respects_quality() {
        let frame = solid_red_frame(16, 16);
        let low = JpegEncoder::new(10).encode(&frame).unwrap();
        let high = JpegEncoder::new(95).encode(&frame).unwrap();
        assert!(
            high.data.len() >= low.data.len(),
            "higher quality should produce equal or larger output"
        );
    }

    #[test]
    fn jpeg_encoder_rejects_mismatched_dimensions() {
        let frame = CapturedFrame {
            width: 100,
            height: 100,
            data: vec![0; 16], // way too small for 100x100 (needs 100*100*4 RGBA bytes)
        };
        let encoder = JpegEncoder::new(80);
        assert!(encoder.encode(&frame).is_err());
    }
}
