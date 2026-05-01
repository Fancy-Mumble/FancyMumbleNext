//! Multi-server session metadata.
//!
//! Phase A of the multi-server rollout: introduces the [`ServerId`]
//! identifier and a lightweight [`SessionMeta`] record that the rest of
//! the backend can use to refer to a connection by stable id.  The
//! actual per-connection data (users, channels, messages, ...) still
//! lives flat inside [`super::SharedState`]; future phases will migrate
//! that data into a `HashMap<ServerId, ServerSession>`.

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::types::ConnectionStatus;

/// Stable identifier for a server connection.
///
/// Minted by the backend on each `connect` call.  Two connections to
/// the same `host:port` (e.g. with different usernames) get distinct
/// ids, so the frontend can show them as separate tabs.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ServerId(Uuid);

impl ServerId {
    /// Create a new random `ServerId`.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ServerId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ServerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::str::FromStr for ServerId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(s).map(Self)
    }
}

/// User-visible summary of a connected (or connecting) server.
///
/// Returned from the `list_servers` Tauri command and used by the
/// frontend tab strip.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    pub id: ServerId,
    pub label: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub cert_label: Option<String>,
    pub status: ConnectionStatus,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
mod tests {
    use super::*;

    #[test]
    fn server_id_is_unique() {
        let a = ServerId::new();
        let b = ServerId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn server_id_roundtrips_through_string() {
        let id = ServerId::new();
        let s = id.to_string();
        let parsed: ServerId = s.parse().unwrap();
        assert_eq!(id, parsed);
    }
}
