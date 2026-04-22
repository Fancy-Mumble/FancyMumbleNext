use mumble_protocol::proto::mumble_tcp;
use serde::Serialize;
use tracing::debug;

use super::{HandleMessage, HandlerContext};

#[derive(Serialize, Clone)]
struct EmbedMedia {
    url: String,
    width: Option<i32>,
    height: Option<i32>,
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
    provider: Option<EmbedProvider>,
    author: Option<EmbedAuthor>,
}

#[derive(Serialize, Clone)]
struct LinkPreviewResponsePayload {
    request_id: String,
    embeds: Vec<LinkEmbed>,
}

fn convert_media(media: &mumble_tcp::fancy_link_preview_response::embed::Media) -> EmbedMedia {
    EmbedMedia {
        url: media.url.clone().unwrap_or_default(),
        width: media.width,
        height: media.height,
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
        provider: embed.provider.as_ref().map(|p| EmbedProvider {
            name: p.name.clone().unwrap_or_default(),
            url: p.url.clone(),
        }),
        author: embed.author.as_ref().map(|a| EmbedAuthor {
            name: a.name.clone().unwrap_or_default(),
            url: a.url.clone(),
        }),
    }
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
