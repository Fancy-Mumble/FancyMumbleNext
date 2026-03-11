#![allow(unused_qualifications)]
//! Server connection form - IP, port, username.
use dioxus::prelude::*;

use crate::services::mumble_backend::MumbleBackend;
use crate::services::{ConnectionStatus, MumbleService};
use crate::state::AppState;

/// The connect page rendered when not connected to any server.
#[component]
pub fn ConnectPage() -> Element {
    let mut app = use_context::<Signal<AppState>>();
    let backend = use_context::<Signal<MumbleBackend>>();

    let on_connect = move |_| {
        let backend = backend.read().clone();
        let host = app.read().server_host.clone();
        let port_str = app.read().server_port.clone();
        let username = app.read().username.clone();

        spawn(async move {
            let port: u16 = match port_str.parse() {
                Ok(p) => p,
                Err(_) => {
                    app.write().error = Some("Invalid port number".into());
                    return;
                }
            };

            if host.is_empty() || username.is_empty() {
                app.write().error = Some("Host and username are required".into());
                return;
            }

            app.write().status = ConnectionStatus::Connecting;
            app.write().error = None;

            // Start the real protocol connection.
            // The event bridge coroutine in App will handle the
            // Connected / Rejected events and transition the page.
            if let Err(e) = backend.connect(host, port, username).await {
                let mut state = app.write();
                state.status = ConnectionStatus::Disconnected;
                state.error = Some(e);
            }
        });
    };

    rsx! {
        div { class: "connect-page",
            div { class: "connect-card",
                div { class: "connect-logo",
                    svg {
                        width: "48",
                        height: "48",
                        view_box: "0 0 24 24",
                        fill: "none",
                        xmlns: "http://www.w3.org/2000/svg",
                        path {
                            d: "M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-1 14.5v-9l7 4.5-7 4.5z",
                            fill: "url(#grad)",
                        }
                        defs {
                            linearGradient {
                                id: "grad",
                                x1: "0",
                                y1: "0",
                                x2: "1",
                                y2: "1",
                                stop { offset: "0%", stop_color: "#818cf8" }
                                stop { offset: "100%", stop_color: "#c084fc" }
                            }
                        }
                    }
                }
                h1 { class: "connect-title", "Mumble" }
                p { class: "connect-subtitle", "Connect to a voice server" }

                if let Some(err) = &app.read().error {
                    div { class: "error-banner",
                        span { class: "error-icon", "!" }
                        span { "{err}" }
                    }
                }

                div { class: "form-group",
                    label { r#for: "host", "Server Address" }
                    input {
                        id: "host",
                        r#type: "text",
                        placeholder: "mumble.example.com",
                        value: "{app.read().server_host}",
                        oninput: move |e: Event<FormData>| {
                            app.write().server_host = e.value();
                        },
                    }
                }

                div { class: "form-group",
                    label { r#for: "port", "Port" }
                    input {
                        id: "port",
                        r#type: "text",
                        placeholder: "64738",
                        value: "{app.read().server_port}",
                        oninput: move |e: Event<FormData>| {
                            app.write().server_port = e.value();
                        },
                    }
                }

                div { class: "form-group",
                    label { r#for: "user", "Username" }
                    input {
                        id: "user",
                        r#type: "text",
                        placeholder: "Your nickname",
                        value: "{app.read().username}",
                        oninput: move |e: Event<FormData>| {
                            app.write().username = e.value();
                        },
                    }
                }

                button {
                    class: "connect-btn",
                    disabled: app.read().status == ConnectionStatus::Connecting,
                    onclick: on_connect,
                    if app.read().status == ConnectionStatus::Connecting {
                        span { class: "spinner" }
                        "Connecting…"
                    } else {
                        "Connect"
                    }
                }
            }
        }
    }
}
