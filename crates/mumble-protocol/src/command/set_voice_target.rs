use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Register a voice target for whisper/shout.
#[derive(Debug)]
pub struct SetVoiceTarget {
    /// Voice target slot ID (1-30).
    pub id: u32,
    /// Entries describing who to whisper/shout to.
    pub targets: Vec<VoiceTargetEntry>,
}

/// A single entry for a voice target (whisper/shout).
#[derive(Debug, Clone)]
pub struct VoiceTargetEntry {
    /// Specific user sessions to target.
    pub sessions: Vec<u32>,
    /// Channel to target (all users in it).
    pub channel_id: Option<u32>,
    /// ACL group to target.
    pub group: Option<String>,
    /// Whether to include users in linked channels.
    pub links: bool,
    /// Whether to include users in child channels.
    pub children: bool,
}

impl CommandAction for SetVoiceTarget {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        let targets = self
            .targets
            .iter()
            .map(|t| mumble_tcp::voice_target::Target {
                session: t.sessions.clone(),
                channel_id: t.channel_id,
                group: t.group.clone(),
                links: if t.links { Some(true) } else { None },
                children: if t.children { Some(true) } else { None },
            })
            .collect();

        let msg = mumble_tcp::VoiceTarget {
            id: Some(self.id),
            targets,
        };
        CommandOutput {
            tcp_messages: vec![ControlMessage::VoiceTarget(msg)],
            ..Default::default()
        }
    }
}
