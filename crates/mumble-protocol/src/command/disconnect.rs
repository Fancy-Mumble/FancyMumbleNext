use crate::command::core::{CommandAction, CommandOutput};
use crate::state::ServerState;

/// Disconnect from the server gracefully.
#[derive(Debug)]
pub struct Disconnect;

impl CommandAction for Disconnect {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        CommandOutput {
            disconnect: true,
            ..Default::default()
        }
    }
}
