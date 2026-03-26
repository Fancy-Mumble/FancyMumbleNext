/**
 * PollCard - renders a poll inside the chat message view.
 *
 * Shows the question, options with vote counts, and lets the local
 * user vote. Votes are tracked locally per-poll using Zustand store
 * integration. Visual feedback via animated bar fills.
 */

import { useState, useCallback, useMemo } from "react";
import type { PollPayload, PollVotePayload } from "./PollCreator";
import PollIcon from "../assets/icons/communication/poll.svg?react";
import styles from "./PollCard.module.css";

// --- Vote store (module-level) ------------------------------------

// Votes are stored in-memory per session; not persisted.
// Map: pollId -> array of PollVotePayload
const voteStore = new Map<string, PollVotePayload[]>();

/** Register a vote (from local user or received via plugin message). */
export function registerVote(vote: PollVotePayload) {
  const existing = voteStore.get(vote.pollId) ?? [];
  // Replace any previous vote by same voter.
  const filtered = existing.filter((v) => v.voter !== vote.voter);
  filtered.push(vote);
  voteStore.set(vote.pollId, filtered);
}

/** Get all votes for a poll. */
export function getVotes(pollId: string): PollVotePayload[] {
  return voteStore.get(pollId) ?? [];
}

// --- Poll store (module-level) ------------------------------------

const pollStore = new Map<string, PollPayload>();

/** Register a poll payload so it can be looked up by ID. */
export function registerPoll(poll: PollPayload) {
  pollStore.set(poll.id, poll);
}

/** Get a poll by ID. */
export function getPoll(pollId: string): PollPayload | undefined {
  return pollStore.get(pollId);
}

// --- Local vote tracking ------------------------------------------
// Tracks which polls the LOCAL user has voted on and what they selected,
// independently of session IDs.

const localVotes = new Map<string, number[]>();

/** Mark the local user as having voted on a poll. */
export function registerLocalVote(pollId: string, selected: number[]) {
  localVotes.set(pollId, selected);
}

/** Get the local user's vote for a poll (undefined if not voted). */
export function getLocalVote(pollId: string): number[] | undefined {
  return localVotes.get(pollId);
}

// --- Component ----------------------------------------------------

interface PollCardProps {
  poll: PollPayload;
  /** The local user's session ID. */
  ownSession: number | null;
  /** Whether this poll is inside the local user's own message bubble. */
  isOwn?: boolean;
  /** Called when the user votes. */
  onVote: (pollId: string, selected: number[]) => void;
}

export default function PollCard({ poll, ownSession, isOwn, onVote }: Readonly<PollCardProps>) {
  const [_rev, forceUpdate] = useState(0);

  const votes = getVotes(poll.id);
  // Use both ownSession matching and the local vote map as fallback.
  const myVote =
    ownSession != null
      ? votes.find((v) => v.voter === ownSession)
      : undefined;
  const localVote = getLocalVote(poll.id);
  const hasVoted = !!myVote || !!localVote;
  const mySelected = myVote?.selected ?? localVote ?? [];

  // Count votes per option.
  const voteCounts = useMemo(() => {
    const counts = new Array(poll.options.length).fill(0);
    for (const v of votes) {
      for (const idx of v.selected) {
        if (idx >= 0 && idx < counts.length) counts[idx]++;
      }
    }
    return counts as number[];
  }, [votes, poll.options.length]);

  const totalVoters = votes.length;
  const totalVotes = voteCounts.reduce((a, b) => a + b, 0);

  // For single-choice: track selected option.
  const [pendingSelection, setPendingSelection] = useState<number[]>([]);

  const handleOptionClick = useCallback(
    (idx: number) => {
      if (hasVoted) return; // Already voted.

      if (poll.multiple) {
        setPendingSelection((prev) =>
          prev.includes(idx)
            ? prev.filter((i) => i !== idx)
            : [...prev, idx],
        );
      } else {
        // Single-choice: vote immediately.
        onVote(poll.id, [idx]);
        forceUpdate((n) => n + 1);
      }
    },
    [hasVoted, poll.multiple, poll.id, onVote],
  );

  const handleSubmitMultiple = useCallback(() => {
    if (pendingSelection.length === 0) return;
    onVote(poll.id, pendingSelection);
    setPendingSelection([]);
    forceUpdate((n) => n + 1);
  }, [pendingSelection, poll.id, onVote]);

  return (
    <div className={`${styles.card} ${isOwn ? styles.cardOwn : ""}`}>
      <div className={styles.header}>
        <PollIcon className={styles.pollIcon} width={16} height={16} />
        <span className={styles.pollLabel}>Poll</span>
        <span className={styles.creatorInfo}>by {poll.creatorName}</span>
      </div>

      <h4 className={styles.question}>{poll.question}</h4>

      <div className={styles.options}>
        {poll.options.map((option, i) => {
          const count = voteCounts[i];
          const pct = totalVotes > 0 ? Math.round((count / totalVotes) * 100) : 0;
          const isSelected = hasVoted
            ? mySelected.includes(i)
            : pendingSelection.includes(i);
          // Collect voter names for this option (only after voting).
          const voterNames = hasVoted
            ? votes
                .filter((v) => v.selected.includes(i) && v.voterName)
                .map((v) => v.voterName)
            : [];

          return (
            <button
              key={i}
              className={`${styles.option} ${isSelected ? styles.selected : ""} ${hasVoted ? styles.voted : ""}`}
              onClick={() => handleOptionClick(i)}
              disabled={hasVoted}
            >
              <div
                className={styles.fill}
                style={{ width: hasVoted ? `${pct}%` : "0%" }}
              />
              <span className={styles.optionText}>
                {poll.multiple && !hasVoted && (
                  <span className={styles.checkbox}>
                    {isSelected ? "☑" : "☐"}
                  </span>
                )}
                {!poll.multiple && !hasVoted && (
                  <span className={styles.radio}>
                    {isSelected ? "◉" : "○"}
                  </span>
                )}
                {option}
              </span>
              {hasVoted && (
                <span className={styles.optionPct}>{pct}%</span>
              )}
              {hasVoted && voterNames.length > 0 && (
                <span className={styles.voterNames}>
                  {voterNames.join(", ")}
                </span>
              )}
            </button>
          );
        })}
      </div>

      {poll.multiple && !hasVoted && pendingSelection.length > 0 && (
        <button className={styles.voteBtn} onClick={handleSubmitMultiple}>
          Vote ({pendingSelection.length} selected)
        </button>
      )}

      <div className={styles.footer}>
        <span>{totalVoters} vote{totalVoters !== 1 ? "s" : ""}</span>
        {poll.multiple && <span> · Multiple choice</span>}
      </div>
    </div>
  );
}
