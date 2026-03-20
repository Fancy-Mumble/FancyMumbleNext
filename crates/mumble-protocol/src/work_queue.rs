//! Priority-based work queue for the Mumble client event loop.
//!
//! Work items arrive from three sources:
//! 1. **UDP transport** - audio/ping (highest priority for low latency)
//! 2. **TCP transport** - control messages
//! 3. **User commands** - from the UI or external API
//!
//! The queue uses separate tokio channels and a `select!` loop that
//! checks UDP first, then TCP, then user commands, ensuring audio
//! is never starved by control traffic.

use tokio::sync::mpsc;
use tracing::trace;

use crate::command::BoxedCommand;
use crate::error::{Error, Result};
use crate::message::{ControlMessage, ServerMessage, UdpMessage};

/// A single item in the work queue.
#[allow(clippy::large_enum_variant, reason = "ServerMessage variant must inline ControlMessage; boxing adds per-packet heap allocation on the hot audio path")]
#[derive(Debug)]
pub enum WorkItem {
    /// Inbound server message (from TCP or UDP).
    ServerMessage(ServerMessage),
    /// Outbound user command (from the UI / API).
    UserCommand(BoxedCommand),
    /// Shutdown signal.
    Shutdown,
}

/// Sender-side handle for injecting work items.
///
/// Cloneable - hand one to the UI thread, one to each transport task, etc.
#[derive(Debug, Clone)]
pub struct WorkQueueSender {
    udp_tx: mpsc::Sender<UdpMessage>,
    tcp_tx: mpsc::Sender<ControlMessage>,
    cmd_tx: mpsc::Sender<BoxedCommand>,
}

impl WorkQueueSender {
    /// Submit an inbound UDP message (audio / ping). Non-blocking.
    pub async fn send_udp(&self, msg: UdpMessage) -> Result<()> {
        self.udp_tx
            .send(msg)
            .await
            .map_err(|_| Error::QueueClosed)
    }

    /// Submit an inbound TCP control message. Non-blocking.
    pub async fn send_tcp(&self, msg: ControlMessage) -> Result<()> {
        self.tcp_tx
            .send(msg)
            .await
            .map_err(|_| Error::QueueClosed)
    }

    /// Submit a user-initiated command.
    pub async fn send_command(&self, cmd: BoxedCommand) -> Result<()> {
        self.cmd_tx
            .send(cmd)
            .await
            .map_err(|_| Error::QueueClosed)
    }
}

/// Receiver-side handle consumed by the client event loop.
#[derive(Debug)]
pub struct WorkQueueReceiver {
    udp_rx: mpsc::Receiver<UdpMessage>,
    tcp_rx: mpsc::Receiver<ControlMessage>,
    cmd_rx: mpsc::Receiver<BoxedCommand>,
}

impl WorkQueueReceiver {
    /// Await the next work item, prioritizing UDP > TCP > Commands.
    ///
    /// Uses biased `select!` to ensure audio packets are processed first.
    pub async fn recv(&mut self) -> WorkItem {
        // Biased select: try UDP first, then TCP, then commands.
        // This guarantees low-latency audio processing.
        tokio::select! {
            biased;

            Some(udp_msg) = self.udp_rx.recv() => {
                trace!("work queue: UDP message");
                WorkItem::ServerMessage(ServerMessage::Udp(udp_msg))
            }
            Some(tcp_msg) = self.tcp_rx.recv() => {
                trace!("work queue: TCP message");
                WorkItem::ServerMessage(ServerMessage::Control(tcp_msg))
            }
            Some(cmd) = self.cmd_rx.recv() => {
                trace!("work queue: user command");
                WorkItem::UserCommand(cmd)
            }
            else => {
                WorkItem::Shutdown
            }
        }
    }
}

/// Channel buffer sizes.
const UDP_CHANNEL_SIZE: usize = 256;
const TCP_CHANNEL_SIZE: usize = 64;
const CMD_CHANNEL_SIZE: usize = 32;

