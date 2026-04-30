//! Image popout window: borderless, transparent, always-on-top viewer.

use crate::state::AppState;

/// Metadata sent alongside an image to the popout window.
///
/// The frontend builds this payload from the originating chat message
/// so the popout can display a frosted-glass info bar with sender,
/// avatar, optional caption and timestamp.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct PopoutImagePayload {
    /// Image URL or data URL to display.
    pub src: String,
    /// Display name of the user who posted the image.
    #[serde(default)]
    pub sender_name: Option<String>,
    /// Avatar image (data URL) of the sender, if available.
    #[serde(default)]
    pub sender_avatar: Option<String>,
    /// Optional caption / surrounding text from the chat message.
    #[serde(default)]
    pub caption: Option<String>,
    /// Unix epoch milliseconds when the message was sent.
    #[serde(default)]
    pub timestamp_ms: Option<i64>,
}

/// Open a borderless, always-on-top window displaying a single image.
///
/// The frontend hands us the image payload (src + display metadata),
/// we stash it under a fresh id, and a new webview window is opened
/// with label `popout-<id>`. The popout's frontend reads its own
/// window label to recover the id and calls [`take_popout_image`].
#[cfg(not(target_os = "android"))]
#[tauri::command]
pub(crate) async fn open_image_popout(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    payload: PopoutImagePayload,
    width: Option<f64>,
    height: Option<f64>,
) -> Result<(), String> {
    let id = uuid::Uuid::new_v4().simple().to_string();
    if let Ok(mut map) = state.popout_images.lock() {
        let _ = map.insert(id.clone(), payload);
    }

    let label = format!("popout-{id}");

    let w = width.unwrap_or(720.0).clamp(160.0, 4096.0);
    let h = height.unwrap_or(480.0).clamp(120.0, 4096.0);

    let _window = tauri::WebviewWindowBuilder::new(
        &app,
        &label,
        tauri::WebviewUrl::App(std::path::PathBuf::from("index.html")),
    )
    .title("")
    .decorations(false)
    .shadow(false)
    .transparent(true)
    .always_on_top(true)
    .inner_size(w, h)
    .resizable(true)
    .skip_taskbar(false)
    .build()
    .map_err(|e: tauri::Error| e.to_string())?;

    Ok(())
}

/// Android stub: popout windows are a desktop-only feature.
#[cfg(target_os = "android")]
#[tauri::command]
pub(crate) async fn open_image_popout(
    _app: tauri::AppHandle,
    _state: tauri::State<'_, AppState>,
    _payload: PopoutImagePayload,
    _width: Option<f64>,
    _height: Option<f64>,
) -> Result<(), String> {
    Err("Image popout windows are not supported on Android".to_string())
}

/// Consume and return the image payload registered for a popout window id.
/// The id is parsed from the calling window's label (`popout-<id>`).
/// Each id is single-use; a missing/already-taken id returns `None`.
#[tauri::command]
pub(crate) fn take_popout_image(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Option<PopoutImagePayload> {
    state
        .popout_images
        .lock()
        .ok()
        .and_then(|mut m| m.remove(&id))
}
