//! Tauri commands for managing custom server emotes via the mumble-file-server
//! plugin's `/emotes` HTTP API.
//!
//! Admin auth is performed by the server using the session JWT advertised
//! in the `fancy-file-server-config` plugin data; the frontend forwards
//! that JWT in a Bearer header.

use std::path::PathBuf;

use reqwest::multipart::{Form, Part};
use serde::{Deserialize, Serialize};

use super::AppState;

const EMOTES_PATH: &str = "/emotes";

/// Request payload for [`AppState::add_custom_emote`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddEmoteRequest {
    /// Base URL advertised by the file-server config (no trailing slash).
    pub base_url: String,
    /// Session JWT used to authenticate as an admin.
    pub session_jwt: String,
    /// Unique short identifier for the emote.
    pub shortcode: String,
    /// Fallback unicode emoji shown when the image can't load.
    pub alias_emoji: String,
    /// Optional human-readable description.
    #[serde(default)]
    pub description: Option<String>,
    /// Local absolute file path for the emote image.
    pub file_path: PathBuf,
    /// MIME type of the emote image (e.g. `image/png`, `image/gif`).
    pub mime_type: String,
}

/// Request payload for [`AppState::remove_custom_emote`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveEmoteRequest {
    pub base_url: String,
    pub session_jwt: String,
    pub shortcode: String,
}

#[derive(Debug, Deserialize)]
struct EmoteUploadResponse {
    shortcode: String,
}

#[derive(Debug, Serialize)]
pub struct AddEmoteResponse {
    pub shortcode: String,
}

#[derive(Debug, Deserialize)]
struct ServerErrorBody {
    error: String,
}

async fn read_error_body(resp: reqwest::Response) -> String {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if let Ok(parsed) = serde_json::from_str::<ServerErrorBody>(&body) {
        format!("{status} - {}", parsed.error)
    } else if body.is_empty() {
        status.to_string()
    } else {
        format!("{status} - {body}")
    }
}

fn endpoint(base_url: &str, suffix: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    format!("{trimmed}{EMOTES_PATH}{suffix}")
}

impl AppState {
    /// Upload a new custom emote to the server.
    pub async fn add_custom_emote(
        &self,
        req: AddEmoteRequest,
    ) -> Result<AddEmoteResponse, String> {
        let bytes = tokio::fs::read(&req.file_path)
            .await
            .map_err(|e| format!("read emote file: {e}"))?;

        let part = Part::bytes(bytes)
            .file_name(
                req.file_path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "emote.bin".to_owned()),
            )
            .mime_str(&req.mime_type)
            .map_err(|e| format!("invalid mime type: {e}"))?;

        let mut form = Form::new()
            .text("shortcode", req.shortcode.clone())
            .text("alias_emoji", req.alias_emoji.clone())
            .part("file", part);
        if let Some(desc) = req.description.clone() {
            form = form.text("description", desc);
        }

        let resp = self
            .http_client
            .post(endpoint(&req.base_url, ""))
            .bearer_auth(&req.session_jwt)
            .multipart(form)
            .send()
            .await
            .map_err(|e| format!("upload emote request failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("add emote failed: {}", read_error_body(resp).await));
        }

        let parsed: EmoteUploadResponse = resp
            .json()
            .await
            .map_err(|e| format!("emote response parse: {e}"))?;
        Ok(AddEmoteResponse { shortcode: parsed.shortcode })
    }

    /// Delete a custom emote from the server by shortcode.
    pub async fn remove_custom_emote(
        &self,
        req: RemoveEmoteRequest,
    ) -> Result<(), String> {
        // Shortcodes are restricted server-side to `[A-Za-z0-9_-]{1,64}` so
        // they need no URL encoding.
        let suffix = format!("/{}", req.shortcode);
        let resp = self
            .http_client
            .delete(endpoint(&req.base_url, &suffix))
            .bearer_auth(&req.session_jwt)
            .send()
            .await
            .map_err(|e| format!("delete emote request failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!(
                "remove emote failed: {}",
                read_error_body(resp).await
            ));
        }
        Ok(())
    }
}
