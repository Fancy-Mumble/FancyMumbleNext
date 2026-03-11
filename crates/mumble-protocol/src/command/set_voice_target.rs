use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Register a voice target for whisper/shout.
#[derive(Debug)]
pub struct SetVoiceTarget {
    pub id: u32,
    pub targets: Vec<VoiceTargetEntry>,
}

/// A single entry for a voice target (whisper/shout).
#[derive(Debug, Clone)]
pub struct VoiceTargetEntry {
    pub sessions: Vec<u32>,
    pub channel_id: Option<u32>,
    pub group: Option<String>,
    pub links: bool,
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
