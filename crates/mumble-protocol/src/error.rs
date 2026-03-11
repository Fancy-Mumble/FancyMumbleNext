use std::io;

/// All errors that can occur within the mumble-protocol library.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("TLS error: {0}")]
    Tls(#[from] rustls::Error),

    #[error("Protobuf decode error: {0}")]
    Decode(#[from] prost::DecodeError),

    #[error("Protobuf encode error: {0}")]
    Encode(#[from] prost::EncodeError),

    #[error("Unknown message type: {0}")]
    UnknownMessageType(u16),

    #[error("Connection rejected: {0}")]
    Rejected(String),

    #[error("Connection closed")]
    ConnectionClosed,

    #[error("Work queue closed")]
    QueueClosed,

    #[error("Invalid state: {0}")]
    InvalidState(String),

    #[error("Opus codec error: {0}")]
    OpusCodec(String),

    #[error("{0}")]
    Other(String),
}

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
