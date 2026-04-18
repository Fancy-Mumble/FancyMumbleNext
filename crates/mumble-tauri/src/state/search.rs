//! Fuzzy super-search across users, channels, groups, and messages.

use fancy_utils::fuzzy;

use super::types::{ChatMessage, PhotoEntry, SearchCategory, SearchFilter, SearchResult};
use super::AppState;

/// Maximum number of results to return per category.
const MAX_PER_CATEGORY: usize = 10;
/// Maximum total results returned.
const MAX_TOTAL: usize = 25;
/// Score threshold - discard results worse than this.
const SCORE_CUTOFF: u32 = fuzzy::DEFAULT_SCORE_CUTOFF;

/// Check whether an HTML message body contains an image tag.
fn body_has_image(body: &str) -> bool {
    let lower = body.to_lowercase();
    lower.contains("<img") || lower.contains("data:image")
}

/// Check whether an HTML message body contains a hyperlink or bare URL.
fn body_has_link(body: &str) -> bool {
    let lower = body.to_lowercase();
    lower.contains("<a ") || lower.contains("http://") || lower.contains("https://")
}

impl AppState {
    /// Fuzzy-search across all data types.
    ///
    /// Returns results sorted by score (best first), capped at [`MAX_TOTAL`].
    ///
    /// `filter` narrows the search scope:
    /// - `None` or `"all"` - search everything (default)
    /// - `"messages"` - only chat messages
    /// - `"photos"` - only messages containing images
    /// - `"users"` - only users
    /// - `"links"` - only messages containing links
    pub fn super_search(&self, query: &str, filter: SearchFilter, channel_id: Option<u32>) -> Vec<SearchResult> {
        let query_lower = query.to_lowercase();
        if query_lower.is_empty() {
            return Vec::new();
        }

        let Ok(state) = self.inner.lock() else {
            return Vec::new();
        };

        let scoped = channel_id.is_some();
        let search_channels = !scoped && filter == SearchFilter::All;
        let search_users = !scoped && matches!(filter, SearchFilter::All | SearchFilter::Users);
        let search_groups = !scoped && filter == SearchFilter::All;
        let search_messages = matches!(filter, SearchFilter::All | SearchFilter::Messages | SearchFilter::Photos | SearchFilter::Links);

        let mut results = Vec::new();

        if search_channels {
            results.extend(search_channels_fuzzy(&state, &query_lower));
        }
        if search_users {
            results.extend(search_users_fuzzy(&state, &query_lower));
        }
        if search_groups {
            results.extend(search_groups_fuzzy(&state, &query_lower));
        }
        if search_messages {
            results.extend(search_messages_fuzzy(&state, &query_lower, query, filter, channel_id, scoped));
        }

        results.sort_by_key(|r| r.score);
        results.truncate(MAX_TOTAL);
        results
    }

    /// Return paginated photo entries extracted from all chat messages.
    ///
    /// Photos are sorted newest-first (by timestamp if available, then
    /// insertion order as fallback).  Pagination is via `offset` / `limit`.
    pub fn get_photos(&self, offset: usize, limit: usize) -> Vec<PhotoEntry> {
        let Ok(state) = self.inner.lock() else {
            return Vec::new();
        };

        let mut entries: Vec<PhotoEntry> = Vec::new();

        // Channel messages
        for (channel_id, msgs) in &state.msgs.by_channel {
            let ch_name = state
                .channels
                .get(channel_id)
                .map(|c| c.name.as_str())
                .unwrap_or("Unknown");
            for msg in msgs {
                if !body_has_image(&msg.body) {
                    continue;
                }
                collect_photos_from_message(
                    msg,
                    Some(*channel_id),
                    None,
                    None,
                    format!("in #{ch_name}"),
                    &mut entries,
                );
            }
        }

        // DM messages
        for msgs in state.msgs.by_dm.values() {
            for msg in msgs {
                if !body_has_image(&msg.body) {
                    continue;
                }
                collect_photos_from_message(
                    msg,
                    None,
                    None,
                    msg.dm_session,
                    format!("DM with {}", msg.sender_name),
                    &mut entries,
                );
            }
        }

        // Group messages
        for (group_id, msgs) in &state.msgs.by_group {
            let group_name = state
                .msgs.group_chats
                .get(group_id)
                .map(|g| g.name.as_str())
                .unwrap_or("Group");
            for msg in msgs {
                if !body_has_image(&msg.body) {
                    continue;
                }
                collect_photos_from_message(
                    msg,
                    None,
                    Some(group_id.clone()),
                    None,
                    format!("in {group_name}"),
                    &mut entries,
                );
            }
        }

        // Sort newest-first.  Messages without a timestamp sort last.
        entries.sort_by(|a, b| {
            let ta = a.timestamp.unwrap_or(0);
            let tb = b.timestamp.unwrap_or(0);
            tb.cmp(&ta)
        });

        // Paginate
        entries.into_iter().skip(offset).take(limit).collect()
    }
}

// -- Helpers -------------------------------------------------------

