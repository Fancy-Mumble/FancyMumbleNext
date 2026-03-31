/**
 * Right-side panel showing channel details (description, name).
 *
 * When the user has Write permission on the channel, an edit button
 * appears to allow inline editing of the channel description and name.
 */

import { useEffect, useState, useCallback, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppStore } from "../../store";
import type { ChannelEntry } from "../../types";
import { getPreferences } from "../../preferencesStorage";
import { canDeleteMessages, hasPermission } from "./ChannelEditorDialog";
import { BioEditor } from "../../pages/settings/BioEditor";
import { SafeHtml } from "../elements/SafeHtml";
import { UserListItem, colorFor } from "./UserListItem";
import { UserContextMenu } from "./UserContextMenu";
import type { UserContextMenuState } from "./UserContextMenu";
import CloseIcon from "../../assets/icons/action/close.svg?react";
import FolderIcon from "../../assets/icons/general/folder.svg?react";
import EditIcon from "../../assets/icons/action/edit.svg?react";
import KeyIcon from "../../assets/icons/status/key.svg?react";
import WarningFilledIcon from "../../assets/icons/status/warning-filled.svg?react";
import RefreshIcon from "../../assets/icons/action/refresh.svg?react";
import styles from "./ChannelInfoPanel.module.css";
import { PERMISSIONS, PERM_KEY_OWNER } from "../../utils/permissions";

/** Mumble permission bitmask: Write (bit 0). */
const PERM_WRITE = 0x01;

interface ChannelInfoPanelProps {
  readonly onClose: () => void;
}

