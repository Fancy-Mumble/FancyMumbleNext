use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Authenticate with the Mumble server.
#[derive(Debug)]
pub struct Authenticate {
    /// Username to authenticate with.
    pub username: String,
    /// Optional server password.
    pub password: Option<String>,
    /// Access tokens for permission-gated channels.
    pub tokens: Vec<String>,
}

impl CommandAction for Authenticate {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::Authenticate {
            username: Some(self.username.clone()),
            password: self.password.clone(),
            tokens: self.tokens.clone(),
            opus: Some(true),
            ..Default::default()
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::Authenticate(msg)],
            ..Default::default()
        }
    }
}
