use crate::command::core::{CommandAction, CommandOutput};
use crate::message::ControlMessage;
use crate::proto::mumble_tcp;
use crate::state::ServerState;

/// Request link preview metadata from the server for one or more URLs.
#[derive(Debug)]
pub struct RequestLinkPreview {
    /// URLs to fetch previews for.
    pub urls: Vec<String>,
    /// Client-chosen correlation ID to match the response.
    pub request_id: String,
}

impl CommandAction for RequestLinkPreview {
    fn execute(&self, _state: &ServerState) -> CommandOutput {
        CommandOutput {
            tcp_messages: vec![ControlMessage::FancyLinkPreviewRequest(
                mumble_tcp::FancyLinkPreviewRequest {
                    urls: self.urls.clone(),
                    request_id: Some(self.request_id.clone()),
                },
            )],
            ..Default::default()
        }
    }
}
