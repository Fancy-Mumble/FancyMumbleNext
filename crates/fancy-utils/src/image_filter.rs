//! Image filtering utilities.
//!
//! Provides the [`ImageFilter`] trait for transforming raw image bytes, along
//! with concrete filter implementations such as [`BlurFilter`] and
//! [`DimFilter`].
//!
//! # Example
//!
//! ```no_run
//! use fancy_utils::image_filter::{ImageFilter, BlurFilter};
//!
//! let original: Vec<u8> = std::fs::read("photo.jpg").unwrap();
//! let blurred = BlurFilter::new(8.0).apply(&original).unwrap();
//! ```

use std::io::Cursor;

use image::{ImageFormat, ImageReader};

/// Maximum dimension for downscaling before processing.
/// Images larger than this are shrunk to fit within these bounds before
/// blur/dim, dramatically reducing processing time.
const PROCESS_MAX_WIDTH: u32 = 960;
const PROCESS_MAX_HEIGHT: u32 = 540;

/// Error type returned by image filter operations.
#[derive(Debug)]
pub struct ImageFilterError(String);

impl std::fmt::Display for ImageFilterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ImageFilterError {}

impl ImageFilterError {
    fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

/// Decode raw image bytes (JPEG or PNG) into an [`image::DynamicImage`].
fn decode(bytes: &[u8]) -> Result<image::DynamicImage, ImageFilterError> {
    ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| ImageFilterError::new(format!("Failed to detect image format: {e}")))?
        .decode()
        .map_err(|e| ImageFilterError::new(format!("Failed to decode image: {e}")))
}

/// Encode a [`image::DynamicImage`] as JPEG bytes.
fn encode_jpeg(img: &image::DynamicImage) -> Result<Vec<u8>, ImageFilterError> {
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Jpeg)
        .map_err(|e| ImageFilterError::new(format!("Failed to encode image as JPEG: {e}")))?;
    Ok(buf)
}

/// A transformation that can be applied to raw image bytes.
///
/// Implementors receive a slice of raw JPEG/PNG bytes and return the
/// transformed image as JPEG bytes.
pub trait ImageFilter {
    /// Apply the filter to `input` bytes and return the result as JPEG bytes.
    ///
    /// # Errors
    ///
    /// Returns an [`ImageFilterError`] if decoding, processing, or encoding
    /// fails.
    fn apply(&self, input: &[u8]) -> Result<Vec<u8>, ImageFilterError>;
}

/// A transformation that operates directly on a decoded [`image::DynamicImage`],
/// avoiding repeated decode/encode cycles when chaining multiple operations.
pub trait ImageTransform {
    /// Apply the transformation to an already-decoded image.
    fn transform(&self, img: image::DynamicImage) -> image::DynamicImage;
}

/// An [`ImageFilter`] that applies a Gaussian blur.
///
/// `sigma` controls the blur radius (clamped to `[0.0, 50.0]`).  A value of
/// `0.0` returns the image unchanged.  Typical range for UI backgrounds is
/// `4.0` – `20.0`.
#[derive(Debug, Clone, Copy)]
pub struct BlurFilter {
    sigma: f32,
}

impl BlurFilter {
    /// Create a new `BlurFilter` with the given sigma (blur radius).
    ///
    /// The value is clamped to `[0.0, 50.0]`.
    #[must_use]
    pub fn new(sigma: f32) -> Self {
        Self {
            sigma: sigma.clamp(0.0, 50.0),
        }
    }

    /// Return the (clamped) sigma value.
    #[must_use]
    pub fn sigma(self) -> f32 {
        self.sigma
    }
}

impl ImageFilter for BlurFilter {
    fn apply(&self, input: &[u8]) -> Result<Vec<u8>, ImageFilterError> {
        let img = decode(input)?;
        encode_jpeg(&self.transform(img))
    }
}

