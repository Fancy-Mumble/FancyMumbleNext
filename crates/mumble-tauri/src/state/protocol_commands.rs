//! Protocol-level commands: plugin data, push notifications, typing
//! indicators, read receipts, WebRTC signaling, reactions, pins, and
//! persistent-chat deletion.

use mumble_protocol::command;
use mumble_protocol::proto::mumble_tcp;
use serde::Deserialize;

use super::types::DeleteAckResult;
use super::AppState;

/// Parameters for a single drawing-stroke packet sent to the server.
///
/// Grouped into a struct to satisfy the `clippy::too_many_arguments` lint
/// on both [`AppState::send_draw_stroke`] and the Tauri command wrapper.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrawStrokeArgs {
    pub channel_id: u32,
    pub stroke_id: String,
    pub color: u32,
    pub width: f32,
    /// Width as a fraction of the shared content's pixel height.
    /// Optional for backwards compatibility with older clients.
    #[serde(default)]
    pub width_frac: Option<f32>,
    pub points: Vec<f32>,
    pub is_end: bool,
    pub is_clear: bool,
    /// When set with `is_clear`, wipes ALL strokes in the channel
    /// (every sender).  Reserved for the active screen-sharer.
    #[serde(default)]
    pub clear_all: bool,
}

/// Frontend-facing tagged-union mirror of
/// [`mumble_tcp::fancy_watch_sync::Event`].
///
/// Lives next to [`AppState::send_watch_sync`] (the only consumer) so
/// that the JSON shape stays in lock-step with the conversion below.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WatchSyncEventArg {
    #[serde(rename_all = "camelCase")]
    Start {
        channel_id: Option<u32>,
        source_url: Option<String>,
        /// `"directMedia"` or `"youtube"`.
        source_kind: Option<String>,
        title: Option<String>,
        host_session: Option<u32>,
    },
    #[serde(rename_all = "camelCase")]
    State {
        /// `"paused"`, `"playing"`, or `"ended"`.
        state: Option<String>,
        current_time: Option<f64>,
        updated_at_ms: Option<u64>,
        host_session: Option<u32>,
    },
    #[serde(rename_all = "camelCase")]
    Join { session: Option<u32> },
    #[serde(rename_all = "camelCase")]
    Leave { session: Option<u32> },
    StateRequest,
    End,
    #[serde(rename_all = "camelCase")]
    HostTransfer { new_host_session: Option<u32> },
}

impl WatchSyncEventArg {
    fn into_proto(self) -> mumble_tcp::fancy_watch_sync::Event {
        use mumble_tcp::fancy_watch_sync::{
            Event, HostTransfer, Member, Start, State,
            StateRequest as PStateRequest, End as PEnd,
        };
        match self {
            Self::Start {
                channel_id,
                source_url,
                source_kind,
                title,
                host_session,
            } => Event::Start(Start {
                channel_id,
                source_url,
                source_kind: source_kind.and_then(parse_source_kind).map(|k| k as i32),
                title,
                host_session,
            }),
            Self::State {
                state,
                current_time,
                updated_at_ms,
                host_session,
            } => Event::State(State {
                state: state.and_then(parse_playback_state).map(|s| s as i32),
                current_time,
                updated_at_ms,
                host_session,
            }),
            Self::Join { session } => Event::Join(Member { session }),
            Self::Leave { session } => Event::Leave(Member { session }),
            Self::StateRequest => Event::StateRequest(PStateRequest {}),
            Self::End => Event::End(PEnd {}),
            Self::HostTransfer { new_host_session } => {
                Event::HostTransfer(HostTransfer { new_host_session })
            }
        }
    }
}

fn parse_source_kind(s: String) -> Option<mumble_tcp::fancy_watch_sync::SourceKind> {
    use mumble_tcp::fancy_watch_sync::SourceKind;
    match s.as_str() {
        "directMedia" => Some(SourceKind::DirectMedia),
        "youtube" => Some(SourceKind::Youtube),
        _ => None,
    }
}

fn parse_playback_state(s: String) -> Option<mumble_tcp::fancy_watch_sync::PlaybackState> {
    use mumble_tcp::fancy_watch_sync::PlaybackState;
    match s.as_str() {
        "paused" => Some(PlaybackState::Paused),
        "playing" => Some(PlaybackState::Playing),
        "ended" => Some(PlaybackState::Ended),
        _ => None,
    }
}

impl AppState {
    pub async fn send_plugin_data(
        &self,
        receiver_sessions: Vec<u32>,
        data: Vec<u8>,
        data_id: String,
    ) -> Result<(), String> {
        let handle = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };

        let handle = handle.ok_or("Not connected")?;

