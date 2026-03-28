//! Network transport layer for the Mumble protocol.
//!
//! Provides TCP (control messages over TLS) and UDP (audio) transports,
//! plus wire-framing and audio codec helpers.
pub mod audio_codec;
pub mod codec;
pub mod ocb2;
pub mod tcp;
pub mod udp;