impl ImageTransform for BlurFilter {
    fn transform(&self, img: image::DynamicImage) -> image::DynamicImage {
        img.blur(self.sigma)
    }
}

/// An [`ImageFilter`] that darkens the image by a given factor, simulating
/// a semi-transparent black overlay.
///
/// `dim` is clamped to `[0.0, 1.0]`.  A value of `0.0` returns the image
/// unchanged; `1.0` produces a fully black image.
#[derive(Debug, Clone, Copy)]
pub struct DimFilter {
    dim: f32,
}

impl DimFilter {
    /// Create a new `DimFilter`.
    ///
    /// `dim` is clamped to `[0.0, 1.0]`.
    #[must_use]
    pub fn new(dim: f32) -> Self {
        Self {
            dim: dim.clamp(0.0, 1.0),
        }
    }

    /// Return the (clamped) dim value.
    #[must_use]
    pub fn dim(self) -> f32 {
        self.dim
    }
}

impl ImageFilter for DimFilter {
    fn apply(&self, input: &[u8]) -> Result<Vec<u8>, ImageFilterError> {
        let img = decode(input)?;
        encode_jpeg(&self.transform(img))
    }
}

impl ImageTransform for DimFilter {
    fn transform(&self, img: image::DynamicImage) -> image::DynamicImage {
        if self.dim == 0.0 {
            return img;
        }

        let mut rgba = img.to_rgba8();
        let factor = 1.0 - self.dim;

        for pixel in rgba.pixels_mut() {
            pixel[0] = (f32::from(pixel[0]) * factor) as u8;
            pixel[1] = (f32::from(pixel[1]) * factor) as u8;
            pixel[2] = (f32::from(pixel[2]) * factor) as u8;
        }

        image::DynamicImage::ImageRgba8(rgba)
    }
}

/// Apply a sequence of filters to raw image bytes in a single pipeline.
///
/// Each filter in `filters` is applied in order, passing the output of one
/// as the input to the next.  Returns the final JPEG-encoded bytes.
///
/// # Errors
///
/// Returns an [`ImageFilterError`] if any filter in the chain fails.
pub fn apply_chain(
    input: &[u8],
    filters: &[&dyn ImageFilter],
) -> Result<Vec<u8>, ImageFilterError> {
    let mut data = input.to_vec();
    for filter in filters {
        data = filter.apply(&data)?;
    }
    Ok(data)
}

/// Apply a sequence of [`ImageTransform`]s to raw image bytes in a single
/// decode-process-encode pass.
///
/// Unlike [`apply_chain`], this function decodes the input **once**, applies
/// all transforms on the in-memory image, and encodes back to JPEG only at
/// the end.  When `downscale` is `true` the image is shrunk to at most
/// 960x540 before applying transforms, which dramatically speeds up
/// expensive operations like Gaussian blur.
///
/// # Errors
///
/// Returns an [`ImageFilterError`] if decoding or encoding fails.
pub fn process_pipeline(
    input: &[u8],
    transforms: &[&dyn ImageTransform],
    downscale: bool,
) -> Result<Vec<u8>, ImageFilterError> {
    let mut img = decode(input)?;

    if downscale {
        let (w, h) = (img.width(), img.height());
        if w > PROCESS_MAX_WIDTH || h > PROCESS_MAX_HEIGHT {
            img = img.resize(
                PROCESS_MAX_WIDTH,
                PROCESS_MAX_HEIGHT,
                image::imageops::FilterType::Triangle,
            );
        }
    }

    for t in transforms {
        img = t.transform(img);
    }

    encode_jpeg(&img)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, reason = "unwrap/expect acceptable in test code")]
mod tests {
    use super::*;

