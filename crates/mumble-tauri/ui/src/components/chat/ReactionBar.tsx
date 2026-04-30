/**
 * Renders emoji reaction pills beneath a message bubble.
 *
 * Each pill shows the emoji and count.  Clicking toggles the current
 * user's reaction.  A "+" button opens the full emoji picker.
 */

import { useState, useCallback } from "react";
import { createPortal } from "react-dom";
import type { ReactionSummary } from "./reactionStore";
import { isMobile } from "../../utils/platform";
import styles from "./ReactionBar.module.css";

interface ReactionBarProps {
  readonly reactions: readonly ReactionSummary[];
  /** Own cert hash for tracking which reactions are ours. */
  readonly ownHash?: string;
  /** Whether this message is from the current user (controls alignment). */
  readonly isOwn?: boolean;
  /** Called when a user toggles an existing reaction emoji. */
  readonly onToggle: (emoji: string) => void;
  /** Called when the user clicks "+" to open the full picker. */
  readonly onAdd: (e: React.MouseEvent) => void;
}

export default function ReactionBar({
  reactions,
  ownHash,
  isOwn,
  onToggle,
  onAdd,
}: ReactionBarProps) {
  const [tooltip, setTooltip] = useState<{ text: string; x: number; y: number } | null>(null);

  const handleMouseEnter = useCallback(
    (e: React.MouseEvent, reaction: ReactionSummary) => {
      if (isMobile) return;
      const names = [...reaction.reactorHashNames.values()];
      const unique = [...new Set(names)];
      const text =
        unique.length <= 3
          ? unique.join(", ")
          : `${unique.slice(0, 3).join(", ")} +${unique.length - 3}`;
      const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
      setTooltip({ text, x: rect.left + rect.width / 2, y: rect.top - 4 });
    },
    [],
  );

  const handleMouseLeave = useCallback(() => setTooltip(null), []);

  if (reactions.length === 0) return null;

  return (
    <div className={`${styles.reactions} ${isOwn ? styles.reactionsOwn : ""}`}>
      {reactions.map((r) => {
        const totalCount = r.reactorHashes.size;
        const active = !!ownHash && r.reactorHashes.has(ownHash);
        const isImageEmoji = r.emoji.startsWith("data:image/");
        return (
          <button
            key={r.emoji}
            type="button"
            className={`${styles.pill} ${active ? styles.pillActive : ""}`}
            onClick={() => onToggle(r.emoji)}
            onMouseEnter={(e) => handleMouseEnter(e, r)}
            onMouseLeave={handleMouseLeave}
            aria-label={`${isImageEmoji ? ":custom:" : r.emoji} ${totalCount}`}
          >
            {isImageEmoji ? (
              <img src={r.emoji} alt="" className={styles.pillEmojiImg} />
            ) : (
              <span className={styles.pillEmoji}>{r.emoji}</span>
            )}
            <span className={styles.pillCount}>{totalCount}</span>
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
