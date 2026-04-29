import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AclData, AclGroup, RegisteredUser } from "../../types";
import { RoleChip } from "../../components/elements/RoleChip";
import styles from "./AdminPanel.module.css";

export interface UserRoleManagerDialogProps {
  readonly user: RegisteredUser;
  /** ACL for the root channel. May be `null` while still loading. */
  readonly acl: AclData | null;
  readonly onClose: () => void;
  readonly onSaved: () => void;
}

interface RoleRow {
  readonly name: string;
  readonly group: AclGroup;
  /** Currently a member via direct add or inheritance. */
  readonly isMember: boolean;
  /** Membership only via inheritance from a parent channel. */
  readonly isInheritedOnly: boolean;
}

function buildRoleRows(groups: readonly AclGroup[], userId: number): RoleRow[] {
  return groups
    .filter((g) => !g.name.startsWith("~"))
    .map((g) => {
      const directlyAdded = g.add.includes(userId);
      const inherited = g.inherited_members.includes(userId);
      return {
        name: g.name,
        group: g,
        isMember: directlyAdded || inherited,
        isInheritedOnly: inherited && !directlyAdded,
      };
    })
    .sort((a, b) => a.name.localeCompare(b.name));
}

/**
 * Modal that lets an administrator toggle which channel groups a registered
 * user belongs to. Saves by pushing an updated `AclData` for the root channel
 * back to the server via `update_acl`.
 */
export function UserRoleManagerDialog({ user, acl, onClose, onSaved }: UserRoleManagerDialogProps) {
  const groups = acl?.groups ?? [];
  const initialMembership = useMemo(() => {
    const set = new Set<string>();
    for (const g of groups) {
      if (g.add.includes(user.user_id)) set.add(g.name);
    }
    return set;
  }, [groups, user.user_id]);

  const [membership, setMembership] = useState<Set<string>>(initialMembership);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Re-seed membership when ACL data finally arrives.
  useEffect(() => {
    setMembership(initialMembership);
  }, [initialMembership]);

  const rows = useMemo(() => buildRoleRows(groups, user.user_id), [groups, user.user_id]);
  const dirty = useMemo(() => {
    if (membership.size !== initialMembership.size) return true;
    for (const name of membership) if (!initialMembership.has(name)) return true;
    return false;
  }, [membership, initialMembership]);

  const toggle = (name: string) => {
    setMembership((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  };

  const handleSave = async () => {
    if (!dirty || saving || !acl) return;
    setSaving(true);
    setError(null);
    const patchedGroups: AclGroup[] = acl.groups.map((g) => {
      const shouldBeMember = membership.has(g.name);
      const isCurrent = g.add.includes(user.user_id);
      if (shouldBeMember === isCurrent) return g;
      const add = shouldBeMember
        ? [...g.add, user.user_id]
        : g.add.filter((id) => id !== user.user_id);
      const remove = shouldBeMember
        ? g.remove.filter((id) => id !== user.user_id)
        : g.remove;
      return { ...g, add, remove };
    });
    try {
      await invoke("update_acl", { acl: { ...acl, groups: patchedGroups } });
      onSaved();
      onClose();
    } catch (e) {
      console.error("Failed to update roles", e);
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  // Open the native <dialog> as modal on mount, close on unmount.
  // Native <dialog> handles Escape -> "close" event automatically.
  const dialogRef = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const node = dialogRef.current;
    if (!node) return;
    if (!node.open) node.showModal();
    const handleNativeClose = () => onClose();
    node.addEventListener("close", handleNativeClose);
    return () => {
      node.removeEventListener("close", handleNativeClose);
      if (node.open) node.close();
    };
  }, [onClose]);

  return (
    <dialog
      ref={dialogRef}
      className={styles.dialogCard}
      aria-label={`Manage roles for ${user.name}`}
    >
      <div>
        <div className={styles.dialogHeader}>
          <h3 className={styles.dialogTitle}>Manage roles for {user.name}</h3>
          <button type="button" className={styles.dialogClose} onClick={onClose} aria-label="Close">
            &times;
          </button>
        </div>

        <div className={styles.dialogBody}>
          {acl === null ? (
            <p className={styles.dimText}>Loading roles...</p>
          ) : rows.length === 0 ? (
            <p className={styles.dimText}>No roles defined on this server.</p>
          ) : (
            <ul className={styles.roleCheckList}>
              {rows.map((row) => {
                const checked = membership.has(row.name);
                return (
                  <li key={row.name} className={styles.roleCheckRow}>
                    <label className={styles.roleCheckLabel}>
                      <input
                        type="checkbox"
                        checked={checked}
                        disabled={row.isInheritedOnly && !checked}
                        onChange={() => toggle(row.name)}
                      />
                      <RoleChip
                        name={row.group.name}
                        color={row.group.color}
                        icon={row.group.icon}
                        size="small"
                      />
                      {row.isInheritedOnly && (
                        <span className={styles.dimText}>(inherited)</span>
                      )}
                    </label>
                  </li>
                );
              })}
            </ul>
          )}
          {error && <p className={styles.errorText}>{error}</p>}
        </div>

        <div className={styles.dialogActions}>
          <button type="button" className={styles.refreshBtn} onClick={onClose} disabled={saving}>
            Cancel
          </button>
          <button
            type="button"
            className={styles.saveBtn}
            onClick={handleSave}
            disabled={!dirty || saving}
          >
            {saving ? "Saving..." : "Save"}
          </button>
        </div>
      </div>
    </dialog>
  );
}
