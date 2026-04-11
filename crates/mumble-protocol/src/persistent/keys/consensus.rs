//! Consensus evaluation and epoch fork resolution.

use std::collections::HashMap;

use crate::error::{Error, Result};
use crate::persistent::KeyTrustLevel;

use super::types::ChannelKey;
use super::KeyManager;

impl KeyManager {
    // ---- Consensus evaluation ---------------------------------------

    /// Evaluate consensus after the 10-second collection window closes.
    ///
    /// Returns the resulting trust level and the accepted key bytes (if any).
    pub fn evaluate_consensus(
        &mut self,
        request_id: &str,
        channel_id: u32,
        key_custodians: &[String],
    ) -> Result<(KeyTrustLevel, Option<[u8; 32]>)> {
        let collector = self
            .pending_consensus
            .remove(request_id)
            .ok_or_else(|| Error::InvalidState("no consensus collector".into()))?;

        if collector.responses.is_empty() {
            return Ok((KeyTrustLevel::Unverified, None));
        }

        // Check for key custodian trust shortcut
        for (sender_hash, key_bytes) in &collector.responses {
            if self.is_trusted_authority_internal(sender_hash, channel_id, key_custodians) {
                let mut key = [0u8; 32];
                key.copy_from_slice(key_bytes);
                let _ = self
                    .archive_keys
                    .insert(channel_id, (ChannelKey { key }, KeyTrustLevel::Verified));
                return Ok((KeyTrustLevel::Verified, Some(key)));
            }
        }

        // Compute client-side threshold
        let required_threshold = compute_consensus_threshold(collector.observed_members);

        // Check if all responses agree
        let mut key_groups: HashMap<Vec<u8>, Vec<String>> = HashMap::new();
        for (sender, key_bytes) in &collector.responses {
            key_groups
                .entry(key_bytes.clone())
                .or_default()
                .push(sender.clone());
        }

        if key_groups.len() == 1 {
            // All agree
            let (key_bytes, senders) = key_groups.into_iter().next().ok_or_else(|| {
                Error::InvalidState(
                    "key_groups unexpectedly empty after len == 1 check".into(),
                )
            })?;
            let mut key = [0u8; 32];
            key.copy_from_slice(&key_bytes);

            let trust = if senders.len() as u32 >= required_threshold {
                KeyTrustLevel::Verified
            } else {
                KeyTrustLevel::Unverified
            };

            let _ = self
                .archive_keys
                .insert(channel_id, (ChannelKey { key }, trust));
            Ok((trust, Some(key)))
        } else {
            // Disagreement - check if any custodian key is present
            if let Some(key) = self.find_custodian_key_in_groups(&key_groups, channel_id, key_custodians) {
                let _ = self.archive_keys.insert(
                    channel_id,
                    (ChannelKey { key }, KeyTrustLevel::Verified),
                );
                return Ok((KeyTrustLevel::Verified, Some(key)));
            }

            // No custodian resolution - mark disputed
            // Accept the majority key tentatively
            let (majority_key, _) = key_groups
                .iter()
                .max_by_key(|(_, senders)| senders.len())
                .ok_or_else(|| {
                    Error::InvalidState(
                        "key_groups unexpectedly empty during majority resolution".into(),
                    )
                })?;
            let mut key = [0u8; 32];
            key.copy_from_slice(majority_key);
            let _ = self
                .archive_keys
                .insert(channel_id, (ChannelKey { key }, KeyTrustLevel::Disputed));
            Ok((KeyTrustLevel::Disputed, Some(key)))
        }
    }

    // ---- Epoch fork resolution --------------------------------------

