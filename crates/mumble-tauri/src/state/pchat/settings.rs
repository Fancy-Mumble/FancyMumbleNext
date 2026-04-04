//! Shared constants for the persistent chat subsystem.

/// Top-level directory for per-identity storage.
pub(crate) const IDENTITIES_DIR: &str = "identities";
/// File name for the pchat identity seed inside each identity folder.
pub(crate) const SEED_FILE: &str = "pchat_seed.bin";
/// File name for the TLS client certificate inside each identity folder.
pub(crate) const TLS_CERT_FILE: &str = "tls.cert.pem";
/// File name for the TLS private key inside each identity folder.
pub(crate) const TLS_KEY_FILE: &str = "tls.key.pem";
/// File name for the signal bridge sender key state.
pub(crate) const SIGNAL_STATE_FILE: &str = "signal_state.json";
/// File name for persisted archive keys inside the identity directory.
pub(crate) const ARCHIVE_KEYS_FILE: &str = "archive_keys.json";

/// Legacy paths used before per-identity storage was introduced.
pub(super) const LEGACY_PCHAT_DIR: &str = "pchat";
pub(super) const LEGACY_SEED_FILE: &str = "identity_seed.bin";
pub(super) const LEGACY_CERTS_DIR: &str = "certs";

/// Maximum number of `SignalV1` envelopes to stash while awaiting a
/// sender key distribution.
pub(super) const MAX_STASHED_ENVELOPES: usize = 50;

/// Placeholder body used when a message cannot be decrypted because
/// the encryption key has not yet arrived.
pub(crate) const PLACEHOLDER_BODY: &str = "[Encrypted message - awaiting key]";
