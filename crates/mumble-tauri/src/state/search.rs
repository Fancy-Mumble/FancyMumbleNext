//! Fuzzy super-search across users, channels, groups, and messages.

use fancy_utils::fuzzy;

use super::types::{SearchCategory, SearchResult};
use super::AppState;

/// Maximum number of results to return per category.
const MAX_PER_CATEGORY: usize = 10;
/// Maximum total results returned.
const MAX_TOTAL: usize = 25;
/// Score threshold - discard results worse than this.
const SCORE_CUTOFF: u32 = fuzzy::DEFAULT_SCORE_CUTOFF;

impl AppState {
    /// Fuzzy-search across all data types.
    ///
    /// Returns results sorted by score (best first), capped at [`MAX_TOTAL`].
    #[allow(clippy::too_many_lines, reason = "super_search iterates all data types in one pass for performance")]
    pub fn super_search(&self, query: &str) -> Vec<SearchResult> {
        let query_lower = query.to_lowercase();
        if query_lower.is_empty() {
            return Vec::new();
        }

        let Ok(state) = self.inner.lock() else {
            return Vec::new();
        };

        let mut results = Vec::new();

        // -- Channels --
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

        // -- Users --
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

        // -- Group chats --
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

        // -- Chat messages (channel, DM, group) --
        let mut msg_results: Vec<SearchResult> = Vec::new();

        // Channel messages
        for (channel_id, msgs) in &state.messages {
            let ch_name = state
                .channels
                .get(channel_id)
                .map(|c| c.name.as_str())
                .unwrap_or("Unknown");
            for msg in msgs {
                if let Some(score) = fuzzy::fuzzy_score(&query_lower, &msg.body.to_lowercase(), SCORE_CUTOFF) {
                    msg_results.push(SearchResult {
                        category: SearchCategory::Message,
                        score,
                        title: fuzzy::snippet(&msg.body, query, 80),
                        subtitle: Some(format!("{} in #{ch_name}", msg.sender_name)),
                        id: Some(*channel_id),
                        string_id: msg.message_id.clone(),
                    });
                }
            }
        }

        // DM messages
        for msgs in state.dm_messages.values() {
            for msg in msgs {
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

        // Group messages
        for (group_id, msgs) in &state.group_messages {
            let group_name = state
                .group_chats
                .get(group_id)
                .map(|g| g.name.as_str())
                .unwrap_or("Group");
            for msg in msgs {
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

        msg_results.sort_by_key(|r| r.score);
        msg_results.truncate(MAX_PER_CATEGORY);
        results.extend(msg_results);

        // Final sort and cap
        results.sort_by_key(|r| r.score);
        results.truncate(MAX_TOTAL);
        results
    }
}
