//! UDP transport for low-latency Mumble audio and ping messages.
//!
//! Mumble encrypts UDP packets using OCB2-AES128. The encryption keys are
//! exchanged over the TCP control channel via [`CryptSetup`] messages.
//!
//! This module provides the framing and send/recv logic. The actual
//! encryption is abstracted behind the [`CryptState`] trait so that
//! callers can plug in their own implementation.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

use prost::Message;
use tokio::net::UdpSocket;
use tracing::{debug, trace, warn};

use crate::error::{Error, Result};
use crate::message::UdpMessage;
use crate::proto::mumble_udp;

/// Maximum UDP datagram size (Mumble practical limit).
const MAX_UDP_SIZE: usize = 1024;

/// Abstraction over the OCB2-AES128 encryption used for Mumble UDP.
///
/// Implement this trait to provide the actual cryptographic operations.
/// The keys and nonces are supplied via `CryptSetup` messages from TCP.
pub trait CryptState: Send + Sync {
    /// Encrypt `plaintext` and return `(ciphertext, tag)`.
    fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>>;

    /// Decrypt `ciphertext` and return plaintext, verifying the tag.
    fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>>;

    /// Returns true once keys have been set up.
    fn is_initialized(&self) -> bool;
}

/// A no-op crypt state for testing or when encryption is handled externally.
#[derive(Debug)]
pub struct PlaintextCryptState;

impl CryptState for PlaintextCryptState {
    fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>> {
        Ok(plaintext.to_vec())
    }

    fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        Ok(ciphertext.to_vec())
    }

    fn is_initialized(&self) -> bool {
        true
    }
}

/// Configuration for the UDP transport.
#[derive(Debug, Clone)]
pub struct UdpConfig {
    /// Hostname or IP address of the Mumble server.
    pub server_host: String,
    /// UDP port the server listens on (default 64738).
    pub server_port: u16,
}

impl Default for UdpConfig {
    fn default() -> Self {
        Self {
            server_host: "localhost".into(),
            server_port: 64738,
        }
    }
}

/// UDP transport for sending/receiving audio and ping messages.
pub struct UdpTransport<C: CryptState> {
    socket: Arc<UdpSocket>,
    server_addr: SocketAddr,
    crypt: C,
    recv_buf: Vec<u8>,
}

impl<C: CryptState + std::fmt::Debug> std::fmt::Debug for UdpTransport<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UdpTransport")
            .field("server_addr", &self.server_addr)
            .finish_non_exhaustive()
    }
}

impl<C: CryptState> UdpTransport<C> {
    /// Bind a local UDP socket and associate it with the server address.
    pub async fn connect(config: &UdpConfig, crypt: C) -> Result<Self> {
        // Use tokio DNS resolution so hostnames like "magical.rocks" work.
        let server_addr: SocketAddr = tokio::net::lookup_host(format!(
            "{}:{}",
            config.server_host, config.server_port
        ))
        .await
        .map_err(|e| Error::InvalidState(format!("DNS lookup failed: {e}")))?
        .next()
        .ok_or_else(|| {
            Error::InvalidState("DNS lookup returned no addresses".into())
        })?;

        let bind_addr: SocketAddr = if server_addr.is_ipv6() {
            SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0)
        } else {
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)
        };

        let socket = UdpSocket::bind(bind_addr).await?;
        socket.connect(server_addr).await?;
        debug!(server = %server_addr, "UDP socket connected");

        Ok(Self {
            socket: Arc::new(socket),
            server_addr,
            crypt,
            recv_buf: vec![0u8; MAX_UDP_SIZE],
        })
    }

    /// Send a UDP message (audio or ping) to the server.
    pub async fn send(&mut self, msg: &UdpMessage) -> Result<()> {
        let payload = encode_udp_message(msg);
        let encrypted = self.crypt.encrypt(&payload)?;
        let _ = self.socket.send(&encrypted).await?;
        trace!("sent UDP packet ({} bytes)", encrypted.len());
        Ok(())
    }

    /// Receive the next UDP message from the server.
    pub async fn recv(&mut self) -> Result<UdpMessage> {
        loop {
            let n = self.socket.recv(&mut self.recv_buf).await?;
            if n == 0 {
                return Err(Error::ConnectionClosed);
            }

            let decrypted = match self.crypt.decrypt(&self.recv_buf[..n]) {
                Ok(data) => data,
                Err(e) => {
                    warn!("UDP decrypt failed, skipping packet: {e}");
                    continue;
                }
            };

            match decode_udp_message(&decrypted) {
                Ok(msg) => return Ok(msg),
                Err(e) => {
                    warn!("UDP decode failed, skipping packet: {e}");
                    continue;
                }
            }
        }
    }

    /// Access the underlying server address.
    pub fn server_addr(&self) -> SocketAddr {
        self.server_addr
    }

    /// Get a clone of the underlying `Arc<UdpSocket>` for concurrent access
    /// (e.g. spawning a reader task while keeping the writer).
    pub fn socket_arc(&self) -> Arc<UdpSocket> {
        self.socket.clone()
    }

    /// Replace the crypt state (e.g. after receiving a new `CryptSetup`).
    pub fn set_crypt_state(&mut self, crypt: C) {
        self.crypt = crypt;
    }
}

// -- Encode / Decode ------------------------------------------------

