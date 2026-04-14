//! Trait for reacting to server events.
//!
//! Implement [`EventHandler`] and supply it to the client to receive
//! callbacks when users connect, channels change, audio arrives, etc.

use crate::message::{ControlMessage, UdpMessage};
use crate::transport::ocb2::PacketStats;

/// Callback interface for server events.
///
/// All methods have default no-op implementations so you only need
/// to override the ones you care about.
#[allow(unused_variables, reason = "default no-op implementations intentionally ignore all parameters")]
pub trait EventHandler: Send + 'static {
    /// Called for every inbound TCP control message.
    fn on_control_message(&mut self, msg: &ControlMessage) {}

    /// Called for every inbound UDP message (audio or ping).
    fn on_udp_message(&mut self, msg: &UdpMessage) {}

    /// Called when the connection has been fully established (`ServerSync` received).
    fn on_connected(&mut self) {}

    /// Called when the connection is lost or shut down.
    fn on_disconnected(&mut self) {}

    /// Called when the audio transport mode changes (e.g. UDP activated or fell back to TCP).
    fn on_audio_transport_changed(&mut self, udp_active: bool) {}

    /// Called on each Ping exchange with updated packet statistics.
    ///
    /// `from_client` is our local decrypt stats (packets we received).
    /// `to_client` is the server's stats echoed back in its Ping reply
    ///   (packets it sent to us).
    fn on_ping_stats(&mut self, from_client: PacketStats, to_client: PacketStats) {}
}

/// A no-op handler for use in tests or headless mode.
#[derive(Debug, Default)]
pub struct NoopEventHandler;

impl EventHandler for NoopEventHandler {}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
    use super::*;
    use crate::message::{ControlMessage, UdpMessage};
    use crate::proto::{mumble_tcp, mumble_udp};
    use std::sync::{Arc, Mutex};

    /// A test handler that records events.
    struct RecordingHandler {
        control_count: Arc<Mutex<u32>>,
        udp_count: Arc<Mutex<u32>>,
        connected: Arc<Mutex<bool>>,
        disconnected: Arc<Mutex<bool>>,
    }

    impl RecordingHandler {
        fn new() -> Self {
            Self {
                control_count: Arc::new(Mutex::new(0)),
                udp_count: Arc::new(Mutex::new(0)),
                connected: Arc::new(Mutex::new(false)),
                disconnected: Arc::new(Mutex::new(false)),
            }
        }
    }

    impl EventHandler for RecordingHandler {
        fn on_control_message(&mut self, _msg: &ControlMessage) {
            *self.control_count.lock().unwrap() += 1;
        }
        fn on_udp_message(&mut self, _msg: &UdpMessage) {
            *self.udp_count.lock().unwrap() += 1;
        }
        fn on_connected(&mut self) {
            *self.connected.lock().unwrap() = true;
        }
        fn on_disconnected(&mut self) {
            *self.disconnected.lock().unwrap() = true;
        }
    }

    #[test]
    fn noop_handler_does_not_panic() {
        let mut handler = NoopEventHandler;
        handler.on_control_message(&ControlMessage::Ping(mumble_tcp::Ping::default()));
        handler.on_udp_message(&UdpMessage::Ping(mumble_udp::Ping::default()));
        handler.on_connected();
        handler.on_disconnected();
    }

    #[test]
    fn recording_handler_tracks_control_messages() {
        let mut handler = RecordingHandler::new();
        let count = handler.control_count.clone();

        handler.on_control_message(&ControlMessage::Ping(mumble_tcp::Ping::default()));
        handler.on_control_message(&ControlMessage::Ping(mumble_tcp::Ping::default()));

        assert_eq!(*count.lock().unwrap(), 2);
    }

    #[test]
    fn recording_handler_tracks_udp_messages() {
        let mut handler = RecordingHandler::new();
        let count = handler.udp_count.clone();

        handler.on_udp_message(&UdpMessage::Ping(mumble_udp::Ping::default()));
        assert_eq!(*count.lock().unwrap(), 1);
    }

    #[test]
    fn recording_handler_tracks_connection_lifecycle() {
        let mut handler = RecordingHandler::new();
        let connected = handler.connected.clone();
        let disconnected = handler.disconnected.clone();

        assert!(!*connected.lock().unwrap());
        assert!(!*disconnected.lock().unwrap());

        handler.on_connected();
        assert!(*connected.lock().unwrap());

        handler.on_disconnected();
        assert!(*disconnected.lock().unwrap());
    }
}
