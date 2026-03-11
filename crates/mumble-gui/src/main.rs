//! Mumble desktop client - Dioxus entry point.

mod components;
mod services;
mod state;

use dioxus::prelude::*;

use components::channel_sidebar::ChannelSidebar;
use components::chat_view::ChatView;
use components::connect_page::ConnectPage;
use services::mumble_backend::MumbleBackend;
use services::{ConnectionStatus, MumbleService, ServerEvent};
use state::{AppState, Page};

const STYLE: Asset = asset!("assets/style.css");

fn main() {
    // Install the ring TLS crypto provider before anything touches rustls.
    let _ = rustls::crypto::ring::default_provider().install_default();
    tracing_subscriber::fmt::init();
    launch(App);
}

#[component]
fn App() -> Element {
    // Provide shared state to all components.
    let mut app = use_context_provider(|| Signal::new(AppState::default()));
    let backend = use_context_provider(|| Signal::new(MumbleBackend::new()));

    // Spawn event bridge coroutine - takes the event receiver once and
    // pushes server updates into the reactive AppState signal.
    use_hook(move || {
        let event_rx = backend.read().take_event_receiver();
        if let Some(mut rx) = event_rx {
            spawn(async move {
                while let Some(event) = rx.recv().await {
                    match event {
                        ServerEvent::Connected => {
                            let channels = backend.read().channels();
                            let users = backend.read().users();
                            let first_channel = channels.first().map(|c| c.id);

                            let mut state = app.write();
                            state.status = ConnectionStatus::Connected;
                            state.channels = channels;
                            state.users = users;
                            state.page = Page::Chat;
                            state.selected_channel = first_channel;
                            state.error = None;

                            if let Some(ch_id) = first_channel {
                                state.messages = backend.read().messages(ch_id);
                            }
                        }

                        ServerEvent::Disconnected => {
                            let mut state = app.write();
                            state.status = ConnectionStatus::Disconnected;
                            state.page = Page::Connect;
                            state.channels.clear();
                            state.users.clear();
                            state.messages.clear();
                            state.selected_channel = None;
                        }

                        ServerEvent::StateChanged => {
                            let channels = backend.read().channels();
                            let users = backend.read().users();

                            let mut state = app.write();
                            state.channels = channels;
                            state.users = users;
                        }

                        ServerEvent::NewMessage { channel_id } => {
                            let selected = app.read().selected_channel;
                            if selected == Some(channel_id) {
                                let msgs = backend.read().messages(channel_id);
                                app.write().messages = msgs;
                            }
                        }

                        ServerEvent::Rejected { reason } => {
                            let mut state = app.write();
                            state.status = ConnectionStatus::Disconnected;
                            state.page = Page::Connect;
                            state.error = Some(reason);
                        }
                    }
                }
            });
        }
    });

    rsx! {
        document::Link { rel: "stylesheet", href: STYLE }
        match app.read().page {
            Page::Connect => rsx! { ConnectPage {} },
            Page::Chat => rsx! {
                div { class: "app-layout",
                    ChannelSidebar {}
                    ChatView {}
                }
            },
        }
    }
}
