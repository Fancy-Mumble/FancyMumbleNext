import { PauseIcon, SearchIcon, UserFilledIcon } from "../../icons";
import { useMemo, useRef, useCallback, useState } from "react";
import type { SavedServer, ServerPingResult } from "../../types";
import { isMobile } from "../../utils/platform";
import SwipeableCard from "../elements/SwipeableCard";
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
  /** Called when the user wants to edit a server (long-press on mobile, hover button on desktop). */
  onEdit?: (server: SavedServer) => void;
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

const LONG_PRESS_MS = 500;

/** Avatar wrapper that supports long-press-to-edit on mobile. */
function AvatarWithLongPress({
  server,
  isConnecting,
  ping,
  isMobile,
  disabled,
  onEdit,
  onCancelConnect,
}: Readonly<{
  server: SavedServer;
  isConnecting: boolean;
  ping?: ServerPingResult;
  isMobile: boolean;
  disabled?: boolean;
  onEdit?: (server: SavedServer) => void;
  onCancelConnect?: (id: string) => void;
}>) {
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const firedRef = useRef(false);

  const startPress = useCallback(() => {
    if (!isMobile || !onEdit || disabled || isConnecting) return;
    firedRef.current = false;
    timerRef.current = setTimeout(() => {
      firedRef.current = true;
      onEdit(server);
    }, LONG_PRESS_MS);
  }, [isMobile, onEdit, disabled, isConnecting, server]);

  const cancelPress = useCallback(() => {
    if (timerRef.current) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  return (
    <div
      className={styles.avatarWrap}
      onTouchStart={startPress}
      onTouchEnd={cancelPress}
      onTouchCancel={cancelPress}
      onContextMenu={(e) => {
        // Prevent browser context menu on long-press
        if (isMobile) e.preventDefault();
      }}
    >
      <div className={styles.avatar}>
        {isConnecting ? (
          <button
            type="button"
            className={styles.cancelBtn}
            title="Cancel connection"
            aria-label="Cancel connection"
            onClick={(e) => {
              e.stopPropagation();
              onCancelConnect?.(server.id);
            }}
          >
            <PauseIcon width={14} height={14} />
          </button>
        ) : (
          (server.label || server.host).charAt(0)
        )}
      </div>
      <PingDot ping={ping} />
    </div>
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
  onEdit,
  disabled,
  connectingId,
}: Readonly<Props>) {
  const [searchQuery, setSearchQuery] = useState("");

  // Favourites first, then filter by search query.
  const displayed = useMemo(() => {
    const sorted = [...servers].sort((a, b) => Number(b.favorite) - Number(a.favorite));
    const q = searchQuery.trim().toLowerCase();
    if (!q) return sorted;
    return sorted.filter(
      (s) => (s.label || "").toLowerCase().includes(q) || s.host.toLowerCase().includes(q),
    );
  }, [servers, searchQuery]);

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

      {/* Search bar - only shown when there are saved servers */}
      {servers.length > 0 && (
        <div className={styles.searchWrap}>
          <SearchIcon className={styles.searchIcon} aria-hidden="true" />
          <input
            className={styles.searchInput}
            type="text"
            placeholder="Search servers..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            disabled={disabled}
            aria-label="Search saved servers"
          />
        </div>
      )}

      {servers.length === 0 ? (
        <div className={styles.empty}>
          No saved servers yet.
          <br />
          Add one to get started!
        </div>
      ) : displayed.length === 0 ? (
        <div className={styles.noResults}>No servers match &ldquo;{searchQuery}&rdquo;</div>
      ) : (
        <div className={styles.scrollList}>
        <div className={styles.list}>
          {displayed.map((s) => {
            const isThisConnecting = connectingId === s.id;
            const cardClasses = [
              styles.serverCard,
              isThisConnecting && styles.serverCardConnecting,
            ].filter(Boolean).join(" ");

            return (
              <SwipeableCard
                key={s.id}
                leftSwipeAction={!isThisConnecting ? {
                  label: "Delete",
                  icon: "\u2715",
                  color: "var(--color-danger, #ef4444)",
                  onTrigger: () => onDelete(s.id),
                } : undefined}
                rightSwipeAction={!isThisConnecting ? {
                  label: s.favorite ? "Unfavorite" : "Favorite",
                  icon: s.favorite ? "\u2606" : "\u2605",
                  color: "#f59e0b",
                  onTrigger: () => onToggleFavorite(s.id),
                } : undefined}
                disabled={disabled || isThisConnecting}
              >
              <div
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
                <AvatarWithLongPress
                  server={s}
                  isConnecting={isThisConnecting}
                  ping={pings[s.id]}
                  isMobile={isMobile}
                  disabled={disabled}
                  onEdit={onEdit}
                  onCancelConnect={onCancelConnect}
                />

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

                {/* Action buttons (right side) - fade in on hover */}
                <div className={styles.cardActions}>
                  {onEdit && (
                    <button
                      className={styles.editBtn}
                      title="Edit server"
                      onClick={(e) => {
                        e.stopPropagation();
                        if (!disabled) onEdit(s);
                      }}
                      type="button"
                    >
                      &#x270E;
                    </button>
                  )}
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
                </div>

                {/* Loading bar at the bottom of the card */}
                {isThisConnecting && <div className={styles.connectingBar} />}
              </div>
              </SwipeableCard>
            );
          })}
        </div>
        </div>
      )}
    </div>
  );
}