fn search_channels_fuzzy(state: &super::SharedState, query_lower: &str) -> Vec<SearchResult> {
    let mut results: Vec<SearchResult> = state
        .channels
        .values()
        .filter_map(|ch| {
            let score = fuzzy::fuzzy_score(query_lower, &ch.name.to_lowercase(), SCORE_CUTOFF)?;
            Some(SearchResult {
                category: SearchCategory::Channel, score, title: ch.name.clone(),
                subtitle: None, id: Some(ch.id), string_id: None,
            })
        })
        .collect();
    results.sort_by_key(|r| r.score);
    results.truncate(MAX_PER_CATEGORY);
    results
}

fn search_users_fuzzy(state: &super::SharedState, query_lower: &str) -> Vec<SearchResult> {
    let mut results: Vec<SearchResult> = state
        .users
        .values()
        .filter_map(|u| {
            let score = fuzzy::fuzzy_score(query_lower, &u.name.to_lowercase(), SCORE_CUTOFF)?;
            let ch_name = state.channels.get(&u.channel_id).map(|c| c.name.clone());
            Some(SearchResult {
                category: SearchCategory::User, score, title: u.name.clone(),
                subtitle: ch_name, id: Some(u.session), string_id: None,
            })
        })
        .collect();
    results.sort_by_key(|r| r.score);
    results.truncate(MAX_PER_CATEGORY);
    results
}

fn search_groups_fuzzy(state: &super::SharedState, query_lower: &str) -> Vec<SearchResult> {
    let mut results: Vec<SearchResult> = state
        .msgs.group_chats
        .values()
        .filter_map(|g| {
            let score = fuzzy::fuzzy_score(query_lower, &g.name.to_lowercase(), SCORE_CUTOFF)?;
            let member_count = g.members.len();
            Some(SearchResult {
                category: SearchCategory::Group, score, title: g.name.clone(),
                subtitle: Some(format!(
                    "{member_count} {}",
                    if member_count == 1 { "member" } else { "members" }
                )),
                id: None, string_id: Some(g.id.clone()),
            })
        })
        .collect();
    results.sort_by_key(|r| r.score);
    results.truncate(MAX_PER_CATEGORY);
    results
}

fn search_messages_fuzzy(
    state: &super::SharedState,
    query_lower: &str,
    query: &str,
    filter: SearchFilter,
    channel_id: Option<u32>,
    scoped: bool,
) -> Vec<SearchResult> {
    let filter_photos = filter == SearchFilter::Photos;
    let filter_links = filter == SearchFilter::Links;
    let mut msg_results: Vec<SearchResult> = Vec::new();

    for (ch_id, msgs) in &state.msgs.by_channel {
        if channel_id.is_some_and(|scope| *ch_id != scope) {
            continue;
        }
        let ch_name = state.channels.get(ch_id).map(|c| c.name.as_str()).unwrap_or("Unknown");
        msg_results.extend(collect_channel_message_results(
            msgs.iter(), *ch_id, ch_name, filter_photos, filter_links, query_lower, query,
        ));
    }

    if !scoped {
        for msgs in state.msgs.by_dm.values() {
            msg_results.extend(collect_dm_message_results(
                msgs.iter(), filter_photos, filter_links, query_lower, query,
            ));
        }
        for (group_id, msgs) in &state.msgs.by_group {
            let group_name = state.msgs.group_chats.get(group_id).map(|g| g.name.as_str()).unwrap_or("Group");
            msg_results.extend(collect_group_message_results(
                msgs.iter(), group_id, group_name, filter_photos, filter_links, query_lower, query,
            ));
        }
    }

    msg_results.sort_by_key(|r| r.score);
    msg_results.truncate(MAX_PER_CATEGORY);
    msg_results
}

/// Extract `src` attributes from `<img>` tags in an HTML string.
fn extract_img_srcs(html: &str) -> Vec<String> {
    let mut srcs = Vec::new();
    let lower = html.to_lowercase();
    let mut search_from = 0;
    while let Some(img_pos) = lower[search_from..].find("<img") {
        let abs_pos = search_from + img_pos;
        let tag_end = match html[abs_pos..].find('>') {
            Some(e) => abs_pos + e,
            None => break,
        };
        let tag = &html[abs_pos..=tag_end];
        if let Some(src) = extract_attr(tag, "src") {
            if !src.is_empty() {
                srcs.push(src);
            }
        }
        search_from = tag_end + 1;
    }
    srcs
}

/// Extract the value of an HTML attribute from a tag string.
fn extract_attr(tag: &str, attr: &str) -> Option<String> {
    let lower = tag.to_lowercase();
    let needle = format!("{attr}=");
    let pos = lower.find(&needle)? + needle.len();
    let rest = &tag[pos..];
    let trimmed = rest.trim_start();
    let (quote, start) = match trimmed.as_bytes().first() {
        Some(b'"') => (b'"', 1),
        Some(b'\'') => (b'\'', 1),
        _ => return None,
    };
    let content = &trimmed[start..];
    let end = content.as_bytes().iter().position(|&b| b == quote)?;
    Some(content[..end].to_string())
}

