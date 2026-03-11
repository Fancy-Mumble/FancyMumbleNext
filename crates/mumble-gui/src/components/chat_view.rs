#![allow(unused_qualifications)]
//! Chat pane - message bubbles with per-user avatars.
use dioxus::prelude::*;

use crate::services::mumble_backend::MumbleBackend;
use crate::services::MumbleService;
use crate::state::AppState;

/// Stable colour per user name.
fn user_color(name: &str) -> &'static str {
    const PALETTE: &[&str] = &[
        "#818cf8", "#34d399", "#fbbf24",
        "#60a5fa", "#a78bfa", "#f472b6",
        "#22d3ee", "#fb923c",
    ];
    let hash = name.bytes().fold(0u32, |acc, b| acc.wrapping_add(b as u32));
    PALETTE[hash as usize % PALETTE.len()]
}

fn user_initials(name: &str) -> String {
    name.chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default()
}

#[component]
pub fn ChatView() -> Element {
    let mut app = use_context::<Signal<AppState>>();
    let backend = use_context::<Signal<MumbleBackend>>();

    let selected = app.read().selected_channel;
    let messages = app.read().messages.clone();
    let users = app.read().users.clone();

    // Determine channel name for the header.
    let channel_name = selected
        .and_then(|id| {
            app.read()
                .channels
                .iter()
                .find(|c| c.id == id)
                .map(|c| c.name.clone())
        })
        .unwrap_or_else(|| "Select a channel".into());

    // Count users in this channel
    let channel_user_count = selected
        .map(|id| users.iter().filter(|u| u.channel_id == id).count())
        .unwrap_or(0);

    let channel_meta = if channel_user_count != 1 {
        format!("{channel_user_count} members")
    } else {
        "1 member".into()
    };

    let on_send = move |_| {
        let draft = app.read().message_draft.clone();
        if draft.trim().is_empty() {
            return;
        }
        if let Some(ch_id) = app.read().selected_channel {
            let backend = backend.read().clone();
            let body = draft.clone();
            spawn(async move {
                let _ = backend.send_message(ch_id, body).await;
                let msgs = backend.messages(ch_id);
                let mut state = app.write();
                state.messages = msgs;
                state.message_draft = String::new();
            });
        }
    };

    rsx! {
        div { class: "chat-pane",
            // --- Header ---
            div { class: "chat-header",
                div { class: "chat-header-info",
                    h2 { "# {channel_name}" }
                    span { class: "chat-header-meta", "{channel_meta}" }
                }
            }

            // --- Messages ---
            div { class: "chat-messages",
                if messages.is_empty() {
                    div { class: "chat-empty",
                        div { class: "chat-empty-icon", "\u{1F4AC}" }
                        p { "No messages yet" }
                        span { "Be the first to say something!" }
                    }
                }
                for msg in messages.iter() {
                    {
                        let bubble_class = if msg.is_own {
                            "message-row own"
                        } else {
                            "message-row"
                        };
                        let bg = user_color(&msg.sender_name);
                        let ini = user_initials(&msg.sender_name);
                        let sender = msg.sender_name.clone();
                        let body = msg.body.clone();
                        rsx! {
                            div { class: "{bubble_class}",
                                if !msg.is_own {
                                    div {
                                        class: "msg-avatar",
                                        style: "background: {bg};",
                                        "{ini}"
                                    }
                                }
                                div { class: "msg-bubble",
                                    if !msg.is_own {
                                        span { class: "msg-sender", style: "color: {bg};", "{sender}" }
                                    }
                                    p { class: "msg-body", "{body}" }
                                }
                            }
                        }
                    }
                }
            }

            // --- Composer ---
            div { class: "chat-composer",
                input {
                    class: "composer-input",
                    r#type: "text",
                    placeholder: "Type a message…",
                    value: "{app.read().message_draft}",
                    oninput: move |e: Event<FormData>| {
                        app.write().message_draft = e.value();
                    },
                    onkeypress: move |e: Event<KeyboardData>| {
                        if e.key() == Key::Enter {
                            on_send(());
                        }
                    },
                }
                button {
                    class: "send-btn",
                    onclick: move |_| on_send(()),
                    // Send arrow icon
                    svg {
                        width: "20",
                        height: "20",
                        view_box: "0 0 24 24",
                        fill: "currentColor",
                        path {
                            d: "M2.01 21L23 12 2.01 3 2 10l15 2-15 2z"
                        }
                    }
                }
            }
        }
    }
}
