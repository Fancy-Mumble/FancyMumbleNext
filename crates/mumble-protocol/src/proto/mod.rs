//! Generated protobuf message types for the Mumble protocol.
//!
//! The two sub-modules are populated by `prost-build` at build time
//! from `proto/Mumble.proto` and `proto/MumbleUDP.proto`.

/// Re-exports for generated Mumble TCP protocol messages.
#[allow(
    missing_docs,
    missing_debug_implementations,
    unused_results,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::doc_markdown,
    reason = "generated code produced by prost-build - not hand-written"
)]
pub mod mumble_tcp {
    include!("mumble_proto.rs");
}

/// Re-exports for generated Mumble UDP protocol messages.
#[allow(
    missing_docs,
    missing_debug_implementations,
    unused_results,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::doc_markdown,
    reason = "generated code produced by prost-build - not hand-written"
)]
pub mod mumble_udp {
    include!("mumble_udp.rs");
}