export default function ChannelInfoPanel({ onClose }: ChannelInfoPanelProps) {
  const selectedChannel = useAppStore((s) => s.selectedChannel);
  const channels = useAppStore((s) => s.channels);

  const channel: ChannelEntry | undefined = channels.find(
    (c) => c.id === selectedChannel,
  );

  const [editing, setEditing] = useState(false);
  const [editName, setEditName] = useState("");
  const [editDescription, setEditDescription] = useState("");
  const [saving, setSaving] = useState(false);

  const users = useAppStore((s) => s.users);
  const selectDmUser = useAppStore((s) => s.selectDmUser);

  // Key holder state
  const keyHolders = useAppStore((s) => s.keyHolders);
  const queryKeyHolders = useAppStore((s) => s.queryKeyHolders);
  const getPersistenceMode = useAppStore((s) => s.getPersistenceMode);

  useEffect(() => {
    if (selectedChannel != null) {
      queryKeyHolders(selectedChannel);
    }
  }, [selectedChannel, queryKeyHolders]);

  const currentHolders = selectedChannel != null ? keyHolders[selectedChannel] ?? [] : [];

  // Build a set of cert hashes that are key holders for fast lookups.
  const holderHashes = useMemo(
    () => new Set(currentHolders.map((h) => h.cert_hash)),
    [currentHolders],
  );

  // Derive online status from the live users list.
  const onlineUserHashes = useMemo(
    () => new Set(users.map((u) => u.hash).filter(Boolean)),
    [users],
  );

  // Offline key holders: holders not currently connected to the server.
  const offlineHolders = useMemo(
    () => currentHolders.filter((h) => !onlineUserHashes.has(h.cert_hash)),
    [currentHolders, onlineUserHashes],
  );

  const channelUsers = useMemo(
    () => users.filter((u) => u.channel_id === selectedChannel),
    [users, selectedChannel],
  );

  const isPersisted =
    selectedChannel != null && getPersistenceMode(selectedChannel) !== "NONE";

  const [devMode, setDevMode] = useState(false);

  useEffect(() => {
    getPreferences()
      .then((prefs) => setDevMode(prefs.userMode === "developer"))
      .catch(() => {});
  }, []);

  const [userCtxMenu, setUserCtxMenu] = useState<UserContextMenuState | null>(
    null,
  );

  const openUserCtxMenu = useCallback(
    (e: React.MouseEvent, session: number) => {
      e.preventDefault();
      const u = users.find((u) => u.session === session);
      if (!u) return;
      setUserCtxMenu({ x: e.clientX, y: e.clientY, user: u });
    },
    [users],
  );

  const canWrite =
    channel?.permissions != null && (channel.permissions & PERM_WRITE) !== 0;

  const canKeyOwner = hasPermission(channel, PERM_KEY_OWNER) && isPersisted;

  const [confirmTakeover, setConfirmTakeover] = useState<"full_wipe" | "key_only" | null>(null);

  const handleKeyTakeover = useCallback(async () => {
    if (!channel || !confirmTakeover) return;
    try {
      await invoke("key_takeover", { channelId: channel.id, mode: confirmTakeover });
    } finally {
      setConfirmTakeover(null);
    }
  }, [channel, confirmTakeover]);

  // Sync edit fields when channel changes or editing starts.
  useEffect(() => {
    if (channel) {
      setEditName(channel.name);
      setEditDescription(channel.description);
    }
  }, [channel, editing]);

  const startEditing = useCallback(() => setEditing(true), []);
  const cancelEditing = useCallback(() => setEditing(false), []);

  const saveChanges = useCallback(async () => {
    if (!channel) return;
    setSaving(true);
    try {
      const nameChanged = editName !== channel.name ? editName : undefined;
      const descChanged =
        editDescription !== channel.description ? editDescription : undefined;
      if (nameChanged !== undefined || descChanged !== undefined) {
        await invoke("update_channel", {
          channelId: channel.id,
          name: nameChanged ?? null,
          description: descChanged ?? null,
        });
      }
      setEditing(false);
    } finally {
      setSaving(false);
    }
  }, [channel, editName, editDescription]);

  if (!channel) {
    return (
      <div className={styles.panel}>
        <button className={styles.closeBtn} onClick={onClose} aria-label="Close">
          <CloseIcon width={14} height={14} />
        </button>
        <div className={styles.header}>
          <div className={styles.channelIcon}>
            <FolderIcon width={24} height={24} />
          </div>
          <h2 className={styles.title}>No channel</h2>
        </div>
      </div>
    );
  }

  return (
    <div className={styles.panel}>
      <button className={styles.closeBtn} onClick={onClose} aria-label="Close">
        <CloseIcon width={14} height={14} />
      </button>

      {/* Header */}
      <div className={styles.header}>
        <div className={styles.channelIcon}>
          <FolderIcon width={24} height={24} />
        </div>
        <div>
          <h2 className={styles.title}># {channel.name}</h2>
          <span className={styles.subtitle}>
            {channel.user_count} {channel.user_count === 1 ? "member" : "members"}
          </span>
        </div>
      </div>

      {/* Channel info section */}
      <div className={styles.section}>
        <div className={styles.sectionHeader}>
          <h3 className={styles.sectionTitle}>Channel</h3>
          {canWrite && !editing && (
            <button
              className={styles.editBtn}
              onClick={startEditing}
              title="Edit channel"
            >
              <EditIcon width={14} height={14} />
            </button>
          )}
        </div>

        {editing ? (
          <div className={styles.editForm}>
            <label className={styles.editLabel}>
              Name
              <input
                className={styles.editInput}
                value={editName}
                onChange={(e) => setEditName(e.target.value)}
              />
            </label>
            <label className={styles.editLabel}>
              Description
              <BioEditor
                value={editDescription}
                onChange={setEditDescription}
                placeholder="Channel description..."
              />
            </label>
            <div className={styles.editActions}>
              <button
                className={styles.cancelBtn}
                onClick={cancelEditing}
                disabled={saving}
              >
                Cancel
              </button>
              <button
                className={styles.saveBtn}
                onClick={saveChanges}
                disabled={saving}
              >
                {saving ? "Saving..." : "Save"}
              </button>
            </div>
          </div>
        ) : (
          <>
            <div className={styles.infoGrid}>
              <span className={styles.infoLabel}>Name</span>
              <span className={styles.infoValue}>{channel.name}</span>
            </div>
            <div className={styles.descriptionSection}>
              <span className={styles.infoLabel}>Description</span>
              <SafeHtml
                html={channel.description}
                className={styles.descriptionContent}
                fallback={<em>No description</em>}
              />
            </div>
          </>
        )}
      </div>

      {/* Members section: online users + offline key holders */}
      <div className={styles.section}>
        <div className={styles.sectionHeader}>
          <h3 className={styles.sectionTitle}>
            Members ({channelUsers.length + offlineHolders.length})
          </h3>
        </div>

        {/* Online: users currently in the channel */}
        {channelUsers.length > 0 && (
          <>
            <span className={styles.subsectionLabel}>Online - {channelUsers.length}</span>
            <div className={styles.membersList}>
              {channelUsers.map((u) => (
                <div key={u.session} className={styles.memberRow}>
                  <UserListItem
                    user={u}
                    onClick={() => selectDmUser(u.session)}
                    onContextMenu={(e) => openUserCtxMenu(e, u.session)}
                  />
                  {u.hash && holderHashes.has(u.hash) && (
                    <KeyIcon className={styles.memberKeyIcon} width={12} height={12} aria-label="Has encryption key" />
                  )}
                  {isPersisted && (!u.hash || !holderHashes.has(u.hash)) && (
                    <WarningFilledIcon className={styles.memberWarningIcon} width={12} height={12} aria-label="Legacy client - cannot read encrypted messages">
                      <title>Legacy client - cannot read encrypted messages</title>
                    </WarningFilledIcon>
                  )}
                </div>
              ))}
            </div>
          </>
        )}

        {/* Offline: key holders not currently connected */}
        {offlineHolders.length > 0 && (
          <>
            <span className={styles.subsectionLabel}>Offline — {offlineHolders.length}</span>
            <div className={styles.holdersList}>
              {offlineHolders.map((holder) => (
                <div key={holder.cert_hash} className={`${styles.holderItem} ${styles.holderOffline}`}>
                  <div className={styles.holderAvatarWrap}>
                    <div
                      className={styles.holderAvatar}
                      style={{ background: colorFor(holder.name) }}
                    >
                      {holder.name.charAt(0).toUpperCase()}
                    </div>
                  </div>
                  <span className={styles.holderName}>{holder.name}</span>
                  <KeyIcon className={styles.memberKeyIcon} width={12} height={12} aria-label="Has encryption key" />
                </div>
              ))}
            </div>
          </>
        )}

        {channelUsers.length === 0 && offlineHolders.length === 0 && (
          <span className={styles.emptyMembers}>No users in this channel</span>
        )}

        {/* Key ownership takeover (requires KeyOwner permission) */}
        {canKeyOwner && (
          <div className={styles.keyTakeoverSection}>
            {confirmTakeover == null ? (
              <button
                className={styles.dangerBtn}
                onClick={() => setConfirmTakeover("full_wipe")}
              >
                <KeyIcon width={14} height={14} />
                Reset Key Ownership
              </button>
            ) : (
              <div className={styles.keyTakeoverConfirm}>
                <span className={styles.keyTakeoverLabel}>Takeover mode:</span>
                <div className={styles.keyTakeoverOptions}>
                  <label className={styles.keyTakeoverOption}>
                    <input
                      type="radio"
                      name="takeoverMode"
                      checked={confirmTakeover === "full_wipe"}
                      onChange={() => setConfirmTakeover("full_wipe")}
                    />
                    <span>Full wipe</span>
                    <span className={styles.keyTakeoverHint}>Delete all messages &amp; take key ownership</span>
                  </label>
                  <label className={styles.keyTakeoverOption}>
                    <input
                      type="radio"
                      name="takeoverMode"
                      checked={confirmTakeover === "key_only"}
                      onChange={() => setConfirmTakeover("key_only")}
                    />
                    <span>Key only</span>
                    <span className={styles.keyTakeoverHint}>Take key ownership, keep messages</span>
                  </label>
                </div>
                <div className={styles.editActions}>
                  <button className={styles.cancelBtn} onClick={() => setConfirmTakeover(null)}>
                    Cancel
                  </button>
                  <button className={styles.dangerBtn} onClick={handleKeyTakeover}>
                    Confirm
                  </button>
                </div>
              </div>
            )}
          </div>
        )}
      </div>

      {userCtxMenu && (
        <UserContextMenu
          menu={userCtxMenu}
          onClose={() => setUserCtxMenu(null)}
        />
      )}

      {/* Developer permissions debug section */}
      {devMode && (
        <div className={styles.section}>
          <div className={styles.sectionHeader}>
            <h3 className={styles.sectionTitle}>Permissions (Dev)</h3>
            <button
              className={styles.editBtn}
              onClick={() => {
              useAppStore.getState().refreshState();
              }}
              title="Force refresh state"
              aria-label="Refresh"
            >
              <RefreshIcon width={14} height={14} />
            </button>
          </div>
          <div className={styles.infoGrid}>
            <span className={styles.infoLabel}>Channel ID</span>
            <span className={styles.infoValue} style={{ fontFamily: "monospace" }}>
              {channel.id}
            </span>
            <span className={styles.infoLabel}>Raw</span>
            <span className={styles.infoValue} style={{ fontFamily: "monospace" }}>
              {channel.permissions != null
                ? `0x${channel.permissions.toString(16).toUpperCase().padStart(8, "0")} (${channel.permissions})`
                : "null (not queried)"}
            </span>
            <span className={styles.infoLabel}>canDelete</span>
            <span className={styles.infoValue} style={{ fontFamily: "monospace" }}>
              {String(canDeleteMessages(channel))}
            </span>
            <span className={styles.infoLabel}>All channels</span>
            <span className={styles.infoValue} style={{ fontFamily: "monospace", fontSize: "11px", whiteSpace: "pre-wrap", maxHeight: "150px", overflowY: "auto", display: "block" }}>
              {channels.map((c) =>
                `#${c.id} ${c.name}: ${c.permissions != null ? `0x${c.permissions.toString(16).toUpperCase()}` : "null"}`
              ).join("\n")}
            </span>
          </div>
          {channel.permissions != null && (
            <div className={styles.permBits}>
              {PERMISSIONS.map(({ bit, label }) => {
                const has = (channel.permissions! & bit) !== 0;
                return (
                  <span
                    key={bit}
                    className={has ? styles.permBitOn : styles.permBitOff}
                    title={`0x${bit.toString(16).toUpperCase()}`}
                  >
                    {label}
                  </span>
                );
              })}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
