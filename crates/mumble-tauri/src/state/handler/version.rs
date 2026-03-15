use mumble_protocol::proto::mumble_tcp;

use super::{HandleMessage, HandlerContext};
use crate::state::types::ServerVersionInfo;

impl HandleMessage for mumble_tcp::Version {
    fn handle(&self, ctx: &HandlerContext) {
        if let Ok(mut state) = ctx.shared.lock() {
            state.server_fancy_version = self.fancy_version;
            state.server_version_info = ServerVersionInfo {
                release: self.release.clone(),
                os: self.os.clone(),
                os_version: self.os_version.clone(),
                version_v1: self.version_v1,
                version_v2: self.version_v2,
                fancy_version: self.fancy_version,
            };
        }
    }
}
