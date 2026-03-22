import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { BanEntry } from "../../types";
import styles from "./AdminPanel.module.css";

const EMPTY_BAN: BanEntry = {
  address: "",
  mask: 32,
  name: "",
  hash: "",
  reason: "",
  start: "",
  duration: 0,
};

export function BanListTab() {
  const [bans, setBans] = useState<BanEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [selectedIdx, setSelectedIdx] = useState<number | null>(null);
  const [editing, setEditing] = useState<BanEntry | null>(null);
  const [dirty, setDirty] = useState(false);

  // Listen for ban-list events from the backend.
  useEffect(() => {
    const unlisten = listen<BanEntry[]>("ban-list", (event) => {
      setBans(event.payload);
      setLoading(false);
      setSelectedIdx(null);
      setEditing(null);
      setDirty(false);
    });
    return () => { unlisten.then((f) => f()); };
  }, []);

  // Request the ban list on mount.
  useEffect(() => {
    setLoading(true);
    invoke("request_ban_list").catch(() => setLoading(false));
  }, []);

  const handleRefresh = useCallback(() => {
    setLoading(true);
    invoke("request_ban_list").catch(() => setLoading(false));
  }, []);

  const handleSelect = useCallback((idx: number) => {
    setSelectedIdx(idx);
    setEditing({ ...bans[idx] });
    setDirty(false);
  }, [bans]);

  const handleAdd = useCallback(() => {
    const newBan = { ...EMPTY_BAN };
    setBans((prev) => [...prev, newBan]);
    setSelectedIdx(bans.length);
    setEditing(newBan);
    setDirty(true);
  }, [bans.length]);

  const handleRemove = useCallback(() => {
    if (selectedIdx == null) return;
    const updated = bans.filter((_, i) => i !== selectedIdx);
    setBans(updated);
    setSelectedIdx(null);
    setEditing(null);
    setDirty(true);
  }, [bans, selectedIdx]);

  const patchEditing = useCallback(
    (patch: Partial<BanEntry>) => {
      setEditing((prev) => (prev ? { ...prev, ...patch } : prev));
      setDirty(true);
    },
    [],
  );

  const handleApplyEdit = useCallback(() => {
    if (selectedIdx == null || !editing) return;
    setBans((prev) => prev.map((b, i) => (i === selectedIdx ? { ...editing } : b)));
  }, [selectedIdx, editing]);

  const handleSave = useCallback(async () => {
    // Apply any in-progress edit first.
    const finalBans =
      selectedIdx != null && editing
        ? bans.map((b, i) => (i === selectedIdx ? { ...editing } : b))
        : bans;
    try {
      await invoke("update_ban_list", { bans: finalBans });
      setDirty(false);
      // Refresh from server.
      handleRefresh();
    } catch (err) {
      console.error("Failed to update ban list:", err);
    }
  }, [bans, selectedIdx, editing, handleRefresh]);

  return (
    <>
      <h2 className={styles.panelTitle}>Ban List</h2>

      <div className={styles.toolbar}>
        <button type="button" className={styles.refreshBtn} onClick={handleRefresh} disabled={loading}>
          {loading ? "Loading..." : "Refresh"}
        </button>
        <button type="button" className={styles.addBtn} onClick={handleAdd}>
          + Add Entry
        </button>
        <button
          type="button"
          className={styles.removeBtn}
          onClick={handleRemove}
          disabled={selectedIdx == null}
        >
          Remove
        </button>
        {dirty && (
          <button type="button" className={styles.saveBtn} onClick={handleSave}>
            Save Changes
          </button>
        )}
      </div>

      <div className={styles.splitView}>
        {/* Ban list */}
        <div className={styles.listPane}>
          {bans.length === 0 ? (
            <div className={styles.emptyRow}>
              {loading ? "Loading..." : "No bans"}
            </div>
          ) : (
            bans.map((b, i) => (
              <button
                type="button"
                key={`${b.address}-${b.hash}-${i}`}
                className={`${styles.listItem} ${selectedIdx === i ? styles.listItemActive : ""}`}
                onClick={() => handleSelect(i)}
              >
                <span className={styles.listItemTitle}>{b.name || b.address || "Unknown"}</span>
                <span className={styles.listItemSub}>
                  {b.address}/{b.mask}
                  {b.reason ? ` - ${b.reason}` : ""}
                </span>
              </button>
            ))
          )}
        </div>

        {/* Detail editor */}
        <div className={styles.detailPane}>
          {editing ? (
            <div className={styles.detailForm}>
              <label className={styles.fieldLabel}>
                Username
                <input
                  className={styles.input}
                  type="text"
                  value={editing.name}
                  onChange={(e) => patchEditing({ name: e.target.value })}
                  onBlur={handleApplyEdit}
                />
              </label>
              <label className={styles.fieldLabel}>
                Address
                <input
                  className={styles.input}
                  type="text"
                  value={editing.address}
                  onChange={(e) => patchEditing({ address: e.target.value })}
                  onBlur={handleApplyEdit}
                />
              </label>
              <label className={styles.fieldLabel}>
                Mask
                <input
                  className={styles.input}
                  type="number"
                  min={0}
                  max={128}
                  value={editing.mask}
                  onChange={(e) => patchEditing({ mask: Number(e.target.value) })}
                  onBlur={handleApplyEdit}
                />
              </label>
              <label className={styles.fieldLabel}>
                Reason
                <input
                  className={styles.input}
                  type="text"
                  value={editing.reason}
                  onChange={(e) => patchEditing({ reason: e.target.value })}
                  onBlur={handleApplyEdit}
                />
              </label>
              <label className={styles.fieldLabel}>
                Hash
                <input
                  className={styles.input}
                  type="text"
                  value={editing.hash}
                  onChange={(e) => patchEditing({ hash: e.target.value })}
                  onBlur={handleApplyEdit}
                />
              </label>
              <label className={styles.fieldLabel}>
                Start
                <input
                  className={styles.input}
                  type="text"
                  value={editing.start}
                  placeholder="e.g. 2025-01-01T00:00:00"
                  onChange={(e) => patchEditing({ start: e.target.value })}
                  onBlur={handleApplyEdit}
                />
              </label>
              <label className={styles.fieldLabel}>
                Duration (seconds, 0 = permanent)
                <input
                  className={styles.input}
                  type="number"
                  min={0}
                  value={editing.duration}
                  onChange={(e) => patchEditing({ duration: Number(e.target.value) })}
                  onBlur={handleApplyEdit}
                />
              </label>
            </div>
          ) : (
            <div className={styles.detailEmpty}>Select a ban entry to edit</div>
          )}
        </div>
      </div>
    </>
  );
}
