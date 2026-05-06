//! Session registry: tracks every active server connection's
//! [`SharedState`] keyed by [`ServerId`], plus which one is currently
//! active.
//!
//! This is the central piece of the multi-server architecture.  Each
//! connected server gets its own `Arc<Mutex<SharedState>>`; the
//! registry maps `ServerId -> Arc<Mutex<SharedState>>` and remembers
//! which session is "active" (the one that commands without an explicit
//! `serverId` operate on).
//!
//! Phase B.1: the registry is in place but only ever holds at most one
//! entry; behaviour is identical to the single-connection world.  Phase
//! B.2 makes `connect` additive and `set_active_server` perform a real
//! switch between concurrent sessions.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::sessions::{ServerId, SessionMeta};
use super::SharedState;

/// Inner mutable state of [`Registry`].
#[derive(Default)]
struct RegistryInner {
    active: Option<ServerId>,
    sessions: HashMap<ServerId, Arc<Mutex<SharedState>>>,
}

/// Concurrency-safe wrapper around the per-session map.
#[derive(Default, Clone)]
pub(crate) struct Registry {
    inner: Arc<Mutex<RegistryInner>>,
}

impl Registry {
    /// Return the active session's id, if any.
    pub(crate) fn active_id(&self) -> Option<ServerId> {
        self.inner.lock().ok().and_then(|g| g.active)
    }

    /// Look up a specific session's [`SharedState`] handle by id.
    pub(crate) fn session(&self, id: ServerId) -> Option<Arc<Mutex<SharedState>>> {
        self.inner.lock().ok()?.sessions.get(&id).cloned()
    }

    /// Insert a new session and mark it as active.  Returns the id.
    pub(crate) fn register_active(
        &self,
        id: ServerId,
        shared: Arc<Mutex<SharedState>>,
    ) -> Option<Arc<Mutex<SharedState>>> {
        let mut guard = self.inner.lock().ok()?;
        let displaced = guard.sessions.insert(id, shared);
        guard.active = Some(id);
        displaced
    }

    /// Set which session is active.  Returns `Err` if `id` is unknown.
    pub(crate) fn set_active(&self, id: ServerId) -> Result<(), String> {
        let mut guard = self.inner.lock().map_err(|e| e.to_string())?;
        if !guard.sessions.contains_key(&id) {
            return Err(format!("unknown server id: {id}"));
        }
        guard.active = Some(id);
        Ok(())
    }

    /// Remove a session.  If it was the active one, picks an arbitrary
    /// remaining session as the new active (or `None` if empty).
    pub(crate) fn remove(&self, id: ServerId) -> Option<Arc<Mutex<SharedState>>> {
        let mut guard = self.inner.lock().ok()?;
        let removed = guard.sessions.remove(&id);
        if guard.active == Some(id) {
            guard.active = guard.sessions.keys().copied().next();
        }
        removed
    }

    /// Remove every disconnected session whose `(host, port, username)`
    /// matches the supplied target.  Used by `connect()` to eliminate
    /// stale tabs left behind by an automatic reconnect attempt against
    /// the same target — without this pruning each retry would leave
    /// the previous failed session in the registry, spamming the tab
    /// strip with one entry per attempt.
    ///
    /// Sessions whose status is still `Connecting` or `Connected` are
    /// left alone: the user may legitimately have several attempts in
    /// flight, and we never want to silently kill a live session.
    pub(crate) fn prune_disconnected_for(
        &self,
        host: &str,
        port: u16,
        username: &str,
    ) -> Vec<ServerId> {
        let Ok(mut guard) = self.inner.lock() else {
            return Vec::new();
        };
        let stale: Vec<ServerId> = guard
            .sessions
            .iter()
            .filter_map(|(id, shared)| {
                let s = shared.lock().ok()?;
                let matches = s.server.host == host
                    && s.server.port == port
                    && s.conn.own_name == username
                    && s.conn.status == super::types::ConnectionStatus::Disconnected;
                matches.then_some(*id)
            })
            .collect();
        for id in &stale {
            let _ = guard.sessions.remove(id);
            if guard.active == Some(*id) {
                guard.active = None;
            }
        }
        stale
    }

