//! Image processing commands (blur, dim, JPEG re-encode).

/// Apply a Gaussian blur to an image.
///
/// `image_base64` is the raw file content encoded as a base64 string.
/// `sigma` controls the blur strength (typical range 1.0 - 30.0).
/// Returns base64-encoded JPEG bytes.
///
/// Runs on a dedicated blocking thread so the async runtime (and Tauri IPC)
/// stays responsive while the CPU-heavy image processing executes.
#[tauri::command]
pub(crate) async fn blur_image(image_base64: String, sigma: f32) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        use base64::{engine::general_purpose::STANDARD, Engine};
        use fancy_utils::image_filter::{BlurFilter, ImageFilter};

        let image_bytes = STANDARD
            .decode(&image_base64)
            .map_err(|e| format!("Failed to decode base64 input: {e}"))?;

        let result = BlurFilter::new(sigma)
            .apply(&image_bytes)
            .map_err(|e| e.to_string())?;
        Ok(STANDARD.encode(result))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Process a chat background image by applying blur and/or dim in one pass.
///
/// `image_base64` is the raw file content encoded as a base64 string.
/// `sigma` controls blur strength (0 = no blur, typical range 1.0 - 30.0).
/// `dim` controls darkening (0.0 = no dim, 1.0 = fully black).
/// Returns base64-encoded JPEG bytes.
///
/// The image is downscaled to 960x540 before processing to keep blur fast.
/// Since the result is used as a blurred/dimmed background, the reduced
/// resolution is imperceptible.
///
/// Runs on a dedicated blocking thread so the async runtime (and Tauri IPC)
/// stays responsive while the CPU-heavy image processing executes.
#[tauri::command]
pub(crate) async fn process_background(
    image_base64: String,
    sigma: f32,
    dim: f32,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        use base64::{engine::general_purpose::STANDARD, Engine};
        use fancy_utils::image_filter::{process_pipeline, BlurFilter, DimFilter, ImageTransform};

        let image_bytes = STANDARD
            .decode(&image_base64)
            .map_err(|e| format!("Failed to decode base64 input: {e}"))?;

        let blur = BlurFilter::new(sigma);
        let dim_filter = DimFilter::new(dim);

        let mut transforms: Vec<&dyn ImageTransform> = Vec::new();
        if sigma > 0.0 {
            transforms.push(&blur);
        }
        if dim > 0.0 {
            transforms.push(&dim_filter);
        }

        if transforms.is_empty() {
            // No processing needed, but re-encode to JPEG for consistency.
            let result = process_pipeline(&image_bytes, &[], false)
                .map_err(|e| e.to_string())?;
            return Ok(STANDARD.encode(result));
        }

        let result =
            process_pipeline(&image_bytes, &transforms, true).map_err(|e| e.to_string())?;
        Ok(STANDARD.encode(result))
    })
    .await
    .map_err(|e| e.to_string())?
}
