import type { SavedServer, ServerPingResult } from "../types";
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

export default function ServerList({
  servers,
  pings,
  onConnect,
  onDelete,
  onAddNew,
  onCancelConnect,
  disabled,
  connectingId,
}: Readonly<Props>) {
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

      {servers.length === 0 ? (
        <div className={styles.empty}>
          No saved servers yet.
          <br />
          Add one to get started!
        </div>
      ) : (
        <div className={styles.list}>
          {servers.map((s) => {
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
                        <svg width="14" height="14" viewBox="0 0 14 14" fill="currentColor" aria-hidden="true">
                          <rect x="2" y="1" width="4" height="12" rx="1" />
                          <rect x="8" y="1" width="4" height="12" rx="1" />
                        </svg>
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
                  ✕
                </button>

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
