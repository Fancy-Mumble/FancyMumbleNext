import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import type { UserEntry } from "../types";
import { useAppStore } from "../store";
import { canDeleteMessages } from "./ChannelEditorDialog";
import ConfirmDialog from "./elements/ConfirmDialog";
import Toast, { type ToastData } from "./elements/Toast";
import VolumeIcon from "../assets/icons/audio/volume.svg?react";
import BlockIcon from "../assets/icons/action/block.svg?react";
import MicIcon from "../assets/icons/audio/mic.svg?react";
import MicOffIcon from "../assets/icons/audio/mic-off.svg?react";
import HeadphonesIcon from "../assets/icons/audio/headphones.svg?react";
import HeadphonesOffIcon from "../assets/icons/audio/headphones-off.svg?react";
import StarIcon from "../assets/icons/status/star.svg?react";
import UserPlusIcon from "../assets/icons/user/user-plus.svg?react";
import MessageMinusIcon from "../assets/icons/communication/message-minus.svg?react";
import ImageIcon from "../assets/icons/general/image.svg?react";
import TrashIcon from "../assets/icons/action/trash.svg?react";
import UserXIcon from "../assets/icons/user/user-x.svg?react";
import styles from "./UserContextMenu.module.css";

/** Mumble permission bitmask: Register users (root channel only). */
const PERM_REGISTER = 0x40000;

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
  const channels = useAppStore((s) => s.channels);
  const selectedChannel = useAppStore((s) => s.selectedChannel);
  const deletePchatMessages = useAppStore((s) => s.deletePchatMessages);
  const isSelf = user.session === ownSession;

  const channel = channels.find((c) => c.id === selectedChannel);
  const showDeleteMessages = !isSelf && canDeleteMessages(channel) && user.hash;

  const rootChannel = channels.find((c) => c.id === 0);
  const hasRegisterPerm =
    rootChannel?.permissions != null && (rootChannel.permissions & PERM_REGISTER) !== 0;
  const isUnregistered = user.user_id == null || user.user_id === 0;
  const canRegister = !isSelf && hasRegisterPerm && isUnregistered;

  const menuRef = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<MenuPosition | null>(null);
  const [volume, setVolume] = useState(() => getLocalVolume(user.session));
  const [blocked, setBlocked] = useState(() => isUserBlocked(user.session));
  const [toast, setToast] = useState<ToastData | null>(null);
  const [deleteUserConfirm, setDeleteUserConfirm] = useState(false);

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
          case "register":
            try {
              await invoke("register_user", { session: user.session });
              setToast({ message: `Registered ${user.name}`, variant: "success" });
            } catch (regErr) {
              console.error("register_user failed:", regErr);
              setToast({ message: "Failed to register user", variant: "error" });
            }
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
                <VolumeIcon width={14} height={14} />
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
                <BlockIcon width={14} height={14} />
              </span>
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
                {user.mute ? (
                  <MicIcon width={14} height={14} />
                ) : (
                  <MicOffIcon width={14} height={14} />
                )}
              </span>
              {user.mute ? "Unmute" : "Mute"}
            </button>
            <button type="button" className={styles.menuItem} onClick={() => handleAction("deaf")}>
              <span className={styles.menuIcon}>
                {user.deaf ? (
                  <HeadphonesIcon width={14} height={14} />
                ) : (
                  <HeadphonesOffIcon width={14} height={14} />
                )}
              </span>
              {user.deaf ? "Undeafen" : "Deafen"}
            </button>
            <button type="button" className={styles.menuItem} onClick={() => handleAction("priority")}>
              <span className={styles.menuIcon}>
                <StarIcon width={14} height={14} />
              </span>
              {user.priority_speaker ? "Remove priority" : "Priority speaker"}
            </button>

            {canRegister && (
              <button type="button" className={styles.menuItem} onClick={() => handleAction("register")}>
                <span className={styles.menuIcon}>
                  <UserPlusIcon width={14} height={14} />
                </span>
                Register
              </button>
            )}

            <div className={styles.divider} />

            <button type="button" className={styles.menuItem} onClick={() => handleAction("reset_comment")}>
              <span className={styles.menuIcon}>
                <MessageMinusIcon width={14} height={14} />
              </span>
              Reset comment
            </button>
            <button type="button" className={styles.menuItem} onClick={() => handleAction("remove_avatar")}>
              <span className={styles.menuIcon}>
                <ImageIcon width={14} height={14} />
              </span>
              Remove avatar
            </button>

            <div className={styles.divider} />

            {showDeleteMessages && (
              <button type="button" className={`${styles.menuItem} ${styles.menuItemDanger}`} onClick={() => setDeleteUserConfirm(true)}>
                <span className={styles.menuIcon}>
                  <TrashIcon width={14} height={14} />
                </span>
                Delete messages
              </button>
            )}

            <button type="button" className={`${styles.menuItem} ${styles.menuItemDanger}`} onClick={() => handleAction("kick")}>
              <span className={styles.menuIcon}>
                <UserXIcon width={14} height={14} />
              </span>
              Kick
            </button>
            <button type="button" className={`${styles.menuItem} ${styles.menuItemDanger}`} onClick={() => handleAction("ban")}>
              <span className={styles.menuIcon}>
                <BlockIcon width={14} height={14} />
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

      {/* Delete-by-user confirmation dialog */}
      {deleteUserConfirm && (
        <ConfirmDialog
          title="Delete messages"
          body={`Are you sure you want to delete all messages from ${user.name} in this channel? This action cannot be undone.`}
          confirmLabel="Delete"
          danger
          onConfirm={async () => {
            if (selectedChannel !== null && user.hash) {
              try {
                await deletePchatMessages(selectedChannel, { senderHash: user.hash });
                setToast({ message: `Deleted messages from ${user.name}`, variant: "success" });
              } catch (err) {
                console.error("delete user messages error:", err);
                setToast({ message: "Failed to delete messages", variant: "error" });
              }
            }
            setDeleteUserConfirm(false);
            onClose();
          }}
          onCancel={() => setDeleteUserConfirm(false)}
        />
      )}

      {toast && <Toast {...toast} onDismiss={() => setToast(null)} />}
    </>,
    document.body,
  );
}
