use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Request the ACL for a specific channel from the server.
#[derive(Debug)]
pub struct RequestAcl {
    /// The channel to request ACL for.
    pub channel_id: u32,
}

impl CommandAction for RequestAcl {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::Acl {
            channel_id: self.channel_id,
            inherit_acls: None,
            groups: Vec::new(),
            acls: Vec::new(),
            query: Some(true),
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::Acl(msg)],
            ..Default::default()
        }
    }
}