/// Collect [`PhotoEntry`] items from a single message's image `src` attrs.
fn collect_photos_from_message(
    msg: &ChatMessage,
    channel_id: Option<u32>,
    group_id: Option<String>,
    dm_session: Option<u32>,
    context: String,
    entries: &mut Vec<PhotoEntry>,
) {
    for src in extract_img_srcs(&msg.body) {
        entries.push(PhotoEntry {
            src,
            sender_name: msg.sender_name.clone(),
            channel_id,
            group_id: group_id.clone(),
            dm_session,
            context: context.clone(),
            timestamp: msg.timestamp,
        });
    }
}

fn score_one_message(body: &str, filter_photos: bool, filter_links: bool, query_lower: &str) -> Option<u32> {
    if filter_photos && !body_has_image(body) {
        return None;
    }
    if filter_links && !body_has_link(body) {
        return None;
    }
    fuzzy::fuzzy_score(query_lower, &body.to_lowercase(), SCORE_CUTOFF)
}

fn collect_channel_message_results<'a>(
    msgs: impl Iterator<Item = &'a ChatMessage>,
    ch_id: u32,
    ch_name: &'a str,
    filter_photos: bool,
    filter_links: bool,
    query_lower: &'a str,
    query: &'a str,
) -> Vec<SearchResult> {
    msgs.filter_map(|msg| {
        let score = score_one_message(&msg.body, filter_photos, filter_links, query_lower)?;
        Some(SearchResult {
            category: SearchCategory::Message,
            score,
            title: fuzzy::snippet(&msg.body, query, 80),
            subtitle: Some(format!("{} in #{ch_name}", msg.sender_name)),
            id: Some(ch_id),
            string_id: msg.message_id.clone(),
        })
    })
    .collect()
}

fn collect_dm_message_results<'a>(
    msgs: impl Iterator<Item = &'a ChatMessage>,
    filter_photos: bool,
    filter_links: bool,
    query_lower: &'a str,
    query: &'a str,
) -> Vec<SearchResult> {
    msgs.filter_map(|msg| {
        let score = score_one_message(&msg.body, filter_photos, filter_links, query_lower)?;
        Some(SearchResult {
            category: SearchCategory::Message,
            score,
            title: fuzzy::snippet(&msg.body, query, 80),
            subtitle: Some(format!("DM with {}", msg.sender_name)),
            id: msg.dm_session,
            string_id: msg.message_id.clone(),
        })
    })
    .collect()
}

fn collect_group_message_results<'a>(
    msgs: impl Iterator<Item = &'a ChatMessage>,
    group_id: &'a str,
    group_name: &'a str,
    filter_photos: bool,
    filter_links: bool,
    query_lower: &'a str,
    query: &'a str,
) -> Vec<SearchResult> {
    msgs.filter_map(|msg| {
        let score = score_one_message(&msg.body, filter_photos, filter_links, query_lower)?;
        Some(SearchResult {
            category: SearchCategory::Message,
            score,
            title: fuzzy::snippet(&msg.body, query, 80),
            subtitle: Some(format!("{} in {group_name}", msg.sender_name)),
            id: None,
            string_id: Some(group_id.to_owned()),
        })
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_single_img_double_quotes() {
        let html = r#"<p>Hello</p><img src="data:image/png;base64,abc123" alt="pic">"#;
        let srcs = extract_img_srcs(html);
        assert_eq!(srcs, vec!["data:image/png;base64,abc123"]);
    }

    #[test]
    fn extract_single_img_single_quotes() {
        let html = "<img src='https://example.com/photo.jpg'>";
        let srcs = extract_img_srcs(html);
        assert_eq!(srcs, vec!["https://example.com/photo.jpg"]);
    }

    #[test]
    fn extract_multiple_images() {
        let html = r#"<img src="a.png"><p>text</p><img src="b.jpg">"#;
        let srcs = extract_img_srcs(html);
        assert_eq!(srcs, vec!["a.png", "b.jpg"]);
    }

    #[test]
    fn extract_no_images() {
        let srcs = extract_img_srcs("<p>No images here</p>");
        assert!(srcs.is_empty());
    }

    #[test]
    fn extract_img_with_extra_attrs() {
        let html = r#"<img alt="photo" src="pic.webp" width="200">"#;
        let srcs = extract_img_srcs(html);
        assert_eq!(srcs, vec!["pic.webp"]);
    }

    #[test]
    fn extract_attr_double_quotes() {
        assert_eq!(
            extract_attr(r#"<img src="hello.png">"#, "src"),
            Some("hello.png".to_string()),
        );
    }

    #[test]
    fn extract_attr_missing() {
        assert_eq!(extract_attr("<img alt='x'>", "src"), None);
    }

    #[test]
    fn body_has_image_detects_img_tag() {
        assert!(body_has_image(r#"<img src="x.png">"#));
    }

    #[test]
    fn body_has_image_detects_data_uri() {
        assert!(body_has_image("look: data:image/png;base64,abc"));
    }

    #[test]
    fn body_has_image_negative() {
        assert!(!body_has_image("<p>No images</p>"));
    }
}
