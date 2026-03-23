import { useState, useCallback } from "react";
import type { AclGroup } from "../../types";
import styles from "./AdminPanel.module.css";

interface UserLike {
  session: number;
  name: string;
  user_id?: number | null;
}

export function GroupsPanel({
  groups,
  users,
  registeredNames,
  onAdd,
  onRemove,
  onPatch,
}: Readonly<{
  groups: AclGroup[];
  users: UserLike[];
  registeredNames: Map<number, string>;
  onAdd: () => void;
  onRemove: (idx: number) => void;
  onPatch: (idx: number, patch: Partial<AclGroup>) => void;
}>) {
  return (
    <>
      <div className={styles.aclSectionHeader}>
        <span className={styles.aclSectionTitle}>Groups</span>
        <button type="button" className={styles.addBtn} onClick={onAdd}>
          + Add Group
        </button>
      </div>
      {groups.length === 0 ? (
        <div className={styles.dimText}>No groups defined</div>
      ) : (
        groups.map((g, i) => (
          <GroupCard
            key={`group-${i}`}
            group={g}
            index={i}
            users={users}
            registeredNames={registeredNames}
            onPatch={onPatch}
            onRemove={onRemove}
          />
        ))
      )}
    </>
  );
}

function GroupCard({
  group,
  index,
  users,
  registeredNames,
  onPatch,
  onRemove,
}: Readonly<{
  group: AclGroup;
  index: number;
  users: UserLike[];
  registeredNames: Map<number, string>;
  onPatch: (idx: number, patch: Partial<AclGroup>) => void;
  onRemove: (idx: number) => void;
}>) {
  const [addInput, setAddInput] = useState("");
  const [removeInput, setRemoveInput] = useState("");

  const resolveUserId = useCallback(
    (input: string): number | null => {
      const trimmed = input.trim();
      if (!trimmed) return null;
      const asNum = Number(trimmed);
      if (Number.isFinite(asNum) && asNum >= 0) return asNum;
      const match = users.find(
        (u) => u.name.toLowerCase() === trimmed.toLowerCase() && u.user_id != null,
      );
      return match?.user_id ?? null;
    },
    [users],
  );

  const handleAddMember = useCallback(() => {
    const uid = resolveUserId(addInput);
    if (uid === null) return;
    if (group.add.includes(uid)) return;
    onPatch(index, { add: [...group.add, uid] });
    setAddInput("");
  }, [addInput, resolveUserId, group.add, onPatch, index]);

  const handleRemoveMember = useCallback(() => {
    const uid = resolveUserId(removeInput);
    if (uid === null) return;
    if (group.remove.includes(uid)) return;
    onPatch(index, { remove: [...group.remove, uid] });
    setRemoveInput("");
  }, [removeInput, resolveUserId, group.remove, onPatch, index]);

  const dropFromAdd = useCallback(
    (uid: number) => {
      onPatch(index, { add: group.add.filter((id) => id !== uid) });
    },
    [group.add, onPatch, index],
  );

  const dropFromRemove = useCallback(
    (uid: number) => {
      onPatch(index, { remove: group.remove.filter((id) => id !== uid) });
    },
    [group.remove, onPatch, index],
  );

  const userNameById = useCallback(
    (uid: number): string => {
      const online = users.find((usr) => usr.user_id === uid);
      if (online) return online.name;
      const registered = registeredNames.get(uid);
      if (registered) return registered;
      return `User #${uid}`;
    },
    [users, registeredNames],
  );

  return (
    <div className={styles.aclCard}>
      <div className={styles.aclCardHeaderStatic}>
        <input
          className={styles.inputSmall}
          type="text"
          value={group.name}
          disabled={group.inherited}
          onChange={(e) => onPatch(index, { name: e.target.value })}
        />
        {group.inherited && <span className={styles.inheritBadge}>Inherited</span>}
        {!group.inherited && (
          <button type="button" className={styles.removeSmallBtn} onClick={() => onRemove(index)}>
            &times;
          </button>
        )}
      </div>

      <div className={styles.aclCardBody}>
        <div className={styles.aclRuleOptions}>
          <label className={styles.checkboxLabel}>
            <input type="checkbox" checked={group.inherit} disabled={group.inherited} onChange={(e) => onPatch(index, { inherit: e.target.checked })} />
            Inherit
          </label>
          <label className={styles.checkboxLabel}>
            <input type="checkbox" checked={group.inheritable} disabled={group.inherited} onChange={(e) => onPatch(index, { inheritable: e.target.checked })} />
            Inheritable
          </label>
        </div>

        {group.inherited_members.length > 0 && (
          <div className={styles.memberSection}>
            <span className={styles.memberSectionTitle}>Inherited members</span>
            <div className={styles.memberChips}>
              {group.inherited_members.map((uid) => (
                <span key={uid} className={styles.memberChip}>
                  {userNameById(uid)}
                </span>
              ))}
            </div>
          </div>
        )}

        <div className={styles.memberSection}>
          <span className={styles.memberSectionTitle}>Members to add</span>
          <div className={styles.memberChips}>
            {group.add.map((uid) => (
              <span key={uid} className={styles.memberChipRemovable}>
                {userNameById(uid)}
                {!group.inherited && (
                  <button type="button" className={styles.chipRemoveBtn} onClick={() => dropFromAdd(uid)}>
                    &times;
                  </button>
                )}
              </span>
            ))}
          </div>
          {!group.inherited && (
            <div className={styles.memberAddRow}>
              <input
                className={styles.inputSmall}
                type="text"
                placeholder="User ID or name"
                value={addInput}
                onChange={(e) => setAddInput(e.target.value)}
                onKeyDown={(e) => { if (e.key === "Enter") handleAddMember(); }}
              />
              <button type="button" className={styles.addBtn} onClick={handleAddMember}>
                Add
              </button>
            </div>
          )}
        </div>

        <div className={styles.memberSection}>
          <span className={styles.memberSectionTitle}>Members to remove</span>
          <div className={styles.memberChips}>
            {group.remove.map((uid) => (
              <span key={uid} className={styles.memberChipRemovable}>
                {userNameById(uid)}
                {!group.inherited && (
                  <button type="button" className={styles.chipRemoveBtn} onClick={() => dropFromRemove(uid)}>
                    &times;
                  </button>
                )}
              </span>
            ))}
          </div>
          {!group.inherited && (
            <div className={styles.memberAddRow}>
              <input
                className={styles.inputSmall}
                type="text"
                placeholder="User ID or name"
                value={removeInput}
                onChange={(e) => setRemoveInput(e.target.value)}
                onKeyDown={(e) => { if (e.key === "Enter") handleRemoveMember(); }}
              />
              <button type="button" className={styles.addBtn} onClick={handleRemoveMember}>
                Exclude
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
