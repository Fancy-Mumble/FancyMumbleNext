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
    #[allow(clippy::too_many_lines, reason = "super_search iterates all data types in one pass for performance")]
    pub fn super_search(&self, query: &str, filter: SearchFilter, channel_id: Option<u32>) -> Vec<SearchResult> {
        let query_lower = query.to_lowercase();
        if query_lower.is_empty() {
            return Vec::new();
        }

        let Ok(state) = self.inner.lock() else {
            return Vec::new();
        };

        // When scoped to a specific channel, only search messages in that channel.
        let scoped = channel_id.is_some();
        let search_channels = !scoped && filter == SearchFilter::All;
        let search_users = !scoped && matches!(filter, SearchFilter::All | SearchFilter::Users);
        let search_groups = !scoped && filter == SearchFilter::All;
        let search_messages = matches!(filter, SearchFilter::All | SearchFilter::Messages | SearchFilter::Photos | SearchFilter::Links);
        let filter_photos = filter == SearchFilter::Photos;
        let filter_links = filter == SearchFilter::Links;

        let mut results = Vec::new();

        // -- Channels --
        if search_channels {
            let mut channel_results: Vec<SearchResult> = state
                .channels
                .values()
                .filter_map(|ch| {
                    let score = fuzzy::fuzzy_score(&query_lower, &ch.name.to_lowercase(), SCORE_CUTOFF)?;
                    Some(SearchResult {
                        category: SearchCategory::Channel,
                        score,
                        title: ch.name.clone(),
                        subtitle: None,
                        id: Some(ch.id),
                        string_id: None,
                    })
                })
                .collect();
            channel_results.sort_by_key(|r| r.score);
            channel_results.truncate(MAX_PER_CATEGORY);
            results.extend(channel_results);
        }

        // -- Users --
        if search_users {
            let mut user_results: Vec<SearchResult> = state
                .users
                .values()
                .filter_map(|u| {
                    let score = fuzzy::fuzzy_score(&query_lower, &u.name.to_lowercase(), SCORE_CUTOFF)?;
                    let ch_name = state
                        .channels
                        .get(&u.channel_id)
                        .map(|c| c.name.clone());
                    Some(SearchResult {
                        category: SearchCategory::User,
                        score,
                        title: u.name.clone(),
                        subtitle: ch_name,
                        id: Some(u.session),
                        string_id: None,
                    })
                })
                .collect();
            user_results.sort_by_key(|r| r.score);
            user_results.truncate(MAX_PER_CATEGORY);
            results.extend(user_results);
        }

        // -- Group chats --
        if search_groups {
            let mut group_results: Vec<SearchResult> = state
                .group_chats
                .values()
                .filter_map(|g| {
                    let score = fuzzy::fuzzy_score(&query_lower, &g.name.to_lowercase(), SCORE_CUTOFF)?;
                    let member_count = g.members.len();
                    Some(SearchResult {
                        category: SearchCategory::Group,
                        score,
                        title: g.name.clone(),
                        subtitle: Some(format!(
                            "{member_count} {}",
                            if member_count == 1 { "member" } else { "members" }
                        )),
                        id: None,
                        string_id: Some(g.id.clone()),
                    })
                })
                .collect();
            group_results.sort_by_key(|r| r.score);
            group_results.truncate(MAX_PER_CATEGORY);
            results.extend(group_results);
        }

        // -- Chat messages (channel, DM, group) --
        if search_messages {
            let mut msg_results: Vec<SearchResult> = Vec::new();

            // Channel messages
            for (ch_id, msgs) in &state.messages {
                if let Some(scope) = channel_id {
                    if *ch_id != scope {
                        continue;
                    }
                }
                let ch_name = state
                    .channels
                    .get(ch_id)
                    .map(|c| c.name.as_str())
                    .unwrap_or("Unknown");
                for msg in msgs {
                    if filter_photos && !body_has_image(&msg.body) {
                        continue;
                    }
                    if filter_links && !body_has_link(&msg.body) {
                        continue;
                    }
                    if let Some(score) = fuzzy::fuzzy_score(&query_lower, &msg.body.to_lowercase(), SCORE_CUTOFF) {
                        msg_results.push(SearchResult {
                            category: SearchCategory::Message,
                            score,
                            title: fuzzy::snippet(&msg.body, query, 80),
                            subtitle: Some(format!("{} in #{ch_name}", msg.sender_name)),
                            id: Some(*ch_id),
                            string_id: msg.message_id.clone(),
                        });
                    }
                }
            }

            // DM messages - skip when scoped to a channel
            if !scoped {
                for msgs in state.dm_messages.values() {
                    for msg in msgs {
                        if filter_photos && !body_has_image(&msg.body) {
                            continue;
                        }
                        if filter_links && !body_has_link(&msg.body) {
                            continue;
                        }
                        if let Some(score) = fuzzy::fuzzy_score(&query_lower, &msg.body.to_lowercase(), SCORE_CUTOFF) {
                            msg_results.push(SearchResult {
                                category: SearchCategory::Message,
                                score,
                                title: fuzzy::snippet(&msg.body, query, 80),
                                subtitle: Some(format!("DM with {}", msg.sender_name)),
                                id: msg.dm_session,
                                string_id: msg.message_id.clone(),
                            });
                        }
                    }
                }
            }

            // Group messages - skip when scoped to a channel
            if !scoped {
                for (group_id, msgs) in &state.group_messages {
                    let group_name = state
                        .group_chats
                        .get(group_id)
                        .map(|g| g.name.as_str())
                        .unwrap_or("Group");
                    for msg in msgs {
                        if filter_photos && !body_has_image(&msg.body) {
                            continue;
                        }
                        if filter_links && !body_has_link(&msg.body) {
                            continue;
                        }
                        if let Some(score) = fuzzy::fuzzy_score(&query_lower, &msg.body.to_lowercase(), SCORE_CUTOFF) {
                            msg_results.push(SearchResult {
                                category: SearchCategory::Message,
                                score,
                                title: fuzzy::snippet(&msg.body, query, 80),
                                subtitle: Some(format!("{} in {group_name}", msg.sender_name)),
                                id: None,
                                string_id: Some(group_id.clone()),
                            });
                        }
                    }
                }
            }

            msg_results.sort_by_key(|r| r.score);
            msg_results.truncate(MAX_PER_CATEGORY);
            results.extend(msg_results);
        }

        // Final sort and cap
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
        for (channel_id, msgs) in &state.messages {
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
        for msgs in state.dm_messages.values() {
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
        for (group_id, msgs) in &state.group_messages {
            let group_name = state
                .group_chats
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