    /// Resolve epoch fork candidates for a (channel, epoch) pair.
    ///
    /// Applies the deterministic tie-breaker: the candidate from the
    /// sender with the lexicographically smallest `cert_hash` wins.
    pub fn resolve_epoch_fork(&mut self, channel_id: u32, epoch: u32) -> Result<Option<String>> {
        let candidates = self
            .pending_epoch_candidates
            .remove(&(channel_id, epoch))
            .unwrap_or_default();

        if candidates.is_empty() {
            return Ok(None);
        }

        // Verify parent_fingerprint chain for each candidate
        let current_epoch_fp = self.current_epoch_fingerprint(channel_id);
        let valid_candidates: Vec<_> = candidates
            .into_iter()
            .filter(|c| {
                if let Some(fp) = current_epoch_fp {
                    c.parent_fingerprint == fp
                } else {
                    true // first epoch, no chain to verify
                }
            })
            .collect();

        if valid_candidates.is_empty() {
            return Err(Error::InvalidState(
                "no valid epoch candidates (parent_fingerprint mismatch)".into(),
            ));
        }

        // Deterministic tie-breaker: lowest cert_hash wins
        let winner = valid_candidates
            .iter()
            .min_by(|a, b| {
                a.sender_hash
                    .to_lowercase()
                    .cmp(&b.sender_hash.to_lowercase())
            })
            .ok_or_else(|| {
                Error::InvalidState(
                    "valid_candidates unexpectedly empty after non-empty check".into(),
                )
            })?;

        let winner_hash = winner.sender_hash.clone();
        let winner_key = winner.epoch_key.clone();

        let _ = self
            .epoch_keys
            .entry(channel_id)
            .or_default()
            .insert(epoch, (winner_key, KeyTrustLevel::Unverified));

        Ok(Some(winner_hash))
    }

    pub(super) fn current_epoch_fingerprint(&self, channel_id: u32) -> Option<[u8; 8]> {
        self.epoch_keys
            .get(&channel_id)
            .and_then(|epochs| epochs.values().next_back())
            .map(|(key, _)| key.fingerprint())
    }

    fn find_custodian_key_in_groups(
        &self,
        key_groups: &HashMap<Vec<u8>, Vec<String>>,
        channel_id: u32,
        key_custodians: &[String],
    ) -> Option<[u8; 32]> {
        for (key_bytes, senders) in key_groups {
            for sender in senders {
                if self.is_trusted_authority_internal(sender, channel_id, key_custodians) {
                    let mut key = [0u8; 32];
                    key.copy_from_slice(key_bytes);
                    return Some(key);
                }
            }
        }
        None
    }
}

/// Compute the consensus threshold from observed member count.
///
/// `required_threshold = clamp(floor(observed_members / 2), 1, 5)`
fn compute_consensus_threshold(observed_members: u32) -> u32 {
    (observed_members / 2).clamp(1, 5)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
    use std::time::Instant;

    use super::super::identity::SeedIdentity;
    use super::super::types::{EpochCandidate, EpochKey};
    use super::super::KeyManager;
    use super::compute_consensus_threshold;
    use crate::persistent::encryption::epoch_fingerprint;

    fn make_key_manager() -> KeyManager {
        let identity = SeedIdentity::from_seed(&[0xAA; 32]).unwrap();
        KeyManager::new(Box::new(identity))
    }

    #[test]
    fn consensus_threshold_computation() {
        assert_eq!(compute_consensus_threshold(0), 1);
        assert_eq!(compute_consensus_threshold(1), 1);
        assert_eq!(compute_consensus_threshold(2), 1);
        assert_eq!(compute_consensus_threshold(3), 1);
        assert_eq!(compute_consensus_threshold(4), 2);
        assert_eq!(compute_consensus_threshold(10), 5);
        assert_eq!(compute_consensus_threshold(100), 5);
    }

    #[test]
    fn epoch_fork_resolution_picks_lowest_hash() {
        let mut km = make_key_manager();

        let candidates = vec![
            EpochCandidate {
                sender_hash: "zzz_hash".into(),
                epoch_key: EpochKey::new([0x01; 32]),
                parent_fingerprint: [0; 8],
                epoch_fingerprint: epoch_fingerprint(&[0x01; 32]),
                received_at: Instant::now(),
            },
            EpochCandidate {
                sender_hash: "aaa_hash".into(),
                epoch_key: EpochKey::new([0x02; 32]),
                parent_fingerprint: [0; 8],
                epoch_fingerprint: epoch_fingerprint(&[0x02; 32]),
                received_at: Instant::now(),
            },
        ];

        let _ = km.pending_epoch_candidates.insert((1, 0), candidates);
        let winner = km.resolve_epoch_fork(1, 0).unwrap();
        assert_eq!(winner, Some("aaa_hash".to_string()));
    }
}
