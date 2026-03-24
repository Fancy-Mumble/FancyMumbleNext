import { useMemo } from "react";
import type { SavedServer, ServerPingResult } from "../types";
import UserFilledIcon from "../assets/icons/user/user-filled.svg?react";
import PauseIcon from "../assets/icons/status/pause.svg?react";
import styles from "./ServerList.module.css";

interface Props {
  servers: SavedServer[];
  /** Map of server id -> ping result. Missing = still pinging. */
  pings: Record<string, ServerPingResult>;
  onConnect: (server: SavedServer) => void;
  onDelete: (id: string) => void;
  onAddNew: () => void;
  /** Called when the user cancels an in-progress connection attempt. */
  onCancelConnect?: (id: string) => void;
  /** Called when the user toggles the favourite star for a server. */
  onToggleFavorite: (id: string) => void;
  disabled?: boolean;
  /** ID of the server currently being connected to (shows pause button). */
  connectingId?: string | null;
}

/** Quality tier based on latency. */
function latencyTier(ms: number): "great" | "okay" | "poor" {
  if (ms < 30) return "great";
  if (ms < 70) return "okay";
  return "poor";
}

function PingDot({ ping }: Readonly<{ ping?: ServerPingResult }>) {
  if (!ping) {
    return (
      <span className={`${styles.pingDot} ${styles.dotProbing}`} title="Checking..." />
    );
  }
  if (!ping.online) {
    return (
      <span className={`${styles.pingDot} ${styles.dotOffline}`} title="Offline" />
    );
  }
  const ms = ping.latency_ms ?? 0;
  const tier = latencyTier(ms);

  const tierClassMap = {
    great: styles.dotGreat,
    okay: styles.dotOkay,
    poor: styles.dotPoor,
  };
  const tierLabelMap = {
    great: `${ms} ms · Excellent`,
    okay: `${ms} ms · Fair`,
    poor: `${ms} ms · High latency`,
  };

  return (
    <span className={`${styles.pingDot} ${tierClassMap[tier]}`} title={tierLabelMap[tier]} />
  );
}

function UsersInfo({ ping }: Readonly<{ ping?: ServerPingResult }>) {
  if (!ping?.online || ping.user_count == null) return null;
  const text = ping.max_user_count != null
    ? `${ping.user_count}/${ping.max_user_count}`
    : `${ping.user_count}`;
  return (
    <span className={styles.users}>
      {text}
      <UserFilledIcon width={10} height={10} />
    </span>
  );
}

export default function ServerList({
  servers,
  pings,
  onConnect,
  onDelete,
  onAddNew,
  onCancelConnect,
  onToggleFavorite,
  disabled,
  connectingId,
}: Readonly<Props>) {
  // Favourites always appear before non-favourites; relative order is preserved.
  const displayed = useMemo(
    () => [...servers].sort((a, b) => Number(b.favorite) - Number(a.favorite)),
    [servers],
  );

  return (
    <div>
      {/* Header row */}
      <div className={styles.header}>
        <span className={styles.heading}>Saved Servers</span>
        <button
          className={styles.addLink}
          onClick={onAddNew}
          disabled={disabled}
          type="button"
        >
          + Add Server
        </button>
      </div>

      {displayed.length === 0 ? (
        <div className={styles.empty}>
          No saved servers yet.
          <br />
          Add one to get started!
        </div>
      ) : (
        <div className={styles.list}>
          {displayed.map((s) => {
            const isThisConnecting = connectingId === s.id;
            const cardClasses = [
              styles.serverCard,
              isThisConnecting && styles.serverCardConnecting,
            ].filter(Boolean).join(" ");

            return (
              <div
                key={s.id}
                className={cardClasses}
                onClick={() => !disabled && onConnect(s)}
                role="button"
                tabIndex={disabled ? -1 : 0}
                onKeyDown={(e) => {
                  if (!disabled && (e.key === "Enter" || e.key === " ")) {
                    e.preventDefault();
                    onConnect(s);
                  }
                }}
                aria-disabled={disabled}
              >
                {/* Avatar with status dot */}
                <div className={styles.avatarWrap}>
                  <div className={styles.avatar}>
                    {isThisConnecting ? (
                      <button
                        type="button"
                        className={styles.cancelBtn}
                        title="Cancel connection"
                        aria-label="Cancel connection"
                        onClick={(e) => {
                          e.stopPropagation();
                          onCancelConnect?.(s.id);
                        }}
                      >
                        <PauseIcon width={14} height={14} />
                      </button>
                    ) : (
                      (s.label || s.host).charAt(0)
                    )}
                  </div>
                  <PingDot ping={pings[s.id]} />
                </div>

                {/* Info - just label and username */}
                <div className={styles.info}>
                  <div className={styles.label}>{s.label || s.host}</div>
                  <div className={styles.meta}>
                    {isThisConnecting ? "Connecting..." : s.username}
                  </div>
                </div>

                {/* User count - non-favorites only; hidden on hover */}
                {!isThisConnecting && !s.favorite && <UsersInfo ping={pings[s.id]} />}

                {/* Favourite star badge (top-right) - favorites only; hidden on hover */}
                {!isThisConnecting && s.favorite && (
                  <span className={styles.favoriteStarBadge} aria-hidden="true">&#x2605;</span>
                )}

                {/* Delete - visible on hover */}
                <button
                  className={styles.deleteBtn}
                  title="Remove server"
                  onClick={(e) => {
                    e.stopPropagation();
                    if (!disabled) onDelete(s.id);
                  }}
                  type="button"
                >
                  &#x2715;
                </button>

                {/* Favourite star button (bottom-right, below delete) - fades in on hover for all cards */}
                {!isThisConnecting && (
                  <button
                    className={styles.favoriteBtn}
                    title={s.favorite ? "Remove from favourites" : "Add to favourites"}
                    aria-label={s.favorite ? "Remove from favourites" : "Add to favourites"}
                    aria-pressed={s.favorite ?? false}
                    onClick={(e) => {
                      e.stopPropagation();
                      if (!disabled) onToggleFavorite(s.id);
                    }}
                    type="button"
                  >
                    {s.favorite ? "\u2605" : "\u2606"}
                  </button>
                )}

                {/* Loading bar at the bottom of the card */}
                {isThisConnecting && <div className={styles.connectingBar} />}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
