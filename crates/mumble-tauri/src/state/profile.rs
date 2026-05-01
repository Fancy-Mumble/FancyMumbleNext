//! User profile management: set/update comment and avatar texture.

use mumble_protocol::command;
use tauri::Emitter;

use super::AppState;

impl AppState {
    /// Set the user's comment on the connected server.
    pub async fn set_user_comment(&self, comment: String) -> Result<(), String> {
        let (handle, own_session) = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            (state.conn.client_handle.clone(), state.conn.own_session)
        };
        match handle {
            Some(h) => {
                h.send(command::SetComment {
                    comment: comment.clone(),
                })
                .await
                .map_err(|e| e.to_string())?;

                if let Some(session) = own_session {
                    let __session = self.inner.snapshot();
                    let _ = __session.lock().ok().and_then(|mut state| {
                        let user = state.users.get_mut(&session)?;
                        user.comment = (!comment.is_empty()).then_some(comment);
                        Some(())
                    });
                    if let Some(app) = self.app_handle() {
                        let _ = app.emit("state-changed", ());
                    }
                }
                Ok(())
            }
            None => Err("Not connected".into()),
        }
    }

    /// Set the user's avatar texture on the connected server.
    pub async fn set_user_texture(&self, texture: Vec<u8>) -> Result<(), String> {
        let (handle, own_session) = {
            let __session = self.inner.snapshot();
            let state = __session.lock().map_err(|e| e.to_string())?;
            (state.conn.client_handle.clone(), state.conn.own_session)
        };
        match handle {
            Some(h) => {
                h.send(command::SetTexture {
                    texture: texture.clone(),
                })
                .await
                .map_err(|e| e.to_string())?;

                if let Some(session) = own_session {
                    let __session = self.inner.snapshot();
                    let _ = __session.lock().ok().and_then(|mut state| {
                        let user = state.users.get_mut(&session)?;
                        user.texture = (!texture.is_empty()).then_some(texture);
                        Some(())
                    });
                    if let Some(app) = self.app_handle() {
                        let _ = app.emit("state-changed", ());
                    }
                }
                Ok(())
            }
            None => Err("Not connected".into()),
        }
    }
}
