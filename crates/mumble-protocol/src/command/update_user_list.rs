use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Send an updated registered user list to the server.
///
/// Each entry carries a `user_id` and an optional `name`.
/// - Present `name` = rename that user.
/// - Absent `name`  = delete (deregister) that user.
///
/// Only the entries included in the list are modified; all other
/// registered users remain untouched on the server.
#[derive(Debug)]
pub struct UpdateUserList {
    /// The user entries to update.
    pub users: Vec<UserListEntry>,
}

/// A single entry in an [`UpdateUserList`] command.
#[derive(Debug)]
pub struct UserListEntry {
    /// Server-assigned registered user id.
    pub user_id: u32,
    /// New display name, or `None` to delete the registration.
    pub name: Option<String>,
}

impl CommandAction for UpdateUserList {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        let users = self
            .users
            .iter()
            .map(|u| mumble_tcp::user_list::User {
                user_id: u.user_id,
                name: u.name.clone(),
                ..Default::default()
            })
            .collect();
        let msg = mumble_tcp::UserList { users };
        CommandOutput {
            tcp_messages: vec![ControlMessage::UserList(msg)],
            ..Default::default()
        }
    }
}
