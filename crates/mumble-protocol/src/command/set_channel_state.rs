use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::{PchatMode, ServerState};

/// Update or create a channel on the server.
///
/// For **editing** an existing channel, set `channel_id` to `Some(id)`.
/// For **creating** a new sub-channel, set `channel_id` to `None` and
/// `parent` to `Some(parent_id)`.
///
/// Only fields set to `Some(...)` are included in the message;
/// the server ignores absent fields.  The caller must ensure the
/// user has the required permissions (Write / `MakeChannel`) before
/// sending.
#[derive(Debug)]
pub struct SetChannelState {
    /// Target channel ID.  `None` when creating a new channel.
    pub channel_id: Option<u32>,
    /// Parent channel ID (required when creating a new channel).
    pub parent: Option<u32>,
    /// New channel name.
    pub name: Option<String>,
    /// New channel description (HTML).
    pub description: Option<String>,
    /// Display order hint for the channel in the tree.
    pub position: Option<i32>,
    /// Whether the channel is temporary (auto-deleted when empty).
    pub temporary: Option<bool>,
    /// Maximum number of users allowed in the channel (0 = unlimited).
    pub max_users: Option<u32>,
    /// Persistent-chat mode for this channel.
    pub pchat_mode: Option<PchatMode>,
    /// Max stored messages (0 = unlimited).
    pub pchat_max_history: Option<u32>,
    /// Auto-delete after N days (0 = forever).
    pub pchat_retention_days: Option<u32>,
}

impl CommandAction for SetChannelState {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        let msg = mumble_tcp::ChannelState {
            channel_id: self.channel_id,
            parent: self.parent,
            name: self.name.clone(),
            description: self.description.clone(),
            position: self.position,
            temporary: self.temporary,
            max_users: self.max_users,
            pchat_mode: self.pchat_mode.map(PchatMode::to_proto),
            pchat_max_history: self.pchat_max_history,
            pchat_retention_days: self.pchat_retention_days,
            ..Default::default()
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::ChannelState(msg)],
            ..Default::default()
        }
    }
}
