//! Query channel permissions from the server.
//!
//! Sends a `PermissionQuery` with the target `channel_id`.  The server
//! responds with the same message type, `permissions` field populated
//! with the granted permission bitmask for the requesting user.

use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

use super::core::{CommandAction, CommandOutput};

/// Ask the server what permissions the current user has on `channel_id`.
#[derive(Debug)]
pub struct PermissionQuery {
    pub channel_id: u32,
}

impl CommandAction for PermissionQuery {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::PermissionQuery {
            channel_id: Some(self.channel_id),
            permissions: None,
            flush: None,
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::PermissionQuery(msg)],
            ..Default::default()
        }
    }
}
