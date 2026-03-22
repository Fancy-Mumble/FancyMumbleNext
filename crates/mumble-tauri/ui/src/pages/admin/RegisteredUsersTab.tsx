import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { RegisteredUser, RegisteredUserUpdate } from "../../types";
import { formatRelativeDate } from "../../utils/format";
import styles from "./AdminPanel.module.css";

type SortKey = "name" | "last_seen" | "last_channel";
type SortDir = "asc" | "desc";

export function RegisteredUsersTab() {
  const [users, setUsers] = useState<RegisteredUser[]>([]);
  const [loading, setLoading] = useState(false);
  const [search, setSearch] = useState("");
  const [sortKey, setSortKey] = useState<SortKey>("name");
  const [sortDir, setSortDir] = useState<SortDir>("asc");
  const searchRef = useRef<HTMLInputElement>(null);

  // Inline-edit state: which user_id is being renamed and its draft name.
  const [editingId, setEditingId] = useState<number | null>(null);
  const [editName, setEditName] = useState("");
  const editRef = useRef<HTMLInputElement>(null);

  // Pending delete confirmation.
  const [deletingId, setDeletingId] = useState<number | null>(null);

  // Listen for user-list events from the backend.
  useEffect(() => {
    const unlisten = listen<RegisteredUser[]>("user-list", (event) => {
      setUsers(event.payload);
      setLoading(false);
    });
    return () => { unlisten.then((f) => f()); };
  }, []);

  // Request the user list on mount.
  useEffect(() => {
    setLoading(true);
    invoke("request_user_list").catch(() => setLoading(false));
  }, []);

  const handleRefresh = useCallback(() => {
    setLoading(true);
    invoke("request_user_list").catch(() => setLoading(false));
  }, []);

  const toggleSort = useCallback(
    (key: SortKey) => {
      if (sortKey === key) {
        setSortDir((d) => (d === "asc" ? "desc" : "asc"));
      } else {
        setSortKey(key);
        setSortDir("asc");
      }
    },
    [sortKey],
  );

  // --- Rename ---
  const startRename = useCallback((user: RegisteredUser) => {
    setEditingId(user.user_id);
    setEditName(user.name);
    setDeletingId(null);
    // Focus the input on next render.
    setTimeout(() => editRef.current?.focus(), 0);
  }, []);

  const cancelRename = useCallback(() => {
    setEditingId(null);
    setEditName("");
  }, []);

  const submitRename = useCallback(async () => {
    if (editingId === null) return;
    const trimmed = editName.trim();
    if (!trimmed) return;
    const update: RegisteredUserUpdate = { user_id: editingId, name: trimmed };
    await invoke("update_user_list", { users: [update] });
    setEditingId(null);
    setEditName("");
    // Refresh the list after the server processes the change.
    setLoading(true);
    invoke("request_user_list").catch(() => setLoading(false));
  }, [editingId, editName]);

  // --- Delete ---
  const confirmDelete = useCallback((userId: number) => {
    setDeletingId(userId);
    setEditingId(null);
  }, []);

  const cancelDelete = useCallback(() => setDeletingId(null), []);

  const submitDelete = useCallback(async () => {
    if (deletingId === null) return;
    const update: RegisteredUserUpdate = { user_id: deletingId, name: null };
    await invoke("update_user_list", { users: [update] });
    setDeletingId(null);
    setLoading(true);
    invoke("request_user_list").catch(() => setLoading(false));
  }, [deletingId]);

  // Filter + sort users.
  const filtered = users
    .filter((u) => {
      if (!search.trim()) return true;
      const q = search.toLowerCase();
      return u.name.toLowerCase().includes(q);
    })
    .sort((a, b) => {
      const dir = sortDir === "asc" ? 1 : -1;
      switch (sortKey) {
        case "name":
          return dir * a.name.localeCompare(b.name);
        case "last_seen":
          return dir * (a.last_seen ?? "").localeCompare(b.last_seen ?? "");
        case "last_channel":
          return dir * ((a.last_channel ?? 0) - (b.last_channel ?? 0));
        default:
          return 0;
      }
    });

  const sortArrow = (key: SortKey) => {
    if (sortKey !== key) return null;
    return sortDir === "asc" ? " \u25B2" : " \u25BC";
  };

  return (
    <>
      <h2 className={styles.panelTitle}>Registered Users</h2>

      <div className={styles.toolbar}>
        <div className={styles.searchWrap}>
          <svg className={styles.searchIcon} width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <circle cx="11" cy="11" r="8" />
            <line x1="21" y1="21" x2="16.65" y2="16.65" />
          </svg>
          <input
            ref={searchRef}
            className={styles.searchInput}
            type="text"
            placeholder="Search users..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
          {search && (
            <button
              type="button"
              className={styles.clearBtn}
              onClick={() => { setSearch(""); searchRef.current?.focus(); }}
              aria-label="Clear search"
            >
              &times;
            </button>
          )}
        </div>
        <button type="button" className={styles.refreshBtn} onClick={handleRefresh} disabled={loading}>
          {loading ? "Loading..." : "Refresh"}
        </button>
      </div>

      <div className={styles.tableWrap}>
        <table className={styles.table}>
          <thead>
            <tr>
              <th className={styles.sortable} onClick={() => toggleSort("name")}>
                Username{sortArrow("name")}
              </th>
              <th className={styles.sortable} onClick={() => toggleSort("last_seen")}>
                Last Seen{sortArrow("last_seen")}
              </th>
              <th className={styles.sortable} onClick={() => toggleSort("last_channel")}>
                Last Channel{sortArrow("last_channel")}
              </th>
              <th>Actions</th>
            </tr>
          </thead>
          <tbody>
            {filtered.length === 0 ? (
              <tr>
                <td colSpan={4} className={styles.emptyRow}>
                  {loading ? "Loading..." : users.length === 0 ? "No registered users" : "No matching users"}
                </td>
              </tr>
            ) : (
              filtered.map((u) => (
                <tr key={u.user_id}>
                  <td>
                    {editingId === u.user_id ? (
                      <span className={styles.inlineEdit}>
                        <input
                          ref={editRef}
                          className={styles.inputSmall}
                          type="text"
                          value={editName}
                          onChange={(e) => setEditName(e.target.value)}
                          onKeyDown={(e) => {
                            if (e.key === "Enter") submitRename();
                            if (e.key === "Escape") cancelRename();
                          }}
                        />
                        <button type="button" className={styles.saveBtn} onClick={submitRename}>Save</button>
                        <button type="button" className={styles.removeBtn} onClick={cancelRename}>Cancel</button>
                      </span>
                    ) : (
                      u.name
                    )}
                  </td>
                  <td className={styles.dimText} title={u.last_seen ?? undefined}>
                    {u.last_seen ? formatRelativeDate(u.last_seen) : "Never"}
                  </td>
                  <td className={styles.dimText}>{u.last_channel ?? "Unknown"}</td>
                  <td>
                    {deletingId === u.user_id ? (
                      <span className={styles.inlineEdit}>
                        <span className={styles.confirmText}>Delete?</span>
                        <button type="button" className={styles.removeBtn} onClick={submitDelete}>Yes</button>
                        <button type="button" className={styles.refreshBtn} onClick={cancelDelete}>No</button>
                      </span>
                    ) : (
                      <span className={styles.actionBtns}>
                        <button
                          type="button"
                          className={styles.refreshBtn}
                          onClick={() => startRename(u)}
                          disabled={editingId !== null && editingId !== u.user_id}
                        >
                          Rename
                        </button>
                        <button
                          type="button"
                          className={styles.removeBtn}
                          onClick={() => confirmDelete(u.user_id)}
                        >
                          Delete
                        </button>
                      </span>
                    )}
                  </td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>

      <div className={styles.statusBar}>
        {filtered.length} of {users.length} user{users.length === 1 ? "" : "s"}
      </div>
    </>
  );
}
