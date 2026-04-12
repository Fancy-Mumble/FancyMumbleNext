//! Channel persistence configuration parsed from `ChannelState` protobuf.

use super::PchatProtocol;
use serde::{Deserialize, Serialize};

/// Persistence configuration for a single channel, derived from
/// the `pchat_*` extension fields on `ChannelState`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelPersistConfig {
    /// The channel this config applies to.
    pub channel_id: u32,
    /// Persistence mode.
    pub mode: PchatProtocol,
    /// Maximum messages stored (0 = unlimited).
    pub max_history: u32,
    /// Auto-delete after N days (0 = forever).
    pub retention_days: u32,
    /// Cert hashes of designated key custodians.
    pub key_custodians: Vec<String>,
}

impl ChannelPersistConfig {
    /// Parse persistence config from a `ChannelState` protobuf message.
    ///
    /// Fields that are absent default to `NONE` mode with server defaults.
    pub fn from_channel_state(
        channel_id: u32,
        pchat_protocol: Option<i32>,
        pchat_max_history: Option<u32>,
        pchat_retention_days: Option<u32>,
        pchat_key_custodians: Vec<String>,
    ) -> Self {
        Self {
            channel_id,
            mode: pchat_protocol
                .map(PchatProtocol::from_proto)
                .unwrap_or(PchatProtocol::None),
            max_history: pchat_max_history.unwrap_or(0),
            retention_days: pchat_retention_days.unwrap_or(0),
            key_custodians: pchat_key_custodians,
        }
    }

    /// Whether this channel has any persistence enabled.
    #[must_use]
    pub fn is_persistent(&self) -> bool {
        self.mode != PchatProtocol::None
    }
}

/// Trait for extracting persistence configuration from the server state.
///
/// Implemented by types that track per-channel configs (e.g. a config
/// registry updated from `ChannelState` messages).
pub trait PersistenceConfigProvider: Send + Sync {
    /// Look up the persistence config for a channel.
    /// Returns `None` if the channel is unknown (treated as `NONE`).
    fn get_config(&self, channel_id: u32) -> Option<&ChannelPersistConfig>;

    /// Resolve the persistence mode for a channel, defaulting to `None`.
    fn mode_for(&self, channel_id: u32) -> PchatProtocol {
        self.get_config(channel_id)
            .map(|c| c.mode)
            .unwrap_or(PchatProtocol::None)
    }
}

/// Simple in-memory registry of channel persistence configs.
#[derive(Debug, Default)]
pub struct ConfigRegistry {
    configs: std::collections::HashMap<u32, ChannelPersistConfig>,
}

impl ConfigRegistry {
    /// Create a new, empty configuration registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or update the config for a channel.
    pub fn upsert(&mut self, config: ChannelPersistConfig) {
        let _ = self.configs.insert(config.channel_id, config);
    }

    /// Remove config when a channel is deleted.
    pub fn remove(&mut self, channel_id: u32) {
        let _ = self.configs.remove(&channel_id);
    }

    /// Iterate over all persistent channels.
    pub fn persistent_channels(&self) -> impl Iterator<Item = &ChannelPersistConfig> {
        self.configs.values().filter(|c| c.is_persistent())
    }
}

impl PersistenceConfigProvider for ConfigRegistry {
    fn get_config(&self, channel_id: u32) -> Option<&ChannelPersistConfig> {
        self.configs.get(&channel_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_channel_state_defaults() {
        let cfg = ChannelPersistConfig::from_channel_state(1, None, None, None, vec![]);
        assert_eq!(cfg.mode, PchatProtocol::None);
        assert_eq!(cfg.max_history, 0);
        assert_eq!(cfg.retention_days, 0);
        assert!(cfg.key_custodians.is_empty());
        assert!(!cfg.is_persistent());
    }

    #[test]
    fn from_channel_state_full_archive() {
        let cfg = ChannelPersistConfig::from_channel_state(
            42,
            Some(2),
            Some(1000),
            Some(90),
            vec!["abc123".into()],
        );
        assert_eq!(cfg.mode, PchatProtocol::FancyV1FullArchive);
        assert_eq!(cfg.max_history, 1000);
        assert_eq!(cfg.retention_days, 90);
        assert_eq!(cfg.key_custodians, vec!["abc123"]);
        assert!(cfg.is_persistent());
    }

    #[test]
    fn config_registry_upsert_and_query() {
        let mut reg = ConfigRegistry::new();
        let cfg = ChannelPersistConfig::from_channel_state(1, Some(2), None, None, vec![]);
        reg.upsert(cfg);
        assert_eq!(reg.mode_for(1), PchatProtocol::FancyV1FullArchive);
        assert_eq!(reg.mode_for(999), PchatProtocol::None);
    }

    #[test]
    fn config_registry_remove() {
        let mut reg = ConfigRegistry::new();
        reg.upsert(ChannelPersistConfig::from_channel_state(1, Some(1), None, None, vec![]));
        assert!(reg.get_config(1).is_some());
        reg.remove(1);
        assert!(reg.get_config(1).is_none());
    }

    #[test]
    fn persistent_channels_iterator() {
        let mut reg = ConfigRegistry::new();
        reg.upsert(ChannelPersistConfig::from_channel_state(1, Some(0), None, None, vec![]));
        reg.upsert(ChannelPersistConfig::from_channel_state(2, Some(1), None, None, vec![]));
        reg.upsert(ChannelPersistConfig::from_channel_state(3, Some(2), None, None, vec![]));
        let persistent: Vec<_> = reg.persistent_channels().collect();
        assert_eq!(persistent.len(), 2);
    }
}
