/**
 * Renders emoji reaction pills beneath a message bubble.
 *
 * Each pill shows the emoji and count.  Clicking toggles the current
 * user's reaction.  A "+" button opens the full emoji picker.
 */

import { useState, useCallback, useMemo } from "react";
import { createPortal } from "react-dom";
import type { ReactionSummary } from "./reactionStore";
import { isMobile } from "../../utils/platform";
import styles from "./ReactionBar.module.css";

interface ReactionBarProps {
  readonly reactions: readonly ReactionSummary[];
  readonly ownSession: number | null;
  /** Called when a user toggles an existing reaction emoji. */
  readonly onToggle: (emoji: string) => void;
  /** Called when the user clicks "+" to open the full picker. */
  readonly onAdd: (e: React.MouseEvent) => void;
}

export default function ReactionBar({
  reactions,
  ownSession,
  onToggle,
  onAdd,
}: ReactionBarProps) {
  const [tooltip, setTooltip] = useState<{ text: string; x: number; y: number } | null>(null);

  const handleMouseEnter = useCallback(
    (e: React.MouseEvent, reaction: ReactionSummary) => {
      if (isMobile) return;
      const names = [...reaction.reactorNames.values()];
      const text =
        names.length <= 3
          ? names.join(", ")
          : `${names.slice(0, 3).join(", ")} +${names.length - 3}`;
      const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
      setTooltip({ text, x: rect.left + rect.width / 2, y: rect.top - 4 });
    },
    [],
  );

  const handleMouseLeave = useCallback(() => setTooltip(null), []);

  // Memoize sorted reactions so render order is stable.
  const sorted = useMemo(
    () => [...reactions].sort((a, b) => b.reactors.size - a.reactors.size),
    [reactions],
  );

  if (sorted.length === 0) return null;

  return (
    <div className={styles.reactions}>
      {sorted.map((r) => {
        const active = ownSession !== null && r.reactors.has(ownSession);
        return (
          <button
            key={r.emoji}
            type="button"
            className={`${styles.pill} ${active ? styles.pillActive : ""}`}
            onClick={() => onToggle(r.emoji)}
            onMouseEnter={(e) => handleMouseEnter(e, r)}
            onMouseLeave={handleMouseLeave}
            aria-label={`${r.emoji} ${r.reactors.size}`}
          >
            <span className={styles.pillEmoji}>{r.emoji}</span>
            <span className={styles.pillCount}>{r.reactors.size}</span>
          </button>
        );
      })}
      <button
        type="button"
        className={styles.addBtn}
        onClick={(e) => onAdd(e)}
        aria-label="Add reaction"
      >
        +
      </button>

      {/* Tooltip portal */}
      {tooltip &&
        createPortal(
          <div
            className={styles.tooltip}
            style={{ left: tooltip.x, top: tooltip.y, transform: "translate(-50%, -100%)" }}
          >
            {tooltip.text}
          </div>,
          document.body,
        )}
    </div>
  );
}
