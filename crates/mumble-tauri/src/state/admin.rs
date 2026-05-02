//! Server administration actions: kick, ban, register, mute, deafen,
//! priority speaker, user stats, user list, ban list, and ACL management.

use mumble_protocol::command;

use super::types::{AclInput, BanEntryInput, RegisteredUserUpdate};
use super::AppState;

impl AppState {
    pub async fn kick_user(&self, session: u32, reason: Option<String>) -> Result<(), String> {
        let handle = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::KickUser { session, reason })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    pub async fn ban_user(&self, session: u32, reason: Option<String>) -> Result<(), String> {
        let handle = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::BanUser { session, reason })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    pub async fn register_user(&self, session: u32) -> Result<(), String> {
        let handle = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::RegisterUser { session })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    pub async fn mute_user(&self, session: u32, muted: bool) -> Result<(), String> {
        let handle = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::SetUserMute { session, muted })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    pub async fn deafen_user(&self, session: u32, deafened: bool) -> Result<(), String> {
        let handle = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::SetUserDeaf { session, deafened })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    pub async fn set_priority_speaker(
        &self,
        session: u32,
        priority: bool,
    ) -> Result<(), String> {
        let handle = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::SetPrioritySpeaker { session, priority })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    pub async fn reset_user_comment(&self, session: u32) -> Result<(), String> {
        let handle = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::ResetUserComment { session })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    pub async fn remove_user_avatar(&self, session: u32) -> Result<(), String> {
        let handle = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::RemoveUserAvatar { session })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    /// Move another user to a different channel (admin action).
    /// Requires the `Move` permission on both source and destination
    /// channels (or `MoveAll` server-wide).
    pub async fn move_user(&self, session: u32, channel_id: u32) -> Result<(), String> {
        let handle = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::MoveUser { session, channel_id })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    pub async fn request_user_stats(&self, session: u32) -> Result<(), String> {
        let handle = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::RequestUserStats { session })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    pub async fn request_user_list(&self) -> Result<(), String> {
        let handle = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::RequestUserList)
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    pub async fn request_user_comment(&self, user_id: u32) -> Result<(), String> {
        let handle = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };
        let handle = handle.ok_or_else(|| "Not connected".to_owned())?;
        handle
            .send(command::RequestBlob {
                session_texture: vec![],
                session_comment: vec![],
                channel_description: vec![],
                user_id_comment: vec![user_id],
            })
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn update_user_list(
        &self,
        users: Vec<RegisteredUserUpdate>,
    ) -> Result<(), String> {
        let handle = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };
        let entries = users
            .into_iter()
            .map(|u| command::UserListEntry {
                user_id: u.user_id,
                name: u.name,
            })
            .collect();
        match handle {
            Some(h) => h
                .send(command::UpdateUserList { users: entries })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    pub async fn request_ban_list(&self) -> Result<(), String> {
        let handle = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::RequestBanList)
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    pub async fn update_ban_list(
        &self,
        bans: Vec<BanEntryInput>,
    ) -> Result<(), String> {
        use mumble_protocol::proto::mumble_tcp;

        let entries: Result<Vec<_>, String> = bans
            .into_iter()
            .map(|b| {
                let address = fancy_utils::net::parse_ip_to_bytes(&b.address)?;
                Ok(mumble_tcp::ban_list::BanEntry {
                    address,
                    mask: b.mask,
                    name: if b.name.is_empty() { None } else { Some(b.name) },
                    hash: if b.hash.is_empty() { None } else { Some(b.hash) },
                    reason: if b.reason.is_empty() { None } else { Some(b.reason) },
                    start: if b.start.is_empty() { None } else { Some(b.start) },
                    duration: if b.duration == 0 { None } else { Some(b.duration) },
                })
            })
            .collect();
        let entries = entries?;

        let handle = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::SendBanList { bans: entries })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    pub async fn request_acl(&self, channel_id: u32) -> Result<(), String> {
        let handle = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::RequestAcl { channel_id })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }

    pub async fn update_acl(&self, acl: AclInput) -> Result<(), String> {
        use mumble_protocol::proto::mumble_tcp;

        let groups: Vec<mumble_tcp::acl::ChanGroup> = acl
            .groups
            .into_iter()
            .map(|g| mumble_tcp::acl::ChanGroup {
                name: g.name,
                inherited: Some(g.inherited),
                inherit: Some(g.inherit),
                inheritable: Some(g.inheritable),
                add: g.add,
                remove: g.remove,
                inherited_members: g.inherited_members,
                color: g.color,
                icon: g.icon,
                style_preset: g.style_preset,
                metadata: g
                    .metadata
                    .into_iter()
                    .map(|(key, value)| mumble_tcp::acl::chan_group::KeyValue {
                        key,
                        value: Some(value),
                    })
                    .collect(),
            })
            .collect();

        let acls: Vec<mumble_tcp::acl::ChanAcl> = acl
            .acls
            .into_iter()
            .map(|a| mumble_tcp::acl::ChanAcl {
                apply_here: Some(a.apply_here),
                apply_subs: Some(a.apply_subs),
                inherited: Some(a.inherited),
                user_id: a.user_id,
                group: a.group,
                grant: Some(a.grant),
                deny: Some(a.deny),
            })
            .collect();

        let handle = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            state.conn.client_handle.clone()
        };
        match handle {
            Some(h) => h
                .send(command::SendAcl {
                    channel_id: acl.channel_id,
                    inherit_acls: acl.inherit_acls,
                    groups,
                    acls,
                })
                .await
                .map_err(|e| e.to_string()),
            None => Err("Not connected".into()),
        }
    }
}
