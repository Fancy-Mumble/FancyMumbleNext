//! Bridge to the Android `FcmPlugin` for FCM device token retrieval.

use tauri::plugin::PluginHandle;
use tauri::Wry;

/// Managed state holding the Tauri mobile plugin handle for
/// `FcmPlugin` (Kotlin).
pub struct FcmPluginHandle(pub PluginHandle<Wry>);

/// Obtain the FCM device token from the Kotlin plugin.
///
/// The server uses this token to send individual push notifications.
pub fn get_token(handle: &FcmPluginHandle) -> Option<String> {
    tracing::info!("FCM: requesting device token");
    match handle
        .0
        .run_mobile_plugin::<serde_json::Value>("getToken", serde_json::json!({}))
    {
        Ok(val) => {
            let token = val
                .get("token")
                .and_then(|t| t.as_str())
                .map(String::from);
            if let Some(ref t) = token {
                tracing::info!(len = t.len(), "FCM: device token obtained");
            }
            token
        }
        Err(e) => {
            tracing::warn!("FCM: failed to get device token: {e}");
            None
        }
    }
}
