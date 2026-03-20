use crate::command::core::{CommandAction, CommandOutput};
use crate::message::UdpMessage;
use crate::proto::mumble_udp;
use crate::state::ServerState;

/// Send encoded audio data to the server.
#[derive(Debug)]
pub struct SendAudio {
    /// Opus-compressed audio payload.
    pub opus_data: Vec<u8>,
    /// Voice target ID (0 = normal, others = whisper/shout targets).
    pub target: u32,
    /// Monotonically increasing frame sequence number.
    pub frame_number: u64,
    /// Optional 3D position of the speaker (x, y, z).
    pub positional_data: Option<[f32; 3]>,
    /// When `true`, this is the final frame of a speech segment.
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
