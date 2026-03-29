import { useState, useCallback, useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import type { ChannelEntry, PchatProtocol } from "../types";
import { useAppStore } from "../store";
import { BioEditor } from "../pages/settings/BioEditor";
import styles from "./ChannelEditorDialog.module.css";

/** Mumble permission bitmask constants (must match ACL.h on the server). */
const PERM_WRITE = 0x01;
const PERM_MAKE_CHANNEL = 0x40;
const PERM_MAKE_TEMP_CHANNEL = 0x400;
export const PERM_DELETE_MESSAGE = 0x1000;

/** Check whether a channel's cached permissions include a specific bit.
 *  Returns `false` when permissions have not been queried yet. */
export function hasPermission(channel: ChannelEntry | undefined, bit: number): boolean {
  if (!channel) return false;
  if (channel.permissions == null) return false;
  return (channel.permissions & bit) !== 0;
}

/** Can the user edit this channel? (requires Write permission) */
export function canEditChannel(channel: ChannelEntry | undefined): boolean {
  return hasPermission(channel, PERM_WRITE);
}

/** Can the user create a sub-channel? (requires MakeChannel or MakeTempChannel) */
export function canCreateChannel(channel: ChannelEntry | undefined): boolean {
  return (
    hasPermission(channel, PERM_MAKE_CHANNEL) ||
    hasPermission(channel, PERM_MAKE_TEMP_CHANNEL)
  );
}

/** Can only create temporary channels (has MakeTempChannel but not MakeChannel). */
export function canOnlyCreateTemp(channel: ChannelEntry | undefined): boolean {
  return (
    !hasPermission(channel, PERM_MAKE_CHANNEL) &&
    hasPermission(channel, PERM_MAKE_TEMP_CHANNEL)
  );
}

/** Can the user delete this channel? (requires Write permission; root channel 0 cannot be deleted) */
export function canDeleteChannel(channel: ChannelEntry | undefined): boolean {
  if (!channel) return false;
  if (channel.id === 0) return false;
  return hasPermission(channel, PERM_WRITE);
}

/** Can the user delete persistent chat messages in this channel?
 *  Requires the dedicated DeleteMessage permission bit.
 *  Returns `false` when permissions have not been queried yet. */
export function canDeleteMessages(channel: ChannelEntry | undefined): boolean {
  if (!channel || channel.permissions == null) return false;
  return (channel.permissions & PERM_DELETE_MESSAGE) !== 0;
}

// ---- Dialog types ------------------------------------------------

interface ChannelEditorProps {
  /** The channel being edited, or `null` when creating a new one. */
  readonly channel: ChannelEntry | null;
  /** Parent channel ID (required when creating). */
  readonly parentId: number;
  /** Whether the user can only create temporary channels. */
  readonly tempOnly?: boolean;
  readonly onClose: () => void;
}

// ---- Component ---------------------------------------------------

export default function ChannelEditorDialog({
  channel,
  parentId,
  tempOnly = false,
  onClose,
}: ChannelEditorProps) {
  const isCreate = channel === null;
  const createChannel = useAppStore((s) => s.createChannel);
  const updateChannel = useAppStore((s) => s.updateChannel);

  // Form state - initialised from existing channel or defaults.
  const [name, setName] = useState(channel?.name ?? "");
  const [description, setDescription] = useState(channel?.description ?? "");
  const [position, setPosition] = useState(channel?.position ?? 0);
  const [temporary, setTemporary] = useState(
    tempOnly ? true : (channel?.temporary ?? false),
  );
  const [maxUsers, setMaxUsers] = useState(channel?.max_users ?? 0);

  // Persistence settings
  const [pchatProtocol, setPchatProtocol] = useState<PchatProtocol>(
    channel?.pchat_protocol ?? "none",
  );
  const [pchatMaxHistory, setPchatMaxHistory] = useState(
    channel?.pchat_max_history ?? 0,
  );
  const [pchatRetentionDays, setPchatRetentionDays] = useState(
    channel?.pchat_retention_days ?? 0,
  );

  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const backdropRef = useRef<HTMLDivElement>(null);

  // Close on Escape.
  useEffect(() => {
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [onClose]);

  // Close on backdrop click.
  const handleBackdropClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === backdropRef.current) onClose();
    },
    [onClose],
  );

  const handleSubmit = useCallback(async () => {
    if (!name.trim()) return;
    setSubmitting(true);
    setError(null);
    try {
      const pchatOpts =
        pchatProtocol !== "none"
          ? {
              pchatProtocol,
              pchatMaxHistory: pchatMaxHistory || undefined,
              pchatRetentionDays: pchatRetentionDays || undefined,
            }
          : { pchatProtocol };

      if (isCreate) {
        await createChannel(parentId, name.trim(), {
          description: description || undefined,
          position: position || undefined,
          temporary: temporary || undefined,
          maxUsers: maxUsers || undefined,
          ...pchatOpts,
        });
      } else {
        await updateChannel(channel.id, {
          name: name.trim() !== channel.name ? name.trim() : undefined,
          description:
            description !== channel.description ? description : undefined,
          position: position !== channel.position ? position : undefined,
          temporary: temporary !== channel.temporary ? temporary : undefined,
          maxUsers: maxUsers !== channel.max_users ? maxUsers : undefined,
          ...pchatOpts,
        });
      }
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setSubmitting(false);
    }
  }, [
    name,
    description,
    position,
    temporary,
    maxUsers,
    pchatProtocol,
    pchatMaxHistory,
    pchatRetentionDays,
    isCreate,
    channel,
    parentId,
    createChannel,
    updateChannel,
    onClose,
  ]);

  return createPortal(
    // eslint-disable-next-line jsx-a11y/no-static-element-interactions
    <div
      ref={backdropRef}
      className={styles.backdrop}
      onClick={handleBackdropClick}
      onKeyDown={(e) => {
        if (e.key === "Escape") onClose();
      }}
    >
      <div className={styles.dialog} role="dialog" aria-modal="true" aria-label={isCreate ? "Create Channel" : "Edit Channel"}>
        <h3 className={styles.title}>{isCreate ? "Create Channel" : "Edit Channel"}</h3>

        {/* Name */}
        <div className={styles.field}>
          <label className={styles.label} htmlFor="ch-ed-name">Name</label>
          <input
            id="ch-ed-name"
            className={styles.input}
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Channel name"
            autoFocus
          />
        </div>

        {/* Description */}
        <div className={styles.field}>
          <span className={styles.label}>Description</span>
          <BioEditor
            value={description}
            onChange={setDescription}
            placeholder="Optional description"
          />
        </div>

        {/* Position & Max Users */}
        <div className={styles.row}>
          <div className={styles.field}>
            <label className={styles.label} htmlFor="ch-ed-pos">Position</label>
            <input
              id="ch-ed-pos"
              className={styles.input}
              type="number"
              value={position}
              onChange={(e) => setPosition(Number(e.target.value))}
              min={0}
            />
          </div>
          <div className={styles.field}>
            <label className={styles.label} htmlFor="ch-ed-max">Max Users</label>
            <input
              id="ch-ed-max"
              className={styles.input}
              type="number"
              value={maxUsers}
              onChange={(e) => setMaxUsers(Number(e.target.value))}
              min={0}
            />
            <span className={styles.hint}>0 = unlimited</span>
          </div>
        </div>

        {/* Temporary */}
        <div className={styles.checkboxRow}>
          <input
            id="ch-ed-temp"
            className={styles.checkbox}
            type="checkbox"
            checked={temporary}
            onChange={(e) => setTemporary(e.target.checked)}
            disabled={tempOnly}
          />
          <label className={styles.checkboxLabel} htmlFor="ch-ed-temp">
            Temporary channel
          </label>
        </div>

        {/* Persistence settings */}
        <div className={styles.section}>
          <h4 className={styles.sectionTitle}>Persistence</h4>

          <div className={styles.field}>
            <label className={styles.label} htmlFor="ch-ed-pchat">Protocol</label>
            <select
              id="ch-ed-pchat"
              className={styles.select}
              value={pchatProtocol}
              onChange={(e) => setPchatProtocol(e.target.value as PchatProtocol)}
            >
              <option value="none">None (standard volatile chat)</option>
              <option value="fancy_v1_post_join">Post-Join (history from first join)</option>
              <option value="fancy_v1_full_archive">Full Archive (all messages)</option>
              <option value="server_managed">Server Managed</option>
            </select>
          </div>

          {pchatProtocol !== "none" && (
            <div className={styles.row}>
              <div className={styles.field}>
                <label className={styles.label} htmlFor="ch-ed-maxhist">
                  Max History
                </label>
                <input
                  id="ch-ed-maxhist"
                  className={styles.input}
                  type="number"
                  value={pchatMaxHistory}
                  onChange={(e) => setPchatMaxHistory(Number(e.target.value))}
                  min={0}
                />
                <span className={styles.hint}>0 = unlimited</span>
              </div>
              <div className={styles.field}>
                <label className={styles.label} htmlFor="ch-ed-ret">
                  Retention (days)
                </label>
                <input
                  id="ch-ed-ret"
                  className={styles.input}
                  type="number"
                  value={pchatRetentionDays}
                  onChange={(e) => setPchatRetentionDays(Number(e.target.value))}
                  min={0}
                />
                <span className={styles.hint}>0 = forever</span>
              </div>
            </div>
          )}
        </div>

        {error && <p className={styles.error}>{error}</p>}

        <div className={styles.actions}>
          <button className={styles.cancelBtn} onClick={onClose} type="button">
            Cancel
          </button>
          <button
            className={styles.submitBtn}
            onClick={handleSubmit}
            disabled={submitting || !name.trim()}
            type="button"
          >
            {submitting
              ? "Saving..."
              : isCreate
                ? "Create"
                : "Save"}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
