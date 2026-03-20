//! Error types for the `mumble-protocol` library.
//!
//! All fallible operations return [`Result<T>`], which is a type alias for
//! `std::result::Result<T, Error>` using the library-local [`Error`] enum.
use std::io;

/// All errors that can occur within the mumble-protocol library.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An I/O error from the underlying OS or network stack.
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    /// A TLS handshake or record-layer error.
    #[error("TLS error: {0}")]
    Tls(#[from] rustls::Error),

    /// Failed to decode a protobuf message from the wire.
    #[error("Protobuf decode error: {0}")]
    Decode(#[from] prost::DecodeError),

    /// Failed to encode a protobuf message for the wire.
    #[error("Protobuf encode error: {0}")]
    Encode(#[from] prost::EncodeError),

    /// The server sent a message-type ID that the library does not recognise.
    #[error("Unknown message type: {0}")]
    UnknownMessageType(u16),

    /// The server rejected the connection (e.g. bad password, banned).
    #[error("Connection rejected: {0}")]
    Rejected(String),

    /// The TCP connection was closed by the remote end.
    #[error("Connection closed")]
    ConnectionClosed,

    /// The internal work queue was dropped while still in use.
    #[error("Work queue closed")]
    QueueClosed,

    /// An operation was attempted that is not valid in the current state.
    #[error("Invalid state: {0}")]
    InvalidState(String),

    /// The audio capture buffer does not yet have a full frame available.
    #[error("Not enough samples")]
    NotEnoughSamples,

    /// The Opus codec reported an error.
    #[error("Opus codec error: {0}")]
    OpusCodec(String),

    /// A catch-all for errors that do not fit a more specific variant.
    #[error("{0}")]
    Other(String),
}

/// Convenience alias so crate code can write `Result<T>` instead of
/// `std::result::Result<T, Error>`.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_io() {
        let err = Error::Io(io::Error::new(io::ErrorKind::ConnectionRefused, "refused"));
        let msg = format!("{err}");
        assert!(msg.contains("IO error"));
        assert!(msg.contains("refused"));
    }

    #[test]
    fn error_display_unknown_message_type() {
        let err = Error::UnknownMessageType(999);
        assert_eq!(format!("{err}"), "Unknown message type: 999");
    }

    #[test]
    fn error_display_rejected() {
        let err = Error::Rejected("wrong password".into());
        assert!(format!("{err}").contains("wrong password"));
    }

    #[test]
    fn error_display_connection_closed() {
        let err = Error::ConnectionClosed;
        assert_eq!(format!("{err}"), "Connection closed");
    }

    #[test]
    fn error_display_queue_closed() {
        let err = Error::QueueClosed;
        assert_eq!(format!("{err}"), "Work queue closed");
    }

    #[test]
    fn error_display_invalid_state() {
        let err = Error::InvalidState("bad".into());
        assert!(format!("{err}").contains("bad"));
    }

    #[test]
    fn error_from_io() {
        let io_err = io::Error::other("test");
        let err: Error = io_err.into();
        assert!(matches!(err, Error::Io(_)));
    }

    #[test]
    fn result_type_works() {
        let val: i32 = 42;
        assert_eq!(val, 42);

        let err: Result<i32> = Err(Error::ConnectionClosed);
        assert!(err.is_err());
    }
}
