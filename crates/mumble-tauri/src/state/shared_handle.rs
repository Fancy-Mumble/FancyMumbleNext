//! Atomically-swappable handle to the active session's [`SharedState`].
//!
//! Wraps an [`arc_swap::ArcSwap`] holding the currently-active
//! `Arc<Mutex<SharedState>>` so that switching the active server is a
//! single atomic pointer swap, while background tasks (event loops,
//! audio loops) that captured a specific session's
//! `Arc<Mutex<SharedState>>` keep operating on that session even after
//! the active one changes.
//!
//! Call sites obtain a snapshot of the current `Arc` first and then
//! lock it normally:
//!
//! ```ignore
//! let session = state.inner.snapshot();
//! let mut s = session.lock().map_err(|e| e.to_string())?;
//! ```
//!
//! The two-step pattern is required because a [`std::sync::MutexGuard`]
//! borrows from its `Mutex`; binding the `Arc` to a local keeps the
//! `Mutex` alive for the duration of the guard without resorting to
//! unsafe lifetime extension.

use std::sync::{Arc, Mutex};

use arc_swap::ArcSwap;

use super::SharedState;

/// Cheaply-cloneable handle to the currently-active per-session state.
#[derive(Clone)]
pub(crate) struct SharedHandle {
    swap: Arc<ArcSwap<Mutex<SharedState>>>,
}

impl SharedHandle {
    /// Build a handle initially pointing at `initial`.
    pub(crate) fn new(initial: Arc<Mutex<SharedState>>) -> Self {
        Self {
            swap: Arc::new(ArcSwap::new(initial)),
        }
    }

    /// Replace the currently-active session's state with `next`.
    /// Returns the previously-active `Arc`.
    pub(crate) fn swap(&self, next: Arc<Mutex<SharedState>>) -> Arc<Mutex<SharedState>> {
        self.swap.swap(next)
    }

    /// Take a snapshot of the currently-active session's `Arc`.  Bind
    /// to a local before locking so the `Arc` outlives the guard.
    pub(crate) fn snapshot(&self) -> Arc<Mutex<SharedState>> {
        self.swap.load_full()
    }

    /// Alias for [`Self::snapshot`] kept for call sites that previously
    /// spelled `Arc::clone(&state.inner)` / `state.inner.clone()`.
    #[inline]
    pub(crate) fn clone_arc(&self) -> Arc<Mutex<SharedState>> {
        self.snapshot()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
mod tests {
    use super::*;

    fn make(host: &str) -> Arc<Mutex<SharedState>> {
        let mut s = SharedState::default();
        s.server.host = host.into();
        Arc::new(Mutex::new(s))
    }

    #[test]
    fn snapshot_returns_active_arc() {
        let h = SharedHandle::new(make("alpha"));
        let arc = h.snapshot();
        assert_eq!(arc.lock().unwrap().server.host, "alpha");
    }

    #[test]
    fn swap_changes_what_snapshot_returns() {
        let h = SharedHandle::new(make("alpha"));
        let _ = h.swap(make("beta"));
        assert_eq!(h.snapshot().lock().unwrap().server.host, "beta");
    }

    #[test]
    fn previous_snapshot_outlives_swap() {
        let h = SharedHandle::new(make("alpha"));
        let snap = h.snapshot();
        let _ = h.swap(make("beta"));
        assert_eq!(snap.lock().unwrap().server.host, "alpha");
        assert_eq!(h.snapshot().lock().unwrap().server.host, "beta");
    }

    #[test]
    fn mutating_via_snapshot_persists() {
        let h = SharedHandle::new(make("alpha"));
        {
            let arc = h.snapshot();
            arc.lock().unwrap().server.host = "gamma".into();
        }
        assert_eq!(h.snapshot().lock().unwrap().server.host, "gamma");
    }
}
