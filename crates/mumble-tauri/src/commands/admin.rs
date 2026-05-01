//! Server-administration commands: kick/ban/mute/deafen, registered user
//! management, ban list and ACL editing.

use crate::state::{self, AppState};

/// Kick a user from the server.
#[tauri::command]
pub(crate) async fn kick_user(
    state: tauri::State<'_, AppState>,
    session: u32,
    reason: Option<String>,
) -> Result<(), String> {
    state.kick_user(session, reason).await
}

/// Ban a user from the server.
#[tauri::command]
pub(crate) async fn ban_user(
    state: tauri::State<'_, AppState>,
    session: u32,
    reason: Option<String>,
) -> Result<(), String> {
    state.ban_user(session, reason).await
}

/// Register a user on the server using their current certificate.
#[tauri::command]
pub(crate) async fn register_user(
    state: tauri::State<'_, AppState>,
    session: u32,
) -> Result<(), String> {
    state.register_user(session).await
}

/// Admin-mute or unmute another user.
#[tauri::command]
pub(crate) async fn mute_user(
    state: tauri::State<'_, AppState>,
    session: u32,
    muted: bool,
) -> Result<(), String> {
    state.mute_user(session, muted).await
}

/// Admin-deafen or undeafen another user.
#[tauri::command]
pub(crate) async fn deafen_user(
    state: tauri::State<'_, AppState>,
    session: u32,
    deafened: bool,
) -> Result<(), String> {
    state.deafen_user(session, deafened).await
}

/// Set or clear priority speaker for another user.
#[tauri::command]
pub(crate) async fn set_priority_speaker(
    state: tauri::State<'_, AppState>,
    session: u32,
    priority: bool,
) -> Result<(), String> {
    state.set_priority_speaker(session, priority).await
}

/// Reset another user's comment (admin action).
#[tauri::command]
pub(crate) async fn reset_user_comment(
    state: tauri::State<'_, AppState>,
    session: u32,
) -> Result<(), String> {
    state.reset_user_comment(session).await
}

/// Remove another user's avatar (admin action).
#[tauri::command]
pub(crate) async fn remove_user_avatar(
    state: tauri::State<'_, AppState>,
    session: u32,
) -> Result<(), String> {
    state.remove_user_avatar(session).await
}

/// Move another user to a different channel (admin action).
#[tauri::command]
pub(crate) async fn move_user_to_channel(
    state: tauri::State<'_, AppState>,
    session: u32,
    channel_id: u32,
) -> Result<(), String> {
    state.move_user(session, channel_id).await
}

/// Request ping/connection statistics for a specific user.
///
/// The server responds asynchronously with a `UserStats` message,
/// which is emitted to the frontend as a `"user-stats"` event.
#[tauri::command]
pub(crate) async fn request_user_stats(
    state: tauri::State<'_, AppState>,
    session: u32,
) -> Result<(), String> {
    state.request_user_stats(session).await
}

/// Request the registered user list from the server.
#[tauri::command]
pub(crate) async fn request_user_list(
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state.request_user_list().await
}

/// Send updated user-list entries (rename / delete) to the server.
#[tauri::command]
pub(crate) async fn update_user_list(
    state: tauri::State<'_, AppState>,
    users: Vec<state::types::RegisteredUserUpdate>,
) -> Result<(), String> {
    state.update_user_list(users).await
}

/// Request the full comment for an offline registered user by their user ID.
/// The server responds (if a comment exists) with a `user-comment` event.
#[tauri::command]
pub(crate) async fn request_user_comment(
    state: tauri::State<'_, AppState>,
    user_id: u32,
) -> Result<(), String> {
    state.request_user_comment(user_id).await
}

/// Request the ban list from the server.
#[tauri::command]
pub(crate) async fn request_ban_list(
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    state.request_ban_list().await
}

/// Send an updated ban list to the server (replaces the full list).
#[tauri::command]
pub(crate) async fn update_ban_list(
    state: tauri::State<'_, AppState>,
    bans: Vec<state::types::BanEntryInput>,
) -> Result<(), String> {
    state.update_ban_list(bans).await
}

/// Request the ACL for a specific channel.
#[tauri::command]
pub(crate) async fn request_acl(
    state: tauri::State<'_, AppState>,
    channel_id: u32,
) -> Result<(), String> {
    state.request_acl(channel_id).await
}

/// Send an updated ACL for a channel to the server.
#[tauri::command]
pub(crate) async fn update_acl(
    state: tauri::State<'_, AppState>,
    acl: state::types::AclInput,
) -> Result<(), String> {
    state.update_acl(acl).await
}
