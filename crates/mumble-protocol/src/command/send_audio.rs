use crate::command::core::{CommandAction, CommandOutput};
use crate::message::UdpMessage;
use crate::proto::mumble_udp;
use crate::state::ServerState;

/// Send encoded audio data to the server.
#[derive(Debug)]
pub struct SendAudio {
    pub opus_data: Vec<u8>,
    pub target: u32,
    pub frame_number: u64,
    pub positional_data: Option<[f32; 3]>,
    pub is_terminator: bool,
}

impl CommandAction for SendAudio {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        let audio = mumble_udp::Audio {
            header: Some(mumble_udp::audio::Header::Target(self.target)),
            sender_session: 0, // Server ignores this field from clients.
            frame_number: self.frame_number,
            opus_data: self.opus_data.clone(),
            positional_data: self
                .positional_data
                .map(|p| p.to_vec())
                .unwrap_or_default(),
            volume_adjustment: 0.0,
            is_terminator: self.is_terminator,
        };
        CommandOutput {
            udp_messages: vec![UdpMessage::Audio(audio)],
            ..Default::default()
        }
    }
}
