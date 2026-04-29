import { ChevronDownIcon, PlayIcon, ScreenShareIcon } from "../../icons";
/**
 * Multi-stream grid overview shown when multiple users are broadcasting
 * in the current channel and no stream is being watched yet.
 * Clicking a card starts watching that stream.
 */
import { useCallback, useState } from "react";
import { useStreamThumbnail } from "./useStreamPreview";
import styles from "./MultiStreamGrid.module.css";

// ---------------------------------------------------------------------------
// Single stream card
// ---------------------------------------------------------------------------

interface StreamCardProps {
  readonly session: number;
  readonly name: string;
  readonly onWatch: (session: number) => void;
}

function StreamCard({ session, name, onWatch }: StreamCardProps) {
  const thumbnail = useStreamThumbnail(session, true);
  const handleClick = useCallback(() => onWatch(session), [session, onWatch]);

  return (
    <button type="button" className={styles.card} onClick={handleClick} aria-label={`Watch ${name}'s stream`}>
      <div className={styles.cardThumb}>
        {thumbnail
          ? <img src={thumbnail} alt="" className={styles.cardImg} />
          : (
            <div className={styles.cardPlaceholder}>
              <ScreenShareIcon width={36} height={36} />
            </div>
          )}

        <div className={styles.cardOverlay}>
          <span className={styles.playBtn}>
            <PlayIcon width={24} height={24} />
          </span>
        </div>

        <span className={styles.liveBadge}>LIVE</span>
      </div>

      <div className={styles.cardFooter}>
        <ScreenShareIcon width={12} height={12} className={styles.footerIcon} />
        <span className={styles.cardName}>{name}</span>
      </div>
    </button>
  );
}

// ---------------------------------------------------------------------------
// Grid container
// ---------------------------------------------------------------------------

interface MultiStreamGridProps {
  readonly broadcasters: { session: number; name: string }[];
  readonly onWatch: (session: number) => void;
}

export default function MultiStreamGrid({ broadcasters, onWatch }: MultiStreamGridProps) {
  const [expanded, setExpanded] = useState(false);

  return (
    <div className={`${styles.container} ${expanded ? styles.containerExpanded : ""}`}>
      <button
        type="button"
        className={styles.gridHeader}
        onClick={() => setExpanded((v) => !v)}
        aria-expanded={expanded}
      >
        <ScreenShareIcon width={14} height={14} />
        <span>
          {broadcasters.length} active stream{broadcasters.length === 1 ? "" : "s"}
        </span>
        <ChevronDownIcon
          width={13}
          height={13}
          className={`${styles.headerChevron} ${expanded ? styles.headerChevronOpen : ""}`}
        />
      </button>

      {expanded && (
        <div className={styles.grid}>
          {broadcasters.map((b) => (
            <StreamCard
              key={b.session}
              session={b.session}
              name={b.name}
              onWatch={onWatch}
            />
          ))}
        </div>
      )}
    </div>
  );
}
