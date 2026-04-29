use mumble_protocol::proto::mumble_tcp;

use super::{HandleMessage, HandlerContext};
use crate::state::types::{AclEntryPayload, AclGroupPayload, AclPayload};

impl HandleMessage for mumble_tcp::Acl {
    fn handle(&self, ctx: &HandlerContext) {
        let groups: Vec<AclGroupPayload> = self
            .groups
            .iter()
            .map(|g| AclGroupPayload {
                name: g.name.clone(),
                inherited: g.inherited(),
                inherit: g.inherit(),
                inheritable: g.inheritable(),
                add: g.add.clone(),
                remove: g.remove.clone(),
                inherited_members: g.inherited_members.clone(),
                color: g.color.clone(),
                icon: g.icon.clone(),
                style_preset: g.style_preset.clone(),
                metadata: g
                    .metadata
                    .iter()
                    .map(|kv| {
                        (
                            kv.key.clone(),
                            kv.value.clone().unwrap_or_default(),
                        )
                    })
                    .collect(),
            })
            .collect();

        let acls: Vec<AclEntryPayload> = self
            .acls
            .iter()
            .map(|a| AclEntryPayload {
                apply_here: a.apply_here(),
                apply_subs: a.apply_subs(),
                inherited: a.inherited(),
                user_id: a.user_id,
                group: a.group.clone(),
                grant: a.grant.unwrap_or(0),
                deny: a.deny.unwrap_or(0),
            })
            .collect();

        let payload = AclPayload {
            channel_id: self.channel_id,
            inherit_acls: self.inherit_acls(),
            groups,
            acls,
        };
        ctx.emit("acl", payload);
    }
}
