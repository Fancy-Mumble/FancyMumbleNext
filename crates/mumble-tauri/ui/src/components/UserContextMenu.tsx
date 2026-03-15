import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import type { UserEntry } from "../types";
import { useAppStore } from "../store";
import styles from "./UserContextMenu.module.css";

// -- Local per-session state for volume and blocked users ----------

/** Local volume overrides keyed by session ID (0-200, default 100). */
const localVolumes = new Map<number, number>();
/** Blocked user sessions for the current connection. */
const blockedUsers = new Set<number>();

export function getLocalVolume(session: number): number {
  return localVolumes.get(session) ?? 100;
}

export function isUserBlocked(session: number): boolean {
  return blockedUsers.has(session);
}

export function resetLocalState(): void {
  localVolumes.clear();
  blockedUsers.clear();
}

// -- Position computation (overflow-aware) -------------------------

interface MenuPosition {
  top: number;
  left: number;
}

function computePosition(
  clickX: number,
  clickY: number,
  menuEl: HTMLElement,
): MenuPosition {
  const { innerWidth: vw, innerHeight: vh } = window;
  const rect = menuEl.getBoundingClientRect();
  const w = rect.width;
  const h = rect.height;
  const margin = 4;

  let left = clickX;
  let top = clickY;

  // Flip horizontally if overflowing right
  if (left + w + margin > vw) {
    left = Math.max(margin, clickX - w);
  }
  // Flip vertically if overflowing bottom
  if (top + h + margin > vh) {
    top = Math.max(margin, clickY - h);
  }

  return { top, left };
}

// -- Menu component ------------------------------------------------

export interface UserContextMenuState {
  x: number;
  y: number;
  user: UserEntry;
}

interface UserContextMenuProps {
  readonly menu: UserContextMenuState;
  readonly onClose: () => void;
}

