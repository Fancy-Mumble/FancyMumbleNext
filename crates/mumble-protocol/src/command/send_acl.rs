use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Send an updated ACL for a channel to the server.
#[derive(Debug)]
pub struct SendAcl {
    /// Target channel.
    pub channel_id: u32,
    /// Whether to inherit ACLs from the parent channel.
    pub inherit_acls: bool,
    /// Group definitions for this channel.
    pub groups: Vec<mumble_tcp::acl::ChanGroup>,
    /// ACL rules for this channel.
    pub acls: Vec<mumble_tcp::acl::ChanAcl>,
}

impl CommandAction for SendAcl {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::Acl {
            channel_id: self.channel_id,
            inherit_acls: Some(self.inherit_acls),
            groups: self.groups.clone(),
            acls: self.acls.clone(),
            query: Some(false),
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::Acl(msg)],
            ..Default::default()
        }
    }
}
