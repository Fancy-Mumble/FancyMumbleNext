/// Re-exports for generated Mumble TCP protocol messages.
#[allow(clippy::doc_markdown)]
pub mod mumble_tcp {
    include!("mumble_proto.rs");
}

/// Re-exports for generated Mumble UDP protocol messages.
#[allow(clippy::doc_markdown)]
pub mod mumble_udp {
    include!("mumble_udp.rs");
}
