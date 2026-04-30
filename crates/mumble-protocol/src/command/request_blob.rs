use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Request the server to send full texture / comment blobs for specific users.
///
/// Mumble servers often omit large `texture` and `comment` payloads from the
/// initial `UserState` batch and only include a hash.  Sending a `RequestBlob`
/// with the relevant session IDs causes the server to follow up with full
/// `UserState` messages containing the actual data.
#[derive(Debug)]
pub struct RequestBlob {
    /// Sessions whose **texture** (avatar) should be fetched.
    pub session_texture: Vec<u32>,
    /// Sessions whose **comment** should be fetched.
    pub session_comment: Vec<u32>,
    /// Channels whose **description** should be fetched.
    pub channel_description: Vec<u32>,
    /// Registered user IDs whose full **comment** should be fetched.
    /// Used for offline users (who have no session) when their profile is opened.
    pub user_id_comment: Vec<u32>,
}

impl CommandAction for RequestBlob {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::RequestBlob {
            session_texture: self.session_texture.clone(),
            session_comment: self.session_comment.clone(),
            channel_description: self.channel_description.clone(),
            user_id_comment: self.user_id_comment.clone(),
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::RequestBlob(msg)],
            ..Default::default()
        }
    }
}
