//! Watch-together (`FancyWatchSync`) inbound handler.
//!
//! Translates the protobuf oneof into a tagged-union JSON payload and
//! emits it to the frontend as the `watch-sync` event.

use mumble_protocol::proto::mumble_tcp;
use serde::Serialize;
use tracing::debug;

use super::{HandleMessage, HandlerContext};

/// Frontend-facing tagged union mirroring [`fancy_watch_sync::Event`].
#[derive(Serialize, Clone)]
#[serde(tag = "type", rename_all = "camelCase")]
pub(crate) enum WatchSyncEvent {
    #[serde(rename_all = "camelCase")]
    Start {
        channel_id: Option<u32>,
        source_url: Option<String>,
        source_kind: Option<String>,
        title: Option<String>,
        host_session: Option<u32>,
    },
    #[serde(rename_all = "camelCase")]
    State {
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

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WatchSyncPayload {
    pub session_id: Option<String>,
    pub actor: Option<u32>,
    pub event: WatchSyncEvent,
}

impl HandleMessage for mumble_tcp::FancyWatchSync {
    fn handle(&self, ctx: &HandlerContext) {
        let Some(event) = self.event.as_ref() else {
            debug!("watch-sync dropped: no event payload");
            return;
        };
        let Some(payload) = into_payload(self.session_id.clone(), self.actor, event) else {
            return;
        };
        debug!(
            session_id = ?payload.session_id,
            actor = ?payload.actor,
            "watch-sync event received"
        );
        ctx.emit("watch-sync", payload);
    }
}

fn into_payload(
    session_id: Option<String>,
    actor: Option<u32>,
    event: &mumble_tcp::fancy_watch_sync::Event,
) -> Option<WatchSyncPayload> {
    use mumble_tcp::fancy_watch_sync::{Event, PlaybackState, SourceKind};

    let event = match event {
        Event::Start(s) => WatchSyncEvent::Start {
            channel_id: s.channel_id,
            source_url: s.source_url.clone(),
            source_kind: s.source_kind.and_then(|v| {
                SourceKind::try_from(v).ok().map(source_kind_to_str)
            }),
            title: s.title.clone(),
            host_session: s.host_session,
        },
        Event::State(s) => WatchSyncEvent::State {
            state: s.state.and_then(|v| {
                PlaybackState::try_from(v).ok().map(playback_state_to_str)
            }),
            current_time: s.current_time,
            updated_at_ms: s.updated_at_ms,
            host_session: s.host_session,
        },
        Event::Join(m) => WatchSyncEvent::Join { session: m.session },
        Event::Leave(m) => WatchSyncEvent::Leave { session: m.session },
        Event::StateRequest(_) => WatchSyncEvent::StateRequest,
        Event::End(_) => WatchSyncEvent::End,
        Event::HostTransfer(t) => WatchSyncEvent::HostTransfer {
            new_host_session: t.new_host_session,
        },
    };

    Some(WatchSyncPayload {
        session_id,
        actor,
        event,
    })
}

fn source_kind_to_str(k: mumble_tcp::fancy_watch_sync::SourceKind) -> String {
    use mumble_tcp::fancy_watch_sync::SourceKind;
    match k {
        SourceKind::DirectMedia => "directMedia".into(),
        SourceKind::Youtube => "youtube".into(),
    }
}

fn playback_state_to_str(s: mumble_tcp::fancy_watch_sync::PlaybackState) -> String {
    use mumble_tcp::fancy_watch_sync::PlaybackState;
    match s {
        PlaybackState::Paused => "paused".into(),
        PlaybackState::Playing => "playing".into(),
        PlaybackState::Ended => "ended".into(),
    }
}