/// Protobuf UDP wire format (Mumble >= 1.5).
///
/// Byte 0 is the raw `UDPMessageType` enum value:
///   - `0x00` = Audio
///   - `0x01` = Ping
///
/// Unlike the legacy format (and the TCP `UdpTunnel` encoding), the
/// target is carried **inside** the protobuf `Audio.target` field, not
/// packed into the header byte.
///
/// Encode a [`UdpMessage`] into the raw UDP wire format (before encryption).
pub fn encode_udp_message(msg: &UdpMessage) -> Vec<u8> {
    match msg {
        UdpMessage::Audio(audio) => {
            let mut buf = Vec::with_capacity(1 + audio.encoded_len());
            buf.push(0x00); // UDPMessageType::Audio
            let encoded = audio.encode_to_vec();
            buf.extend_from_slice(&encoded);
            buf
        }
        UdpMessage::Ping(ping) => {
            let mut buf = Vec::with_capacity(1 + ping.encoded_len());
            buf.push(0x01); // UDPMessageType::Ping
            let encoded = ping.encode_to_vec();
            buf.extend_from_slice(&encoded);
            buf
        }
    }
}

/// Decode a raw UDP payload (after decryption) into a [`UdpMessage`].
///
/// Byte 0 is the `UDPMessageType` enum value:
///   - `0x00` = Audio (protobuf)
///   - `0x01` = Ping  (protobuf)
pub fn decode_udp_message(data: &[u8]) -> Result<UdpMessage> {
    if data.is_empty() {
        return Err(Error::InvalidState("empty UDP packet".into()));
    }

    match data[0] {
        0x00 => {
            let audio = mumble_udp::Audio::decode(&data[1..])?;
            Ok(UdpMessage::Audio(audio))
        }
        0x01 => {
            let ping = mumble_udp::Ping::decode(&data[1..])?;
            Ok(UdpMessage::Ping(ping))
        }
        other => Err(Error::InvalidState(format!(
            "unsupported UDP message type: 0x{other:02x}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_udp_ping() -> Result<()> {
        let ping = mumble_udp::Ping {
            timestamp: 12345,
            ..Default::default()
        };
        let msg = UdpMessage::Ping(ping);
        let encoded = encode_udp_message(&msg);
        let decoded = decode_udp_message(&encoded)?;

        match decoded {
            UdpMessage::Ping(p) => assert_eq!(p.timestamp, 12345),
            other => panic!("unexpected: {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn roundtrip_udp_audio() -> Result<()> {
        let audio = mumble_udp::Audio {
            sender_session: 5,
            frame_number: 100,
            opus_data: vec![0xCA, 0xFE],
            positional_data: vec![1.0, 2.0, 3.0],
            volume_adjustment: 0.5,
            is_terminator: true,
            header: Some(mumble_udp::audio::Header::Target(0)),
        };
        let msg = UdpMessage::Audio(audio);
        let encoded = encode_udp_message(&msg);
        let decoded = decode_udp_message(&encoded)?;

        match decoded {
            UdpMessage::Audio(a) => {
                assert_eq!(a.sender_session, 5);
                assert_eq!(a.frame_number, 100);
                assert_eq!(a.opus_data, vec![0xCA, 0xFE]);
                assert_eq!(a.positional_data, vec![1.0, 2.0, 3.0]);
                assert!(a.is_terminator);
            }
            other => panic!("expected Audio, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn udp_ping_marker_is_0x01() {
        let msg = UdpMessage::Ping(mumble_udp::Ping::default());
        let encoded = encode_udp_message(&msg);
        assert_eq!(encoded[0], 0x01);
    }

    #[test]
    fn udp_audio_marker_is_0x00() {
        let msg = UdpMessage::Audio(mumble_udp::Audio::default());
        let encoded = encode_udp_message(&msg);
        assert_eq!(encoded[0], 0x00);
    }

    #[test]
    fn udp_audio_preserves_target_in_payload() -> Result<()> {
        let audio = mumble_udp::Audio {
            header: Some(mumble_udp::audio::Header::Target(3)),
            frame_number: 1,
            opus_data: vec![0xAB],
            ..Default::default()
        };
        let encoded = encode_udp_message(&UdpMessage::Audio(audio));
        let decoded = decode_udp_message(&encoded)?;
        match decoded {
            UdpMessage::Audio(a) => {
                assert_eq!(
                    a.header,
                    Some(mumble_udp::audio::Header::Target(3)),
                    "target should survive round-trip inside protobuf payload"
                );
            }
            other => panic!("expected Audio, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn decode_empty_returns_error() {
        let result = decode_udp_message(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn decode_unknown_type_returns_error() {
        // 0x02 is not a valid protobuf UDP message type
        let result = decode_udp_message(&[0x02]);
        assert!(result.is_err());
    }

    #[test]
    fn plaintext_crypt_state_is_passthrough() -> Result<()> {
        let mut crypt = PlaintextCryptState;
        let data = b"hello world";

        let encrypted = crypt.encrypt(data)?;
        assert_eq!(encrypted, data);

        let decrypted = crypt.decrypt(&encrypted)?;
        assert_eq!(decrypted, data);

        assert!(crypt.is_initialized());
        Ok(())
    }

    #[test]
    fn udp_ping_extended_info_roundtrip() -> Result<()> {
        let ping = mumble_udp::Ping {
            timestamp: 99999,
            request_extended_information: true,
            server_version_v2: 0x0001_0005_0000_0000,
            user_count: 10,
            max_user_count: 100,
            max_bandwidth_per_user: 72000,
        };
        let msg = UdpMessage::Ping(ping);
        let encoded = encode_udp_message(&msg);
        let decoded = decode_udp_message(&encoded)?;

        match decoded {
            UdpMessage::Ping(p) => {
                assert_eq!(p.timestamp, 99999);
                assert!(p.request_extended_information);
                assert_eq!(p.user_count, 10);
                assert_eq!(p.max_user_count, 100);
            }
            other => panic!("expected Ping, got {other:?}"),
        }
        Ok(())
    }
}