        handle
            .send(command::SendPluginData {
                receiver_sessions,
                data,
                data_id,
            })
            .await
            .map_err(|e| format!("Failed to send plugin data: {e}"))?;

        Ok(())
    }

    pub async fn send_push_update(&self, muted_channels: Vec<u32>) -> Result<(), String> {
        let handle = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };

        let handle = handle.ok_or("Not connected")?;

        handle
            .send(command::SendFancyPushUpdate {
                muted_channels: muted_channels.clone(),
            })
            .await
            .map_err(|e| format!("Failed to send push update: {e}"))?;

        handle
            .send(command::SendFancySubscribePush {
                muted_channels,
            })
            .await
            .map_err(|e| format!("Failed to send subscribe push update: {e}"))?;

        Ok(())
    }

    pub async fn send_subscribe_push(
        &self,
        muted_channels: Vec<u32>,
    ) -> Result<(), String> {
        let handle = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };

        let handle = handle.ok_or("Not connected")?;

        handle
            .send(command::SendFancySubscribePush { muted_channels })
            .await
            .map_err(|e| format!("Failed to send subscribe push: {e}"))?;

        Ok(())
    }

    pub async fn send_typing_indicator(&self, channel_id: u32) -> Result<(), String> {
        let handle = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };

        let handle = handle.ok_or("Not connected")?;

        handle
            .send(command::SendTypingIndicator { channel_id })
            .await
            .map_err(|e| format!("Failed to send typing indicator: {e}"))?;

        Ok(())
    }

    /// Send a single watch-together event.  See [`WatchSyncEventArg`]
    /// for the JSON shape accepted from the frontend.
    pub async fn send_watch_sync(
        &self,
        session_id: String,
        event: WatchSyncEventArg,
    ) -> Result<(), String> {
        let handle = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };
        let handle = handle.ok_or("Not connected")?;

        let message = mumble_tcp::FancyWatchSync {
            session_id: Some(session_id),
            actor: None, // Server fills this in on relay.
            event: Some(event.into_proto()),
        };

        handle
            .send(command::SendWatchSync { message })
            .await
            .map_err(|e| format!("Failed to send watch-sync: {e}"))?;

        Ok(())
    }

    pub async fn send_draw_stroke(&self, args: DrawStrokeArgs) -> Result<(), String> {
        let handle = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };

        let handle = handle.ok_or("Not connected")?;

        tracing::info!(
            target: "draw",
            channel_id = args.channel_id,
            stroke_id = %args.stroke_id,
            coords = args.points.len(),
            is_end = args.is_end,
            is_clear = args.is_clear,
            clear_all = args.clear_all,
            width_frac = ?args.width_frac,
            "tx send_draw_stroke"
        );

        handle
            .send(command::SendDrawStroke {
                channel_id: args.channel_id,
                stroke_id: args.stroke_id,
                color: args.color,
                width: args.width,
                width_frac: args.width_frac,
                points: args.points,
                is_end: args.is_end,
                is_clear: args.is_clear,
                clear_all: args.clear_all,
            })
            .await
            .map_err(|e| format!("Failed to send draw stroke: {e}"))?;

        Ok(())
    }

    pub async fn request_link_preview(
        &self,
        urls: Vec<String>,
        request_id: String,
    ) -> Result<(), String> {
        let handle = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };

        let handle = handle.ok_or("Not connected")?;

        handle
            .send(command::RequestLinkPreview { urls, request_id })
            .await
            .map_err(|e| format!("Failed to request link preview: {e}"))?;

        Ok(())
    }

    pub async fn send_read_receipt(
        &self,
        channel_id: u32,
        last_read_message_id: String,
    ) -> Result<(), String> {
        let handle = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };

        let handle = handle.ok_or("Not connected")?;

        handle
            .send(command::SendReadReceipt {
                channel_id,
                last_read_message_id: Some(last_read_message_id),
                query: false,
                query_message_id: None,
            })
            .await
            .map_err(|e| format!("Failed to send read receipt: {e}"))?;

        Ok(())
    }

    pub async fn query_read_receipts(&self, channel_id: u32) -> Result<(), String> {
        let handle = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };

        let handle = handle.ok_or("Not connected")?;

        handle
            .send(command::SendReadReceipt {
                channel_id,
                last_read_message_id: None,
                query: true,
                query_message_id: None,
            })
            .await
            .map_err(|e| format!("Failed to query read receipts: {e}"))?;

        Ok(())
    }

    pub async fn send_webrtc_signal(
        &self,
        target_session: u32,
        signal_type: i32,
        payload: String,
        server_id: Option<String>,
    ) -> Result<(), String> {
        let handle = {
            // When the caller specifies a server_id, send via THAT
            // connection's client handle (not the active session's).
            // This is essential when multiple server tabs are open in
            // the same window: WebRTC ICE candidates trickle in async
            // and must always be delivered through the connection that
            // owns the peer connection, regardless of which tab the
            // user is currently looking at.
            let arc = if let Some(sid_str) = server_id.as_deref() {
                let sid: super::sessions::ServerId = sid_str
                    .parse()
                    .map_err(|e| format!("invalid server_id: {e}"))?;
                self.registry
                    .session(sid)
                    .ok_or_else(|| format!("unknown server_id: {sid_str}"))?
            } else {
                self.inner.snapshot()
            };
            let state = arc.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };

        let handle = handle.ok_or("Not connected")?;

        handle
            .send(command::SendWebRtcSignal {
                target_session,
                signal_type,
                payload,
            })
            .await
            .map_err(|e| format!("Failed to send WebRTC signal: {e}"))?;

        Ok(())
    }

    pub async fn send_reaction(
        &self,
        channel_id: u32,
        message_id: String,
        emoji: String,
        action: String,
    ) -> Result<(), String> {
        let handle = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };

        let handle = handle.ok_or("Not connected")?;

        let reaction_action = match action.as_str() {
            "remove" => mumble_tcp::ReactionAction::ReactionRemove as i32,
            _ => mumble_tcp::ReactionAction::ReactionAdd as i32,
        };

        let emoji_oneof = if emoji.starts_with(':') && emoji.ends_with(':') && emoji.len() > 2 {
            let shortcode = emoji[1..emoji.len() - 1].to_owned();
            Some(
                mumble_tcp::pchat_reaction::Emoji::ServerEmoji(
                    mumble_tcp::ServerEmoji {
                        shortcode: Some(shortcode.into_bytes()),
                    },
                ),
            )
        } else {
            Some(
                mumble_tcp::pchat_reaction::Emoji::UnicodeEmoji(
                    mumble_tcp::UnicodeEmoji {
                        grapheme: Some(emoji),
                    },
                ),
            )
        };

        let msg = mumble_tcp::PchatReaction {
            channel_id: Some(channel_id),
            message_id: Some(message_id),
            emoji: emoji_oneof,
            action: Some(reaction_action),
            sender_hash: None,
            timestamp: None,
        };

        handle
            .send(command::SendPchatReaction { message: msg })
            .await
            .map_err(|e| format!("Failed to send reaction: {e}"))?;

        Ok(())
    }

    pub async fn pin_message(
        &self,
        channel_id: u32,
        message_id: String,
        unpin: bool,
    ) -> Result<(), String> {
        let handle = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };

        let handle = handle.ok_or("Not connected")?;

        let msg = mumble_tcp::PchatPin {
            channel_id: Some(channel_id),
            message_id: Some(message_id),
            unpin: Some(unpin),
            sender_hash: None,
            timestamp: None,
        };

        handle
            .send(command::SendPchatPin { message: msg })
            .await
            .map_err(|e| format!("Failed to send pin: {e}"))?;

        Ok(())
    }

    pub async fn delete_pchat_messages(
        &self,
        channel_id: u32,
        message_ids: Vec<String>,
        time_from: Option<u64>,
        time_to: Option<u64>,
        sender_hash: Option<String>,
    ) -> Result<(), String> {
        let (handle, rx) = {
            let __session = self.inner.snapshot();
            let mut state = __session.lock().map_err(|e| e.to_string())?;
            let h = state.conn.client_handle.clone().ok_or("Not connected")?;

            let (tx, rx) = tokio::sync::oneshot::channel::<DeleteAckResult>();
            state.pchat_ctx.pending_delete_acks.push(tx);
            (h, rx)
        };

        let time_range = if time_from.is_some() || time_to.is_some() {
            Some(mumble_tcp::pchat_delete_messages::TimeRange {
                from: time_from,
                to: time_to,
            })
        } else {
            None
        };

        handle
            .send(command::SendPchatDeleteMessages {
                message: mumble_tcp::PchatDeleteMessages {
                    channel_id: Some(channel_id),
                    message_ids,
                    time_range,
                    sender_hash,
                },
            })
            .await
            .map_err(|e| format!("Failed to send pchat delete: {e}"))?;

        match tokio::time::timeout(std::time::Duration::from_secs(15), rx).await {
            Ok(Ok(ack)) if ack.success => Ok(()),
            Ok(Ok(ack)) => Err(format!(
                "Server rejected deletion: {}",
                ack.reason.unwrap_or_else(|| "permission denied".to_string())
            )),
            Ok(Err(_)) => Err("Delete acknowledgement channel closed".to_string()),
            Err(_) => Err("Delete request timed out".to_string()),
        }
    }
}
