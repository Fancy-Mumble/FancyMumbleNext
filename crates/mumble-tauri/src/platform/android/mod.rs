//! Android-specific platform integrations.
//!
//! - [`connection_service`]: Foreground service bridge for background connectivity.
//! - [`fcm_service`]: Firebase Cloud Messaging device token retrieval.

pub(crate) mod connection_service;
pub(crate) mod fcm_service;