    /// Snapshot the metadata of every known session, suitable for the
    /// `list_servers` command.  Reads the per-session `SharedState` to
    /// derive the live status, host, port, username, etc.
    pub(crate) fn list_meta(&self) -> Vec<SessionMeta> {
        let Ok(guard) = self.inner.lock() else {
            return Vec::new();
        };
        guard
            .sessions
            .iter()
            .filter_map(|(id, shared)| {
                let s = shared.lock().ok()?;
                Some(SessionMeta {
                    id: *id,
                    label: format_label(&s.conn.own_name, &s.server.host, s.server.port),
                    host: s.server.host.clone(),
                    port: s.server.port,
                    username: s.conn.own_name.clone(),
                    cert_label: s.cert_label.clone(),
                    status: s.conn.status,
                })
            })
            .collect()
    }
}

fn format_label(username: &str, host: &str, port: u16) -> String {
    if username.is_empty() {
        format!("{host}:{port}")
    } else {
        format!("{username}@{host}:{port}")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
mod tests {
    use super::*;

    fn make_shared(host: &str, port: u16, user: &str) -> Arc<Mutex<SharedState>> {
        let mut s = SharedState::default();
        s.server.host = host.into();
        s.server.port = port;
        s.conn.own_name = user.into();
        Arc::new(Mutex::new(s))
    }

    #[test]
    fn register_and_resolve_active() {
        let reg = Registry::default();
        let id = ServerId::new();
        let shared = make_shared("h", 1, "u");
        let _ = reg.register_active(id, shared);
        assert_eq!(reg.active_id(), Some(id));
    }

    #[test]
    fn remove_picks_next_active() {
        let reg = Registry::default();
        let a = ServerId::new();
        let b = ServerId::new();
        let _ = reg.register_active(a, make_shared("a", 1, "u1"));
        let _ = reg.register_active(b, make_shared("b", 2, "u2"));
        assert_eq!(reg.active_id(), Some(b));
        let _ = reg.remove(b);
        assert_eq!(reg.active_id(), Some(a));
        let _ = reg.remove(a);
        assert!(reg.active_id().is_none());
    }

    #[test]
    fn list_meta_synthesises_label() {
        let reg = Registry::default();
        let id = ServerId::new();
        let _ = reg.register_active(id, make_shared("mumble.example", 64738, "alice"));
        let metas = reg.list_meta();
        assert_eq!(metas.len(), 1);
        assert_eq!(metas[0].label, "alice@mumble.example:64738");
    }

    #[test]
    fn prune_removes_only_disconnected_matching_target() {
        use super::super::types::ConnectionStatus;

        let reg = Registry::default();
        // Two stale disconnected sessions targeting the same server.
        let stale_a = ServerId::new();
        let stale_b = ServerId::new();
        // A live connecting session to the same target — must NOT be pruned.
        let live = ServerId::new();
        // A disconnected session targeting a *different* server — must NOT be pruned.
        let other = ServerId::new();

        let stale_a_shared = make_shared("h", 1, "u");
        let stale_b_shared = make_shared("h", 1, "u");
        let live_shared = make_shared("h", 1, "u");
        live_shared.lock().unwrap().conn.status = ConnectionStatus::Connecting;
        let other_shared = make_shared("other", 1, "u");

        let _ = reg.register_active(stale_a, stale_a_shared);
        let _ = reg.register_active(stale_b, stale_b_shared);
        let _ = reg.register_active(live, live_shared);
        let _ = reg.register_active(other, other_shared);

        let pruned = reg.prune_disconnected_for("h", 1, "u");
        assert_eq!(pruned.len(), 2);
        assert!(pruned.contains(&stale_a));
        assert!(pruned.contains(&stale_b));
        let remaining: Vec<_> = reg.list_meta().into_iter().map(|m| m.id).collect();
        assert!(remaining.contains(&live));
        assert!(remaining.contains(&other));
        assert_eq!(remaining.len(), 2);
    }
}
