use mumble_protocol::proto::mumble_tcp;
use serde::Serialize;
use tracing::debug;

use super::{HandleMessage, HandlerContext};

/// Server-side downscaled preview, ready to render in `<img>`.
#[derive(Serialize, Clone)]
struct EmbedPreview {
    /// `data:image/jpeg;base64,...` URL the frontend can drop straight into an
    /// `<img src>` without ever performing a network request to the origin
    /// host.
    data_url: String,
    mime: String,
    width: Option<i32>,
    height: Option<i32>,
}

#[derive(Serialize, Clone)]
struct EmbedMedia {
    /// Original (full-resolution) media URL.  Only fetched on explicit user
    /// action so the user's IP isn't leaked to the origin host by default.
    url: String,
    width: Option<i32>,
    height: Option<i32>,
    /// Bytes the original CDN reported for the source asset.
    original_size: Option<u32>,
    /// Inline server-fetched preview.  When present the UI MUST prefer this
    /// over `url` so the user's IP isn't leaked to the origin.
    preview: Option<EmbedPreview>,
}

#[derive(Serialize, Clone)]
struct EmbedProvider {
    name: String,
    url: Option<String>,
}

#[derive(Serialize, Clone)]
struct EmbedAuthor {
    name: String,
    url: Option<String>,
}

#[derive(Serialize, Clone)]
struct EmbedField {
    name: String,
    value: String,
    inline: bool,
}

#[derive(Serialize, Clone)]
struct LinkEmbed {
    url: Option<String>,
    r#type: Option<String>,
    title: Option<String>,
    description: Option<String>,
    color: Option<i32>,
    site_name: Option<String>,
    thumbnail: Option<EmbedMedia>,
    image: Option<EmbedMedia>,
    video: Option<EmbedMedia>,
    favicon: Option<EmbedMedia>,
    provider: Option<EmbedProvider>,
    author: Option<EmbedAuthor>,
    canonical_url: Option<String>,
    lang: Option<String>,
    published_time: Option<String>,
    modified_time: Option<String>,
    keywords: Vec<String>,
    summary: Option<String>,
    content_type: Option<String>,
    content_length: Option<u64>,
    media_duration: Option<String>,
    nsfw: Option<bool>,
    reading_time: Option<String>,
    fields: Vec<EmbedField>,
    fetched_at: Option<String>,
}

#[derive(Serialize, Clone)]
struct LinkPreviewResponsePayload {
    request_id: String,
    embeds: Vec<LinkEmbed>,
}

fn convert_media(media: &mumble_tcp::fancy_link_preview_response::embed::Media) -> EmbedMedia {
    let preview = media.preview_data.as_ref().and_then(|bytes| {
        if bytes.is_empty() {
            return None;
        }
        let mime = media
            .preview_mime
            .clone()
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| "image/jpeg".to_string());
        // Base64-encode the bytes into a self-contained data URL.  This keeps
        // the binary payload entirely inside the existing IPC bridge - the
        // frontend never needs to make a separate network request to render
        // the preview, which means the user's IP stays unexposed.
        let encoded = base64_encode(bytes);
        Some(EmbedPreview {
            data_url: format!("data:{mime};base64,{encoded}"),
            mime,
            width: media.preview_width,
            height: media.preview_height,
        })
    });

    EmbedMedia {
        url: media.url.clone().unwrap_or_default(),
        width: media.width,
        height: media.height,
        original_size: media.original_size,
        preview,
    }
}

fn convert_embed(embed: &mumble_tcp::fancy_link_preview_response::Embed) -> LinkEmbed {
    LinkEmbed {
        url: embed.url.clone(),
        r#type: embed.r#type.clone(),
        title: embed.title.clone(),
        description: embed.description.clone(),
        color: embed.color,
        site_name: embed.site_name.clone(),
        thumbnail: embed.thumbnail.as_ref().map(convert_media),
        image: embed.image.as_ref().map(convert_media),
        video: embed.video.as_ref().map(convert_media),
        favicon: embed.favicon.as_ref().map(convert_media),
        provider: embed.provider.as_ref().map(|p| EmbedProvider {
            name: p.name.clone().unwrap_or_default(),
            url: p.url.clone(),
        }),
        author: embed.author.as_ref().map(|a| EmbedAuthor {
            name: a.name.clone().unwrap_or_default(),
            url: a.url.clone(),
        }),
        canonical_url: embed.canonical_url.clone(),
        lang: embed.lang.clone(),
        published_time: embed.published_time.clone(),
        modified_time: embed.modified_time.clone(),
        keywords: embed.keywords.clone(),
        summary: embed.summary.clone(),
        content_type: embed.content_type.clone(),
        content_length: embed.content_length,
        media_duration: embed.media_duration.clone(),
        nsfw: embed.nsfw,
        reading_time: embed.reading_time.clone(),
        fields: embed
            .fields
            .iter()
            .map(|f| EmbedField {
                name: f.name.clone().unwrap_or_default(),
                value: f.value.clone().unwrap_or_default(),
                inline: f.r#inline.unwrap_or(false),
            })
            .collect(),
        fetched_at: embed.fetched_at.clone(),
    }
}

/// Minimal RFC 4648 base64 encoder.  Avoids pulling in a new dependency for a
/// tiny job (the whole point of inlining the preview is to keep the IPC
/// simple).
fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 3 <= input.len() {
        let b0 = input[i];
        let b1 = input[i + 1];
        let b2 = input[i + 2];
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        out.push(TABLE[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        out.push(TABLE[(b2 & 0x3f) as usize] as char);
        i += 3;
    }
    let rem = input.len() - i;
    if rem == 1 {
        let b0 = input[i];
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[((b0 & 0x03) << 4) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let b0 = input[i];
        let b1 = input[i + 1];
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        out.push(TABLE[((b1 & 0x0f) << 2) as usize] as char);
        out.push('=');
    }
    out
}

impl HandleMessage for mumble_tcp::FancyLinkPreviewResponse {
    fn handle(&self, ctx: &HandlerContext) {
        let request_id = self.request_id.clone().unwrap_or_default();
        let embeds: Vec<LinkEmbed> = self.embeds.iter().map(convert_embed).collect();

        debug!(
            request_id = %request_id,
            embed_count = embeds.len(),
            "received link preview response"
        );

        ctx.emit(
            "link-preview-response",
            LinkPreviewResponsePayload {
                request_id,
                embeds,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }
}