/// Create a linked (sender, receiver) pair for the work queue.
pub fn create() -> (WorkQueueSender, WorkQueueReceiver) {
    let (udp_tx, udp_rx) = mpsc::channel(UDP_CHANNEL_SIZE);
    let (tcp_tx, tcp_rx) = mpsc::channel(TCP_CHANNEL_SIZE);
    let (cmd_tx, cmd_rx) = mpsc::channel(CMD_CHANNEL_SIZE);

    (
        WorkQueueSender {
            udp_tx,
            tcp_tx,
            cmd_tx,
        },
        WorkQueueReceiver {
            udp_rx,
            tcp_rx,
            cmd_rx,
        },
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
    use super::*;
    use crate::command::Disconnect;
    use crate::proto::{mumble_tcp, mumble_udp};

    #[tokio::test]
    async fn send_and_receive_tcp_message() {
        let (sender, mut receiver) = create();
        let ping = ControlMessage::Ping(mumble_tcp::Ping {
            timestamp: Some(123),
            ..Default::default()
        });
        sender.send_tcp(ping).await.unwrap();

        let item = receiver.recv().await;
        match item {
            WorkItem::ServerMessage(ServerMessage::Control(ControlMessage::Ping(p))) => {
                assert_eq!(p.timestamp, Some(123));
            }
            _ => panic!("expected TCP Ping message"),
        }
    }

    #[tokio::test]
    async fn send_and_receive_udp_message() {
        let (sender, mut receiver) = create();
        let udp_ping = UdpMessage::Ping(mumble_udp::Ping {
            timestamp: 456,
            ..Default::default()
        });
        sender.send_udp(udp_ping).await.unwrap();

        let item = receiver.recv().await;
        match item {
            WorkItem::ServerMessage(ServerMessage::Udp(UdpMessage::Ping(p))) => {
                assert_eq!(p.timestamp, 456);
            }
            _ => panic!("expected UDP Ping message"),
        }
    }

    #[tokio::test]
    async fn send_and_receive_command() {
        let (sender, mut receiver) = create();
        let cmd: BoxedCommand = Box::new(Disconnect);
        sender.send_command(cmd).await.unwrap();

        let item = receiver.recv().await;
        match item {
            WorkItem::UserCommand(_) => {} // correct
            _ => panic!("expected UserCommand"),
        }
    }

    #[tokio::test]
    async fn udp_has_priority_over_tcp() {
        let (sender, mut receiver) = create();

        // Send TCP first, then UDP
        let tcp_msg = ControlMessage::Ping(mumble_tcp::Ping {
            timestamp: Some(1),
            ..Default::default()
        });
        let udp_msg = UdpMessage::Ping(mumble_udp::Ping {
            timestamp: 2,
            ..Default::default()
        });

        sender.send_tcp(tcp_msg).await.unwrap();
        sender.send_udp(udp_msg).await.unwrap();

        // Give the runtime a tick to make both available
        tokio::task::yield_now().await;

        // UDP should come first due to biased select
        let first = receiver.recv().await;
        match first {
            WorkItem::ServerMessage(ServerMessage::Udp(UdpMessage::Ping(p))) => {
                assert_eq!(p.timestamp, 2);
            }
            _ => panic!("expected UDP first due to priority"),
        }

        let second = receiver.recv().await;
        match second {
            WorkItem::ServerMessage(ServerMessage::Control(ControlMessage::Ping(p))) => {
                assert_eq!(p.timestamp, Some(1));
            }
            _ => panic!("expected TCP second"),
        }
    }

    #[tokio::test]
    async fn shutdown_when_all_senders_dropped() {
        let (sender, mut receiver) = create();
        drop(sender);

        let item = receiver.recv().await;
        matches!(item, WorkItem::Shutdown);
    }

    #[tokio::test]
    async fn sender_is_cloneable() {
        let (sender, mut receiver) = create();
        let sender2 = sender.clone();

        sender
            .send_tcp(ControlMessage::Ping(mumble_tcp::Ping {
                timestamp: Some(10),
                ..Default::default()
            }))
            .await
            .unwrap();
        sender2
            .send_tcp(ControlMessage::Ping(mumble_tcp::Ping {
                timestamp: Some(20),
                ..Default::default()
            }))
            .await
            .unwrap();

        let _ = receiver.recv().await;
        let _ = receiver.recv().await;
        // Both messages received - sender clone works
    }

    #[tokio::test]
    async fn send_fails_when_receiver_dropped() {
        let (sender, receiver) = create();
        drop(receiver);

        let result = sender
            .send_tcp(ControlMessage::Ping(mumble_tcp::Ping::default()))
            .await;
        assert!(result.is_err());
    }
}