export function UserContextMenu({ menu, onClose }: UserContextMenuProps) {
  const { user } = menu;
  const ownSession = useAppStore((s) => s.ownSession);
  const isSelf = user.session === ownSession;

  const menuRef = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<MenuPosition | null>(null);
  const [volume, setVolume] = useState(() => getLocalVolume(user.session));
  const [blocked, setBlocked] = useState(() => isUserBlocked(user.session));

  // Compute position once the menu is rendered and we know its size.
  useEffect(() => {
    if (menuRef.current) {
      setPos(computePosition(menu.x, menu.y, menuRef.current));
    }
  }, [menu.x, menu.y]);

  // Close on Escape.
  useEffect(() => {
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [onClose]);

  // -- Volume handler ----------------------------------------------

  const handleVolumeChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const v = Number(e.target.value);
      setVolume(v);
      localVolumes.set(user.session, v);
    },
    [user.session],
  );

  // -- Block toggle ------------------------------------------------

  const toggleBlock = useCallback(() => {
    if (blockedUsers.has(user.session)) {
      blockedUsers.delete(user.session);
      setBlocked(false);
    } else {
      blockedUsers.add(user.session);
      setBlocked(true);
    }
    onClose();
  }, [user.session, onClose]);

  // -- Admin actions -----------------------------------------------

  const handleAction = useCallback(
    async (action: string) => {
      try {
        switch (action) {
          case "mute":
            await invoke("mute_user", { session: user.session, muted: !user.mute });
            break;
          case "deaf":
            await invoke("deafen_user", { session: user.session, deafened: !user.deaf });
            break;
          case "priority":
            await invoke("set_priority_speaker", {
              session: user.session,
              priority: !user.priority_speaker,
            });
            break;
          case "kick":
            await invoke("kick_user", { session: user.session, reason: null });
            break;
          case "ban":
            await invoke("ban_user", { session: user.session, reason: null });
            break;
          case "reset_comment":
            await invoke("reset_user_comment", { session: user.session });
            break;
          case "remove_avatar":
            await invoke("remove_user_avatar", { session: user.session });
            break;
        }
      } catch (err) {
        console.error(`Admin action "${action}" failed:`, err);
      }
      onClose();
    },
    [user, onClose],
  );

  return createPortal(
    <>
      {/* Invisible overlay to catch clicks outside */}
      <div className={styles.overlay} onClick={onClose} onContextMenu={(e) => { e.preventDefault(); onClose(); }} />
      <div
        ref={menuRef}
        className={styles.menu}
        style={pos ? { top: pos.top, left: pos.left } : { top: menu.y, left: menu.x, visibility: "hidden" }}
      >
        {/* -- Local settings -- */}
        {!isSelf && (
          <>
            <div className={styles.sectionLabel}>Local</div>
            <div className={styles.volumeRow}>
              <span className={styles.menuIcon}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5" />
                  <path d="M19.07 4.93a10 10 0 0 1 0 14.14M15.54 8.46a5 5 0 0 1 0 7.07" />
                </svg>
              </span>
              <input
                type="range"
                className={styles.volumeSlider}
                min={0}
                max={200}
                value={volume}
                onChange={handleVolumeChange}
                title={`Volume: ${volume}%`}
              />
              <span className={styles.volumeValue}>{volume}%</span>
            </div>
            <button type="button" className={`${styles.menuItem} ${blocked ? styles.menuItemDanger : ""}`} onClick={toggleBlock}>
              <span className={styles.menuIcon}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <circle cx="12" cy="12" r="10" />
                  <line x1="4.93" y1="4.93" x2="19.07" y2="19.07" />
                </svg>
              </span>
              {blocked ? "Unblock user" : "Block user"}
            </button>
          </>
        )}

        {/* -- Admin actions -- */}
        {!isSelf && (
          <>
            <div className={styles.divider} />
            <div className={styles.sectionLabel}>Admin</div>
            <button type="button" className={styles.menuItem} onClick={() => handleAction("mute")}>
              <span className={styles.menuIcon}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  {user.mute ? (
                    <>
                      <path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z" />
                      <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
                      <line x1="12" y1="19" x2="12" y2="23" />
                      <line x1="8" y1="23" x2="16" y2="23" />
                    </>
                  ) : (
                    <>
                      <line x1="1" y1="1" x2="23" y2="23" />
                      <path d="M9 9v3a3 3 0 0 0 5.12 2.12M15 9.34V4a3 3 0 0 0-5.94-.6" />
                      <path d="M17 16.95A7 7 0 0 1 5 12v-2m14 0v2c0 .76-.13 1.49-.36 2.18" />
                      <line x1="12" y1="19" x2="12" y2="23" />
                      <line x1="8" y1="23" x2="16" y2="23" />
                    </>
                  )}
                </svg>
              </span>
              {user.mute ? "Unmute" : "Mute"}
            </button>
            <button type="button" className={styles.menuItem} onClick={() => handleAction("deaf")}>
              <span className={styles.menuIcon}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  {user.deaf ? (
                    <path d="M3 14h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-7a9 9 0 0 1 18 0v7a2 2 0 0 1-2 2h-1a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2h3" />
                  ) : (
                    <>
                      <line x1="1" y1="1" x2="23" y2="23" />
                      <path d="M3 14h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-7a9 9 0 0 1 18 0v7a2 2 0 0 1-2 2h-1a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2h3" />
                    </>
                  )}
                </svg>
              </span>
              {user.deaf ? "Undeafen" : "Deafen"}
            </button>
            <button type="button" className={styles.menuItem} onClick={() => handleAction("priority")}>
              <span className={styles.menuIcon}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" />
                </svg>
              </span>
              {user.priority_speaker ? "Remove priority" : "Priority speaker"}
            </button>

            <div className={styles.divider} />

            <button type="button" className={styles.menuItem} onClick={() => handleAction("reset_comment")}>
              <span className={styles.menuIcon}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
                  <line x1="9" y1="10" x2="15" y2="10" />
                </svg>
              </span>
              Reset comment
            </button>
            <button type="button" className={styles.menuItem} onClick={() => handleAction("remove_avatar")}>
              <span className={styles.menuIcon}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <rect x="3" y="3" width="18" height="18" rx="2" ry="2" />
                  <circle cx="8.5" cy="8.5" r="1.5" />
                  <polyline points="21 15 16 10 5 21" />
                </svg>
              </span>
              Remove avatar
            </button>

            <div className={styles.divider} />

            <button type="button" className={`${styles.menuItem} ${styles.menuItemDanger}`} onClick={() => handleAction("kick")}>
              <span className={styles.menuIcon}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <path d="M16 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2" />
                  <circle cx="8.5" cy="7" r="4" />
                  <line x1="18" y1="8" x2="23" y2="13" />
                  <line x1="23" y1="8" x2="18" y2="13" />
                </svg>
              </span>
              Kick
            </button>
            <button type="button" className={`${styles.menuItem} ${styles.menuItemDanger}`} onClick={() => handleAction("ban")}>
              <span className={styles.menuIcon}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <circle cx="12" cy="12" r="10" />
                  <line x1="4.93" y1="4.93" x2="19.07" y2="19.07" />
                </svg>
              </span>
              Ban
            </button>
          </>
        )}

        {/* Self user - minimal menu */}
        {isSelf && (
          <div className={styles.sectionLabel} style={{ padding: "8px 12px" }}>
            No actions available for yourself
          </div>
        )}
      </div>
    </>,
    document.body,
  );
}
