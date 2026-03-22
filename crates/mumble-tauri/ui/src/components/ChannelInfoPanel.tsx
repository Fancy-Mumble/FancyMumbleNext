/**
 * Right-side panel showing channel details (description, name).
 *
 * When the user has Write permission on the channel, an edit button
 * appears to allow inline editing of the channel description and name.
 */

import { useEffect, useState, useCallback, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppStore } from "../store";
import type { ChannelEntry } from "../types";
import { getPreferences } from "../preferencesStorage";
import { canDeleteMessages } from "./ChannelEditorDialog";
import { BioEditor } from "../pages/settings/BioEditor";
import { SafeHtml } from "./SafeHtml";
import { UserListItem, colorFor } from "./UserListItem";
import { UserContextMenu } from "./UserContextMenu";
import type { UserContextMenuState } from "./UserContextMenu";
import styles from "./ChannelInfoPanel.module.css";

/** Mumble permission bitmask: Write (bit 0). */
const PERM_WRITE = 0x01;

/** Named ACL permission bits (must match ACL.h on the server). */
const PERMISSION_BITS: readonly [number, string][] = [
  [0x01, "Write"],
  [0x02, "Traverse"],
  [0x04, "Enter"],
  [0x08, "Speak"],
  [0x10, "MuteDeafen"],
  [0x20, "Move"],
  [0x40, "MakeChannel"],
  [0x80, "LinkChannel"],
  [0x100, "Whisper"],
  [0x200, "TextMessage"],
  [0x400, "MakeTempChannel"],
  [0x800, "Listen"],
  [0x1000, "DeleteMessage"],
];

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
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round">
            <line x1="18" y1="6" x2="6" y2="18" />
            <line x1="6" y1="6" x2="18" y2="18" />
          </svg>
        </button>
        <div className={styles.header}>
          <div className={styles.channelIcon}>
            <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.93a2 2 0 0 1-1.66-.9l-.82-1.2A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13c0 1.1.9 2 2 2Z" />
            </svg>
          </div>
          <h2 className={styles.title}>No channel</h2>
        </div>
      </div>
    );
  }

  return (
    <div className={styles.panel}>
      <button className={styles.closeBtn} onClick={onClose} aria-label="Close">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round">
          <line x1="18" y1="6" x2="6" y2="18" />
          <line x1="6" y1="6" x2="18" y2="18" />
        </svg>
      </button>

      {/* Header */}
      <div className={styles.header}>
        <div className={styles.channelIcon}>
          <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.93a2 2 0 0 1-1.66-.9l-.82-1.2A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13c0 1.1.9 2 2 2Z" />
          </svg>
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
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
                <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" />
              </svg>
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
                    <svg className={styles.memberKeyIcon} width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-label="Has encryption key">
                      <path d="M21 2l-2 2m-7.61 7.61a5.5 5.5 0 1 1-7.778 7.778 5.5 5.5 0 0 1 7.777-7.777zm0 0L15.5 7.5m0 0l3 3L22 7l-3-3m-3.5 3.5L19 4" />
                    </svg>
                  )}
                  {isPersisted && (!u.hash || !holderHashes.has(u.hash)) && (
                    <svg className={styles.memberWarningIcon} width="12" height="12" viewBox="0 0 24 24" fill="currentColor" stroke="none" aria-label="Legacy client - cannot read encrypted messages">
                      <title>Legacy client - cannot read encrypted messages</title>
                      <path d="M12 2L1 21h22L12 2zm0 3.99L19.53 19H4.47L12 5.99zM11 16h2v2h-2zm0-6h2v4h-2z" />
                    </svg>
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
                  <svg className={styles.memberKeyIcon} width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-label="Has encryption key">
                    <path d="M21 2l-2 2m-7.61 7.61a5.5 5.5 0 1 1-7.778 7.778 5.5 5.5 0 0 1 7.777-7.777zm0 0L15.5 7.5m0 0l3 3L22 7l-3-3m-3.5 3.5L19 4" />
                  </svg>
                </div>
              ))}
            </div>
          </>
        )}

        {channelUsers.length === 0 && offlineHolders.length === 0 && (
          <span className={styles.emptyMembers}>No users in this channel</span>
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
            >
              Refresh
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
              {PERMISSION_BITS.map(([bit, name]) => {
                const has = (channel.permissions! & bit) !== 0;
                return (
                  <span
                    key={bit}
                    className={has ? styles.permBitOn : styles.permBitOff}
                    title={`0x${bit.toString(16).toUpperCase()}`}
                  >
                    {name}
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
