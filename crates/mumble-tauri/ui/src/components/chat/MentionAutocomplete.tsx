/**
 * MentionAutocomplete - dropdown popup shown above the chat composer
 * when the user types `@` to mention someone or a role.
 *
 * Pure presentational component: receives a filtered list of candidates
 * and emits a `pick` callback with the selected item.  Trigger detection
 * and state management live in the parent (ChatComposer).
 */

import { useEffect, useRef, type KeyboardEvent } from "react";
import styles from "./MentionAutocomplete.module.css";
import { colorFor } from "../../utils/format";

export type MentionCandidate =
  | {
      readonly kind: "user";
      readonly session: number;
      readonly name: string;
      readonly avatarUrl?: string;
    }
  | {
      readonly kind: "role";
      readonly name: string;
    }
  | {
      readonly kind: "everyone";
    }
  | {
      readonly kind: "here";
    };

export interface MentionAutocompleteProps {
  readonly candidates: readonly MentionCandidate[];
  readonly activeIndex: number;
  readonly onPick: (candidate: MentionCandidate) => void;
  readonly onActiveIndexChange: (index: number) => void;
}

/**
 * Decide how a candidate should be displayed.
 *
 * Extracted as a tiny helper to keep the row JSX readable.
 */
function candidateLabel(c: MentionCandidate): { label: string; hint: string } {
  switch (c.kind) {
    case "user":
      return { label: c.name, hint: "User" };
    case "role":
      return { label: `@${c.name}`, hint: "Role" };
    case "everyone":
      return { label: "@everyone", hint: "Notify the whole channel" };
    case "here":
      return { label: "@here", hint: "Notify online users in this channel" };
  }
}

function candidateKey(c: MentionCandidate, idx: number): string {
  switch (c.kind) {
    case "user":
      return `u-${c.session}`;
    case "role":
      return `r-${c.name}`;
    case "everyone":
      return "everyone";
    case "here":
      return "here";
    default:
      return `i-${idx}`;
  }
}

export default function MentionAutocomplete({
  candidates,
  activeIndex,
  onPick,
  onActiveIndexChange,
}: MentionAutocompleteProps) {
  const listRef = useRef<HTMLUListElement>(null);

  // Keep the active item scrolled into view.
  useEffect(() => {
    const list = listRef.current;
    if (!list) return;
    const item = list.querySelector<HTMLLIElement>(`li[data-idx="${activeIndex}"]`);
    item?.scrollIntoView({ block: "nearest" });
  }, [activeIndex]);

  if (candidates.length === 0) {
    return (
      <div className={styles.popup} role="listbox">
        <div className={styles.empty}>No matches</div>
      </div>
    );
  }

  return (
    <div className={styles.popup} role="listbox" aria-label="Mention suggestions">
      <ul ref={listRef} className={styles.list}>
        {candidates.map((c, idx) => {
          const { label, hint } = candidateLabel(c);
          const active = idx === activeIndex;
          return (
            <li
              key={candidateKey(c, idx)}
              data-idx={idx}
              className={`${styles.item} ${active ? styles.itemActive : ""}`}
              role="option"
              aria-selected={active}
              onMouseEnter={() => onActiveIndexChange(idx)}
              onMouseDown={(e) => {
                // Prevent textarea blur (which would dismiss the popup).
                e.preventDefault();
                onPick(c);
              }}
            >
              <CandidateIcon candidate={c} />
              <span className={styles.itemLabel}>{label}</span>
              <span className={styles.itemHint}>{hint}</span>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

function CandidateIcon({ candidate }: { readonly candidate: MentionCandidate }) {
  if (candidate.kind === "user") {
    if (candidate.avatarUrl) {
      return <img src={candidate.avatarUrl} alt="" className={styles.avatar} />;
    }
    return (
      <div
        className={styles.avatarFallback}
        style={{ background: colorFor(candidate.name) }}
      >
        {candidate.name.charAt(0).toUpperCase()}
      </div>
    );
  }
  return <div className={styles.iconBadge} aria-hidden>@</div>;
}

/**
 * Convenience handler for the parent: process arrow keys, Enter, Tab,
 * Escape against the candidate list and return the resulting action.
 */
export function handleMentionKey(
  e: KeyboardEvent<HTMLTextAreaElement>,
  state: { activeIndex: number; count: number },
):
  | { kind: "move"; index: number }
  | { kind: "pick"; index: number }
  | { kind: "close" }
  | null {
  if (state.count === 0) return null;
  switch (e.key) {
    case "ArrowDown":
      return { kind: "move", index: (state.activeIndex + 1) % state.count };
    case "ArrowUp":
      return {
        kind: "move",
        index: (state.activeIndex - 1 + state.count) % state.count,
      };
    case "Enter":
    case "Tab":
      return { kind: "pick", index: state.activeIndex };
    case "Escape":
      return { kind: "close" };
    default:
      return null;
  }
}
