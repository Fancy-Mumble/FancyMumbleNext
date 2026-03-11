//! Reactive application state shared across all Dioxus components.

use crate::services::{ChannelEntry, ChatMessage, ConnectionStatus, UserEntry};

/// The page the user is currently viewing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    /// Server connection form.
    Connect,
    /// Main chat view (channel list + chat pane).
    Chat,
}

/// Root application state stored in Dioxus context.
#[derive(Debug, Clone, PartialEq)]
pub struct AppState {
    pub page: Page,
    pub status: ConnectionStatus,
    pub server_host: String,
    pub server_port: String,
    pub username: String,
    pub channels: Vec<ChannelEntry>,
    pub users: Vec<UserEntry>,
    pub selected_channel: Option<u32>,
    pub messages: Vec<ChatMessage>,
    pub message_draft: String,
    pub error: Option<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            page: Page::Connect,
            status: ConnectionStatus::Disconnected,
            server_host: String::new(),
            server_port: "64738".into(),
            username: String::new(),
            channels: Vec::new(),
            users: Vec::new(),
            selected_channel: None,
            messages: Vec::new(),
            message_draft: String::new(),
            error: None,
        }
    }
}
