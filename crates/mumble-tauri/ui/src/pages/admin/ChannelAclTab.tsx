import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useAppStore } from "../../store";
import type { AclData, AclEntry, AclGroup, ChannelEntry } from "../../types";
import styles from "./AdminPanel.module.css";

/** Mumble permission bit definitions. */
const PERMISSIONS: { bit: number; label: string }[] = [
  { bit: 0x01, label: "Write" },
  { bit: 0x02, label: "Traverse" },
  { bit: 0x04, label: "Enter" },
  { bit: 0x08, label: "Speak" },
  { bit: 0x10, label: "Mute/Deafen" },
  { bit: 0x20, label: "Move" },
  { bit: 0x40, label: "Make Channel" },
  { bit: 0x80, label: "Link Channel" },
  { bit: 0x100, label: "Whisper" },
  { bit: 0x200, label: "Text Message" },
  { bit: 0x400, label: "Make Temp Channel" },
  { bit: 0x800, label: "Listen" },
  { bit: 0x10000, label: "Kick" },
  { bit: 0x20000, label: "Ban" },
  { bit: 0x40000, label: "Register" },
  { bit: 0x80000, label: "Self-Register" },
  { bit: 0x100000, label: "Reset User Content" },
];

export function ChannelAclTab() {
  const channels = useAppStore((s) => s.channels);
  const [selectedChannel, setSelectedChannel] = useState<number | null>(null);
  const [aclData, setAclData] = useState<AclData | null>(null);
  const [loading, setLoading] = useState(false);
  const [dirty, setDirty] = useState(false);

  // Listen for ACL events from the backend.
  useEffect(() => {
    const unlisten = listen<AclData>("acl", (event) => {
      setAclData(event.payload);
      setLoading(false);
      setDirty(false);
    });
    return () => { unlisten.then((f) => f()); };
  }, []);

  const handleChannelSelect = useCallback((channelId: number) => {
    setSelectedChannel(channelId);
    setLoading(true);
    setAclData(null);
    invoke("request_acl", { channelId }).catch(() => setLoading(false));
  }, []);

  const handleToggleInherit = useCallback(() => {
    if (!aclData) return;
    setAclData({ ...aclData, inherit_acls: !aclData.inherit_acls });
    setDirty(true);
  }, [aclData]);

  const patchGroup = useCallback(
    (idx: number, patch: Partial<AclGroup>) => {
      if (!aclData) return;
      const groups = aclData.groups.map((g, i) => (i === idx ? { ...g, ...patch } : g));
      setAclData({ ...aclData, groups });
      setDirty(true);
    },
    [aclData],
  );

  const addGroup = useCallback(() => {
    if (!aclData) return;
    const newGroup: AclGroup = {
      name: "new_group",
      inherited: false,
      inherit: true,
      inheritable: true,
      add: [],
      remove: [],
      inherited_members: [],
    };
    setAclData({ ...aclData, groups: [...aclData.groups, newGroup] });
    setDirty(true);
  }, [aclData]);

  const removeGroup = useCallback(
    (idx: number) => {
      if (!aclData) return;
      const groups = aclData.groups.filter((_, i) => i !== idx);
      setAclData({ ...aclData, groups });
      setDirty(true);
    },
    [aclData],
  );

  const patchAcl = useCallback(
    (idx: number, patch: Partial<AclEntry>) => {
      if (!aclData) return;
      const acls = aclData.acls.map((a, i) => (i === idx ? { ...a, ...patch } : a));
      setAclData({ ...aclData, acls });
      setDirty(true);
    },
    [aclData],
  );

  const addAcl = useCallback(() => {
    if (!aclData) return;
    const newAcl: AclEntry = {
      apply_here: true,
      apply_subs: true,
      inherited: false,
      user_id: null,
      group: "all",
      grant: 0,
      deny: 0,
    };
    setAclData({ ...aclData, acls: [...aclData.acls, newAcl] });
    setDirty(true);
  }, [aclData]);

  const removeAcl = useCallback(
    (idx: number) => {
      if (!aclData) return;
      const acls = aclData.acls.filter((_, i) => i !== idx);
      setAclData({ ...aclData, acls });
      setDirty(true);
    },
    [aclData],
  );

  const togglePermBit = useCallback(
    (aclIdx: number, field: "grant" | "deny", bit: number) => {
      if (!aclData) return;
      const entry = aclData.acls[aclIdx];
      const current = entry[field];
      patchAcl(aclIdx, { [field]: current ^ bit });
    },
    [aclData, patchAcl],
  );

  const handleSave = useCallback(async () => {
    if (!aclData) return;
    try {
      await invoke("update_acl", { acl: aclData });
      setDirty(false);
    } catch (err) {
      console.error("Failed to update ACL:", err);
    }
  }, [aclData]);

  // Build a flat channel list sorted by name for the selector.
  const sortedChannels = [...channels].sort((a: ChannelEntry, b: ChannelEntry) => a.name.localeCompare(b.name));

  return (
    <>
      <h2 className={styles.panelTitle}>Channel ACL Editor</h2>

      {/* Channel selector */}
      <div className={styles.toolbar}>
        <select
          className={styles.channelSelect}
          value={selectedChannel ?? ""}
          onChange={(e) => {
            const val = e.target.value;
            if (val) handleChannelSelect(Number(val));
          }}
        >
          <option value="">Select a channel...</option>
          {sortedChannels.map((ch) => (
            <option key={ch.id} value={ch.id}>{ch.name}</option>
          ))}
        </select>
        {dirty && (
          <button type="button" className={styles.saveBtn} onClick={handleSave}>
            Save Changes
          </button>
        )}
      </div>

      {loading && <div className={styles.emptyRow}>Loading ACL...</div>}

      {aclData && !loading && (
        <div className={styles.aclContent}>
          {/* Inherit toggle */}
          <label className={styles.checkboxLabel}>
            <input
              type="checkbox"
              checked={aclData.inherit_acls}
              onChange={handleToggleInherit}
            />
            Inherit ACLs from parent channel
          </label>

          {/* Groups section */}
          <div className={styles.aclSection}>
            <div className={styles.aclSectionHeader}>
              <h3 className={styles.aclSectionTitle}>Groups</h3>
              <button type="button" className={styles.addBtn} onClick={addGroup}>
                + Add Group
              </button>
            </div>
            {aclData.groups.length === 0 ? (
              <div className={styles.dimText}>No groups defined</div>
            ) : (
              aclData.groups.map((g, i) => (
                <div key={`group-${i}`} className={styles.aclCard}>
                  <div className={styles.aclCardHeader}>
                    <input
                      className={styles.inputSmall}
                      type="text"
                      value={g.name}
                      disabled={g.inherited}
                      onChange={(e) => patchGroup(i, { name: e.target.value })}
                    />
                    {g.inherited && <span className={styles.inheritBadge}>Inherited</span>}
                    {!g.inherited && (
                      <button type="button" className={styles.removeSmallBtn} onClick={() => removeGroup(i)}>
                        &times;
                      </button>
                    )}
                  </div>
                  <div className={styles.aclCardBody}>
                    <label className={styles.checkboxLabel}>
                      <input type="checkbox" checked={g.inherit} disabled={g.inherited} onChange={(e) => patchGroup(i, { inherit: e.target.checked })} />
                      Inherit
                    </label>
                    <label className={styles.checkboxLabel}>
                      <input type="checkbox" checked={g.inheritable} disabled={g.inherited} onChange={(e) => patchGroup(i, { inheritable: e.target.checked })} />
                      Inheritable
                    </label>
                  </div>
                </div>
              ))
            )}
          </div>

          {/* ACL rules section */}
          <div className={styles.aclSection}>
            <div className={styles.aclSectionHeader}>
              <h3 className={styles.aclSectionTitle}>ACL Rules</h3>
              <button type="button" className={styles.addBtn} onClick={addAcl}>
                + Add Rule
              </button>
            </div>
            {aclData.acls.length === 0 ? (
              <div className={styles.dimText}>No ACL rules defined</div>
            ) : (
              aclData.acls.map((a, i) => (
                <AclRuleCard
                  key={`acl-${i}`}
                  entry={a}
                  index={i}
                  onPatch={patchAcl}
                  onRemove={removeAcl}
                  onToggleBit={togglePermBit}
                />
              ))
            )}
          </div>
        </div>
      )}

      {!selectedChannel && !loading && (
        <div className={styles.detailEmpty}>Select a channel to edit its ACL</div>
      )}
    </>
  );
}

