#![allow(unused_qualifications)]
//! Left sidebar - channel list with user counts and active highlighting.
use dioxus::prelude::*;

use crate::services::mumble_backend::MumbleBackend;
use crate::services::MumbleService;
use crate::state::AppState;

/// Generates a stable accent colour from a channel name.
fn channel_color(name: &str) -> &'static str {
    const PALETTE: &[&str] = &[
        "#818cf8", "#34d399", "#fbbf24",
        "#60a5fa", "#a78bfa", "#f472b6",
        "#22d3ee", "#fb923c",
    ];
    let hash = name.bytes().fold(0u32, |acc, b| acc.wrapping_add(b as u32));
    PALETTE[hash as usize % PALETTE.len()]
}

/// First letter(s) used as the channel avatar.
fn channel_initials(name: &str) -> String {
    name.chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default()
}

#[component]
pub fn ChannelSidebar() -> Element {
    let mut app = use_context::<Signal<AppState>>();
    let backend = use_context::<Signal<MumbleBackend>>();

    let channels = app.read().channels.clone();
    let users = app.read().users.clone();
    let selected = app.read().selected_channel;
    let username = app.read().username.clone();

    let on_disconnect = move |_| {
        let backend = backend.read().clone();
        spawn(async move {
            let _ = backend.disconnect().await;
        });
    };

    rsx! {
        div { class: "sidebar",
            // Header with server info and disconnect
            div { class: "sidebar-header",
                div { class: "sidebar-header-info",
                    h2 { "Channels" }
                    span { class: "sidebar-user-badge", "{username}" }
                }
                button {
                    class: "disconnect-btn",
                    title: "Disconnect",
                    onclick: on_disconnect,
                    // × icon
                    "\u{2715}"
                }
            }

            // Channel list
            div { class: "channel-list",
                for ch in channels.iter() {
                    {
                        let ch_id = ch.id;
                        let ch_name = ch.name.clone();
                        let is_active = selected == Some(ch_id);
                        let bg = channel_color(&ch_name);
                        let initials = channel_initials(&ch_name);
                        let user_count = ch.user_count;
                        let user_label = if user_count != 1 {
                            format!("{user_count} users")
                        } else {
                            format!("{user_count} user")
                        };
                        rsx! {
                            div {
                                class: if is_active { "channel-item active" } else { "channel-item" },
                                onclick: move |_| {
                                    let msgs = backend.read().messages(ch_id);
                                    let mut state = app.write();
                                    state.selected_channel = Some(ch_id);
                                    state.messages = msgs;
                                },

                                // Round avatar
                                div {
                                    class: "channel-avatar",
                                    style: "background: {bg};",
                                    "{initials}"
                                }

                                // Name + user count
                                div { class: "channel-info",
                                    span { class: "channel-name", "{ch_name}" }
                                    span { class: "channel-meta", "{user_label}" }
                                }

                                // Active indicator dot
                                if is_active {
                                    div { class: "active-indicator" }
                                }
                            }
                        }
                    }
                }
            }

            // User list at bottom
            div { class: "sidebar-users",
                div { class: "sidebar-users-header",
                    {
                        let count_label = format!("Online - {}", users.len());
                        rsx! { span { "{count_label}" } }
                    }
                }
                for user in users.iter() {
                    {
                        let name = user.name.clone();
                        let ini = name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
                        let bg = channel_color(&name);
                        rsx! {
                            div { class: "user-item",
                                div {
                                    class: "user-avatar-small",
                                    style: "background: {bg};",
                                    "{ini}"
                                }
                                span { class: "user-name", "{name}" }
                                span { class: "online-dot" }
                            }
                        }
                    }
                }
            }
        }
    }
}
