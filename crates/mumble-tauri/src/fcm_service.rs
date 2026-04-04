//! Bridge to the Android `FcmPlugin` for FCM topic subscriptions.
//!
//! When the client connects to a Mumble server and receives the
//! channel list, it subscribes to the corresponding FCM topics so
//! that the server's push notifications reach this device even when
//! the app is in the background.

use tauri::plugin::PluginHandle;
use tauri::Wry;

/// Managed state holding the Tauri mobile plugin handle for
/// `FcmPlugin` (Kotlin).
pub struct FcmPluginHandle(pub PluginHandle<Wry>);

/// Obtain the FCM device token from the Kotlin plugin.
///
/// The server uses this token to send individual push notifications
/// instead of broadcasting to FCM topics.
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

/// Subscribe to a single FCM topic.
pub fn subscribe_topic(handle: &FcmPluginHandle, topic: &str) {
    tracing::info!(topic, "FCM: subscribing to topic");
    let payload = serde_json::json!({ "topic": topic });
    if let Err(e) = handle.0.run_mobile_plugin::<()>("subscribeTopic", payload) {
        tracing::warn!(topic, "FCM: failed to subscribe to topic: {e}");
    }
}

/// Unsubscribe from a single FCM topic.
pub fn unsubscribe_topic(handle: &FcmPluginHandle, topic: &str) {
    tracing::info!(topic, "FCM: unsubscribing from topic");
    let payload = serde_json::json!({ "topic": topic });
    if let Err(e) = handle.0.run_mobile_plugin::<()>("unsubscribeTopic", payload) {
        tracing::warn!(topic, "FCM: failed to unsubscribe from topic: {e}");
    }
}

/// Subscribe to FCM topics for all given channel IDs.
///
/// The topic format matches the Mumble server's push dispatcher:
/// `{prefix}_server{server_id}_channel{channel_id}`
pub fn subscribe_channels(
    handle: &FcmPluginHandle,
    topic_prefix: &str,
    server_id: u32,
    channel_ids: &[u32],
) {
    let topics: Vec<String> = channel_ids
        .iter()
        .map(|&ch| format!("{topic_prefix}_server{server_id}_channel{ch}"))
        .collect();

    tracing::info!(count = topics.len(), "FCM: subscribing to channel topics");

    let payload = serde_json::json!({ "topics": topics });
    if let Err(e) = handle.0.run_mobile_plugin::<()>("subscribeTopics", payload) {
        tracing::warn!("FCM: failed to batch-subscribe topics: {e}");
    }
}

/// Unsubscribe from all FCM topics (deletes the FCM token).
pub fn unsubscribe_all(handle: &FcmPluginHandle) {
    tracing::info!("FCM: unsubscribing from all topics");
    let payload = serde_json::json!({});
    if let Err(e) = handle.0.run_mobile_plugin::<()>("unsubscribeAll", payload) {
        tracing::warn!("FCM: failed to unsubscribe all: {e}");
    }
}