    /// Minimal 1x1 white JPEG for unit tests (no filesystem required).
    fn tiny_jpeg() -> Vec<u8> {
        // Create a 1x1 white RGB image and encode it as JPEG in memory.
        let img = image::DynamicImage::new_rgb8(4, 4);
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Jpeg)
            .expect("encode test image");
        buf
    }

    #[test]
    fn blur_filter_produces_jpeg() {
        let input = tiny_jpeg();
        let result = BlurFilter::new(2.0).apply(&input).unwrap();
        // JPEG magic bytes: 0xFF 0xD8
        assert_eq!(&result[..2], &[0xFF, 0xD8]);
    }

    #[test]
    fn blur_filter_zero_sigma_roundtrips() {
        let input = tiny_jpeg();
        let result = BlurFilter::new(0.0).apply(&input).unwrap();
        assert!(!result.is_empty());
    }

    #[test]
    fn blur_filter_clamps_sigma() {
        let f = BlurFilter::new(999.0);
        assert_eq!(f.sigma(), 50.0);
        let f2 = BlurFilter::new(-5.0);
        assert_eq!(f2.sigma(), 0.0);
    }

    #[test]
    fn dim_filter_produces_jpeg() {
        let input = tiny_jpeg();
        let result = DimFilter::new(0.5).apply(&input).unwrap();
        assert_eq!(&result[..2], &[0xFF, 0xD8]);
    }

    #[test]
    fn dim_filter_zero_is_noop() {
        let input = tiny_jpeg();
        let result = DimFilter::new(0.0).apply(&input).unwrap();
        assert!(!result.is_empty());
    }

    #[test]
    fn dim_filter_clamps_value() {
        let f = DimFilter::new(2.0);
        assert_eq!(f.dim(), 1.0);
        let f2 = DimFilter::new(-0.5);
        assert_eq!(f2.dim(), 0.0);
    }

    #[test]
    fn dim_filter_full_produces_dark_image() {
        // Create a 2x2 white image.
        let img = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            2,
            2,
            image::Rgb([255, 255, 255]),
        ));
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
            .unwrap();

        let result = DimFilter::new(1.0).apply(&buf).unwrap();
        let decoded = decode(&result).unwrap().to_rgb8();
        // Every pixel should be black (0, 0, 0).
        for pixel in decoded.pixels() {
            assert_eq!(pixel.0, [0, 0, 0]);
        }
    }

    #[test]
    fn apply_chain_blur_and_dim() {
        let input = tiny_jpeg();
        let blur = BlurFilter::new(2.0);
        let dim = DimFilter::new(0.3);
        let result = apply_chain(&input, &[&blur, &dim]).unwrap();
        assert_eq!(&result[..2], &[0xFF, 0xD8]);
    }

    #[test]
    fn process_pipeline_single_decode_encode() {
        let input = tiny_jpeg();
        let blur = BlurFilter::new(2.0);
        let dim = DimFilter::new(0.3);
        let result = process_pipeline(&input, &[&blur, &dim], false).unwrap();
        assert_eq!(&result[..2], &[0xFF, 0xD8]);
    }

    #[test]
    fn process_pipeline_with_downscale() {
        // Create a 2000x1200 image that should be downscaled.
        let img = image::DynamicImage::new_rgb8(2000, 1200);
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Jpeg)
            .expect("encode test image");

        let blur = BlurFilter::new(4.0);
        let result = process_pipeline(&buf, &[&blur], true).unwrap();

        // Verify it's valid JPEG and was downscaled.
        let decoded = decode(&result).unwrap();
        assert!(decoded.width() <= 960);
        assert!(decoded.height() <= 540);
    }

    #[test]
    fn process_pipeline_no_downscale_preserves_size() {
        let img = image::DynamicImage::new_rgb8(800, 600);
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Jpeg)
            .expect("encode test image");

        let dim = DimFilter::new(0.5);
        let result = process_pipeline(&buf, &[&dim], false).unwrap();

        let decoded = decode(&result).unwrap();
        assert_eq!(decoded.width(), 800);
        assert_eq!(decoded.height(), 600);
    }
}
