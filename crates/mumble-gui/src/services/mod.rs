//! Service traits that decouple the GUI from the protocol backend.
//!
//! Any frontend (Dioxus, egui, TUI, …) programs against these traits.
//! The concrete implementation lives in [`crate::services::mumble_backend`]
//! and can be swapped without touching UI code.

pub mod mumble_backend;

use std::future::Future;
use std::pin::Pin;

/// Lightweight value types the UI works with - no protobuf leaks.
///
/// A channel visible on the server.
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelEntry {
    pub id: u32,
    pub name: String,
    pub description: String,
    /// Number of users currently in the channel.
    pub user_count: u32,
}

/// A user visible on the server.
#[derive(Debug, Clone, PartialEq)]
pub struct UserEntry {
    pub session: u32,
    pub name: String,
    pub channel_id: u32,
    /// Optional texture / avatar bytes (PNG).
    pub avatar: Option<Vec<u8>>,
}

/// A single chat message.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatMessage {
    /// Session id of the sender (`None` = server message).
    pub sender_session: Option<u32>,
    /// Display name of the sender.
    pub sender_name: String,
    /// Message body (may contain HTML).
    pub body: String,
    /// Channel the message was sent to (0 = current).
    pub channel_id: u32,
    /// Whether the current user sent this message.
    pub is_own: bool,
}

/// Connection state the UI can query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnectionStatus {
    #[default]
    Disconnected,
    Connecting,
    Connected,
}

/// Events pushed from the protocol backend to the GUI for reactive updates.
#[derive(Debug, Clone)]
pub enum ServerEvent {
    /// `ServerSync` received - fully connected and authenticated.
    Connected,
    /// Connection dropped or shut down.
    Disconnected,
    /// User/channel state changed - refresh lists.
    StateChanged,
    /// A new text message arrived for a channel.
    NewMessage { channel_id: u32 },
    /// Server rejected the connection.
    Rejected { reason: String },
}

/// Result alias for service operations.
pub type ServiceResult<T> = Result<T, String>;
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

// ─── Core service trait ────────────────────────────────────────────

/// The interface every backend must implement.
///
/// The GUI talks **only** through this trait, never directly to
/// `mumble-protocol` types.  This makes it trivial to swap the
/// backend out for mocks in tests or a different protocol entirely.
#[allow(dead_code)]
pub trait MumbleService: Send + Sync + 'static {
    /// Connect to a Mumble server.
    fn connect(
        &self,
        host: String,
        port: u16,
        username: String,
    ) -> BoxFuture<'_, ServiceResult<()>>;

    /// Disconnect from the current server.
    fn disconnect(&self) -> BoxFuture<'_, ServiceResult<()>>;

    /// Current connection status.
    fn status(&self) -> ConnectionStatus;

    /// Snapshot of all channels on the server.
    fn channels(&self) -> Vec<ChannelEntry>;

    /// Snapshot of all users on the server.
    fn users(&self) -> Vec<UserEntry>;

    /// Send a text message to a channel.
    fn send_message(
        &self,
        channel_id: u32,
        body: String,
    ) -> BoxFuture<'_, ServiceResult<()>>;

    /// Get recent chat messages for a channel.
    fn messages(&self, channel_id: u32) -> Vec<ChatMessage>;
}
