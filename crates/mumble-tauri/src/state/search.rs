//! Fuzzy super-search across users, channels, groups, and messages.

use super::types::{SearchCategory, SearchResult};
use super::AppState;

/// Maximum number of results to return per category.
const MAX_PER_CATEGORY: usize = 10;
/// Maximum total results returned.
const MAX_TOTAL: usize = 25;
/// Score threshold - discard results worse than this.
const SCORE_CUTOFF: u32 = 500;

impl AppState {
    /// Fuzzy-search across all data types.
    ///
    /// Returns results sorted by score (best first), capped at [`MAX_TOTAL`].
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
                let score = fuzzy_score(&query_lower, &ch.name.to_lowercase())?;
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
                let score = fuzzy_score(&query_lower, &u.name.to_lowercase())?;
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
                let score = fuzzy_score(&query_lower, &g.name.to_lowercase())?;
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
                if let Some(score) = fuzzy_score(&query_lower, &msg.body.to_lowercase()) {
                    msg_results.push(SearchResult {
                        category: SearchCategory::Message,
                        score,
                        title: snippet(&msg.body, query, 80),
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
                if let Some(score) = fuzzy_score(&query_lower, &msg.body.to_lowercase()) {
                    msg_results.push(SearchResult {
                        category: SearchCategory::Message,
                        score,
                        title: snippet(&msg.body, query, 80),
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
                if let Some(score) = fuzzy_score(&query_lower, &msg.body.to_lowercase()) {
                    msg_results.push(SearchResult {
                        category: SearchCategory::Message,
                        score,
                        title: snippet(&msg.body, query, 80),
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

// ── Fuzzy matching ────────────────────────────────────────────────

/// Compute a fuzzy match score between `pattern` (lowercase query) and
/// `text` (lowercase haystack).
///
/// Returns `Some(score)` if the pattern fuzzy-matches the text, where
/// lower scores are better. Returns `None` if no match.
///
/// The algorithm is a simplified Smith-Waterman-style scoring:
/// - Exact substring match gets the best score (0 + length penalty).
/// - Character-by-character fuzzy match allows gaps and transpositions.
/// - Each matched character position contributes; consecutive matches
///   and matches at word boundaries get bonuses.
fn fuzzy_score(pattern: &str, text: &str) -> Option<u32> {
    if pattern.is_empty() {
        return Some(0);
    }

    // Quick exact substring check (best possible match).
    if text.contains(pattern) {
        // Score based on how much "extra" text surrounds the match.
        // Shorter texts matching fully get better scores.
        let len_diff = text.len().saturating_sub(pattern.len()) as u32;
        return Some(len_diff);
    }

    let pattern_chars: Vec<char> = pattern.chars().collect();
    let text_chars: Vec<char> = text.chars().collect();

    if pattern_chars.len() > text_chars.len() + 2 {
        return None;
    }

    // Try fuzzy character-by-character matching allowing skips and
    // single-character transpositions / substitutions.
    let score = fuzzy_match_chars(&pattern_chars, &text_chars)?;

    if score > SCORE_CUTOFF {
        return None;
    }

    Some(score)
}

/// Character-level fuzzy matching with gap penalties and bonuses.
///
/// Uses a greedy forward scan with backtracking for transpositions.
fn fuzzy_match_chars(pattern: &[char], text: &[char]) -> Option<u32> {
    let mut score: u32 = 0;
    let mut text_idx = 0;
    let mut pat_idx = 0;
    let mut consecutive = 0u32;
    let mut matched = 0u32;

    while pat_idx < pattern.len() && text_idx < text.len() {
        let p = pattern[pat_idx];
        let t = text[text_idx];

        if p == t {
            // Direct match
            matched += 1;
            consecutive += 1;
            // Bonus for consecutive matches (subtract from score).
            if consecutive > 1 {
                score = score.saturating_sub(consecutive * 2);
            }
            // Bonus for matching at start or after a word boundary.
            if text_idx == 0 || is_boundary(text[text_idx - 1]) {
                score = score.saturating_sub(10);
            }
            pat_idx += 1;
            text_idx += 1;
        } else if pat_idx + 1 < pattern.len()
            && text_idx + 1 < text.len()
            && pattern[pat_idx + 1] == t
            && p == text[text_idx + 1]
        {
            // Transposition: "ab" in pattern matches "ba" in text.
            matched += 2;
            score += 5; // small penalty for transposition
            pat_idx += 2;
            text_idx += 2;
            consecutive = 0;
        } else {
            // Gap in text (skip one text char).
            score += 3;
            text_idx += 1;
            consecutive = 0;
        }
    }

    // Remaining unmatched pattern characters: each is a substitution/typo.
    let remaining = (pattern.len() - pat_idx) as u32;
    if remaining > 0 {
        // Allow up to ~30% of pattern length as typos.
        let max_typos = (pattern.len() as u32 / 3).max(1);
        if remaining > max_typos {
            return None;
        }
        score += remaining * 15;
        matched += remaining; // count as "matched" for threshold
    }

    // Must match at least 60% of pattern chars (via direct or transposition).
    let min_matched = ((pattern.len() as f32) * 0.6).ceil() as u32;
    if matched < min_matched {
        return None;
    }

    // Penalty for text being much longer than the pattern.
    let len_ratio = text.len() as u32 / (pattern.len() as u32).max(1);
    score += len_ratio;

    Some(score)
}

/// Check if a character is a word boundary (space, punctuation, etc.).
fn is_boundary(c: char) -> bool {
    c.is_whitespace() || c == '_' || c == '-' || c == '.'
}

/// Extract a snippet of `text` around the first occurrence of `query`,
/// capped at `max_len` characters. Strips HTML tags for cleaner display.
fn snippet(text: &str, query: &str, max_len: usize) -> String {
    // Strip HTML tags for display.
    let plain = strip_html(text);

    let lower = plain.to_lowercase();
    let query_lower = query.to_lowercase();

    // Find approximate position of the query in the plain text.
    let pos = lower.find(&query_lower).unwrap_or(0);

    // Center the snippet around the match.
    let half = max_len / 2;
    let start = pos.saturating_sub(half);
    let end = (start + max_len).min(plain.len());
    let start = if end == plain.len() {
        end.saturating_sub(max_len)
    } else {
        start
    };

    let mut s = String::new();
    if start > 0 {
        s.push_str("...");
    }
    s.push_str(&plain[start..end]);
    if end < plain.len() {
        s.push_str("...");
    }
    s
}

/// Minimal HTML tag stripper for search result snippets.
fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        if c == '<' {
            in_tag = true;
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag {
            out.push(c);
        }
    }
    out
}
