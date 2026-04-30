//! Read-only query methods: status, users, channels, messages, server
//! info, debug stats, and welcome text.

use super::offload::OffloadStore;
use super::types::*;
use super::AppState;

impl AppState {
    pub fn status(&self) -> ConnectionStatus {
        self.inner
            .lock()
            .map(|s| s.conn.status)
            .unwrap_or(ConnectionStatus::Disconnected)
    }

    pub fn channels(&self) -> Vec<ChannelEntry> {
        self.inner
            .lock()
            .map(|mut s| {
                Self::refresh_user_counts(&mut s);
                let root_perms = s.server.root_permissions;
                let mut channels: Vec<_> = s.channels.values().cloned().collect();
                if let Some(fallback) = root_perms {
                    for ch in &mut channels {
                        ch.permissions = ch.permissions.or(Some(fallback));
                    }
                }
                channels.sort_by_key(|c| c.id);
                channels
            })
            .unwrap_or_default()
    }

    pub fn users(&self) -> Vec<UserEntry> {
        self.inner
            .lock()
            .map(|s| s.users.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Return the avatar texture bytes for a single user, or `None` if the
    /// user is not connected or has no avatar.  Used by the frontend to
    /// lazily fetch avatars after `get_users` returned only the byte
    /// length (`texture_size`).
    pub fn user_texture(&self, session: u32) -> Option<Vec<u8>> {
        self.inner
            .lock()
            .ok()
            .and_then(|s| s.users.get(&session).and_then(|u| u.texture.clone()))
    }

    /// Return the description text for a single channel, or `None` if the
    /// channel is unknown or has no description.  Used by the frontend to
    /// lazily fetch descriptions after `get_channels` returned only the
    /// byte length (`description_size`).
    pub fn channel_description(&self, channel_id: u32) -> Option<String> {
        self.inner
            .lock()
            .ok()
            .and_then(|s| s.channels.get(&channel_id).map(|c| c.description.clone()))
            .filter(|d| !d.is_empty())
    }

    pub fn messages(&self, channel_id: u32) -> Vec<ChatMessage> {
        self.inner
            .lock()
            .map(|s| s.msgs.by_channel.get(&channel_id).cloned().unwrap_or_default())
            .unwrap_or_default()
    }

    pub fn dm_messages(&self, session: u32) -> Vec<ChatMessage> {
        self.inner
            .lock()
            .map(|s| s.msgs.by_dm.get(&session).cloned().unwrap_or_default())
            .unwrap_or_default()
    }

    pub fn get_own_session(&self) -> Option<u32> {
        self.inner.lock().ok().and_then(|s| s.conn.own_session)
    }

    pub fn push_subscribed_channels(&self) -> Vec<u32> {
        self.inner
            .lock()
            .map(|s| s.push_subscribed_channels.iter().copied().collect())
            .unwrap_or_default()
    }

    pub fn server_config(&self) -> ServerConfig {
        self.inner
            .lock()
            .map(|s| s.server.config.clone())
            .unwrap_or_default()
    }

    pub fn server_info(&self) -> ServerInfo {
        self.inner
            .lock()
            .map(|s| {
                let vi = &s.server.version_info;
                let protocol_version = vi.version_v2.map(|v| {
                    let major = (v >> 48) & 0xFFFF;
                    let minor = (v >> 32) & 0xFFFF;
                    let patch = (v >> 16) & 0xFFFF;
                    format!("{major}.{minor}.{patch}")
                }).or_else(|| vi.version_v1.map(|v| {
                    let major = (v >> 16) & 0xFF;
                    let minor = (v >> 8) & 0xFF;
                    let patch = v & 0xFF;
                    format!("{major}.{minor}.{patch}")
                }));

                let os = match (vi.os.as_deref(), vi.os_version.as_deref()) {
                    (Some(name), Some(ver)) if !ver.is_empty() => Some(format!("{name} ({ver})")),
                    (Some(name), _) => Some(name.to_owned()),
                    _ => None,
                };

                ServerInfo {
                    host: s.server.host.clone(),
                    port: s.server.port,
                    user_count: s.users.len() as u32,
                    max_users: s.server.max_users,
                    protocol_version,
                    fancy_version: s.server.fancy_version,
                    release: vi.release.clone(),
                    os,
                    max_bandwidth: s.server.max_bandwidth,
                    opus: s.server.opus,
                }
            })
            .unwrap_or_else(|_| ServerInfo {
                host: String::new(),
                port: 0,
                user_count: 0,
                max_users: None,
                protocol_version: None,
                fancy_version: None,
                release: None,
                os: None,
                max_bandwidth: None,
                opus: false,
            })
    }

    pub fn debug_stats(&self) -> DebugStats {
        self.inner
            .lock()
            .map(|s| {
                let channel_msgs: usize = s.msgs.by_channel.values().map(Vec::len).sum();
                let dm_msgs: usize = s.msgs.by_dm.values().map(Vec::len).sum();
                let offloaded = s
                    .offload_store
                    .as_ref()
                    .map_or(0, OffloadStore::offloaded_count);

                DebugStats {
                    channel_message_count: channel_msgs,
                    dm_message_count: dm_msgs,
                    total_message_count: channel_msgs + dm_msgs,
                    offloaded_count: offloaded,
                    channel_count: s.channels.len(),
                    user_count: s.users.len(),
                    connection_epoch: s.conn.epoch,
                    voice_state: format!("{:?}", s.audio.voice_state),
                    uptime_seconds: self.start_time.elapsed().as_secs(),
                }
            })
            .unwrap_or(DebugStats {
                channel_message_count: 0,
                dm_message_count: 0,
                total_message_count: 0,
                offloaded_count: 0,
                channel_count: 0,
                user_count: 0,
                connection_epoch: 0,
                voice_state: "Unknown".into(),
                uptime_seconds: self.start_time.elapsed().as_secs(),
            })
    }

    pub fn welcome_text(&self) -> Option<String> {
        self.inner
            .lock()
            .ok()
            .and_then(|s| s.server.welcome_text.clone())
    }
}