function AclRuleCard({
  entry,
  index,
  onPatch,
  onRemove,
  onToggleBit,
}: Readonly<{
  entry: AclEntry;
  index: number;
  onPatch: (idx: number, patch: Partial<AclEntry>) => void;
  onRemove: (idx: number) => void;
  onToggleBit: (idx: number, field: "grant" | "deny", bit: number) => void;
}>) {
  return (
    <div className={styles.aclCard}>
      <div className={styles.aclCardHeader}>
        <span className={styles.aclRuleLabel}>
          {entry.group ? `@${entry.group}` : entry.user_id != null ? `User #${entry.user_id}` : "Unknown"}
        </span>
        {entry.inherited && <span className={styles.inheritBadge}>Inherited</span>}
        {!entry.inherited && (
          <button type="button" className={styles.removeSmallBtn} onClick={() => onRemove(index)}>
            &times;
          </button>
        )}
      </div>

      <div className={styles.aclCardBody}>
        <div className={styles.aclRuleOptions}>
          <label className={styles.checkboxLabel}>
            <input type="checkbox" checked={entry.apply_here} disabled={entry.inherited} onChange={(e) => onPatch(index, { apply_here: e.target.checked })} />
            Apply here
          </label>
          <label className={styles.checkboxLabel}>
            <input type="checkbox" checked={entry.apply_subs} disabled={entry.inherited} onChange={(e) => onPatch(index, { apply_subs: e.target.checked })} />
            Apply to sub-channels
          </label>
        </div>

        {!entry.inherited && (
          <div className={styles.aclRuleOptions}>
            <label className={styles.fieldLabel}>
              Group
              <input
                className={styles.inputSmall}
                type="text"
                value={entry.group ?? ""}
                onChange={(e) => onPatch(index, { group: e.target.value || null, user_id: null })}
              />
            </label>
            <label className={styles.fieldLabel}>
              User ID
              <input
                className={styles.inputSmall}
                type="number"
                value={entry.user_id ?? ""}
                onChange={(e) => {
                  const val = e.target.value;
                  onPatch(index, { user_id: val ? Number(val) : null, group: null });
                }}
              />
            </label>
          </div>
        )}

        {/* Permission bits */}
        <div className={styles.permGrid}>
          <div className={styles.permHeader}>
            <span>Permission</span>
            <span>Allow</span>
            <span>Deny</span>
          </div>
          {PERMISSIONS.map(({ bit, label }) => (
            <div key={bit} className={styles.permRow}>
              <span className={styles.permLabel}>{label}</span>
              <input
                type="checkbox"
                checked={(entry.grant & bit) !== 0}
                disabled={entry.inherited}
                onChange={() => onToggleBit(index, "grant", bit)}
              />
              <input
                type="checkbox"
                checked={(entry.deny & bit) !== 0}
                disabled={entry.inherited}
                onChange={() => onToggleBit(index, "deny", bit)}
              />
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
