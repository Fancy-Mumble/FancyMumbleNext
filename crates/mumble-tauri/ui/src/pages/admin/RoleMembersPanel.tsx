import { useMemo } from "react";
import type { AclGroup } from "../../types";
import { MemberPicker } from "../../components/elements/MemberPicker";
import { useAppStore } from "../../store";
import { getCachedUserAvatar } from "../../lazyBlobs";
import styles from "./AdminPanel.module.css";

export interface RoleMembersPanelProps {
  readonly role: AclGroup;
  readonly onPatch: (patch: Partial<AclGroup>) => void;
  readonly registeredUsers: readonly { user_id: number; name: string }[];
  readonly disabled?: boolean;
}

/** Members sub-tab: edit `add` and `remove` lists with autocomplete pickers. */
export function RoleMembersPanel({ role, onPatch, registeredUsers, disabled }: RoleMembersPanelProps) {
  const onlineUsers = useAppStore((s) => s.users);

  const candidates = useMemo(
    () => registeredUsers.map((u) => ({ user_id: u.user_id, name: u.name })),
    [registeredUsers],
  );
  const resolveName = (id: number) =>
    registeredUsers.find((u) => u.user_id === id)?.name ?? `User #${id}`;

  /** Look up the avatar data URL for a registered user via the live online users list. */
  const getAvatar = (id: number): string | null => {
    const live = onlineUsers.find((u) => u.user_id === id);
    if (!live) return null;
    return getCachedUserAvatar(live.session, live.texture_size);
  };

  const inheritedNames = role.inherited_members.map(resolveName);

  return (
    <div className={styles.editorMain}>
      <fieldset className={styles.fieldset}>
        <legend>Members</legend>
        <MemberPicker
          value={role.add}
          candidates={candidates}
          resolveName={resolveName}
          getAvatar={getAvatar}
          onChange={(add) => onPatch({ add })}
          disabled={disabled || role.inherited}
          emptyLabel="No explicit members"
        />
      </fieldset>

      <fieldset className={styles.fieldset}>
        <legend>Excluded members</legend>
        <p className={styles.dimText}>
          Users who should be removed from the inherited member set.
        </p>
        <MemberPicker
          value={role.remove}
          candidates={candidates}
          resolveName={resolveName}
          getAvatar={getAvatar}
          onChange={(remove) => onPatch({ remove })}
          disabled={disabled || role.inherited}
          emptyLabel="No exclusions"
        />
      </fieldset>

      {role.inherited_members.length > 0 && (
        <fieldset className={styles.fieldset}>
          <legend>Inherited members ({role.inherited_members.length})</legend>
          <div className={styles.inheritedChips}>
            {inheritedNames.map((name, i) => (
              <span key={`${role.inherited_members[i]}-${name}`} className={styles.inheritBadge}>
                {name}
              </span>
            ))}
          </div>
        </fieldset>
      )}

      <fieldset className={styles.fieldset}>
        <legend>Inheritance</legend>
        <label className={styles.checkboxLabel}>
          <input
            type="checkbox"
            checked={role.inherit}
            onChange={(e) => onPatch({ inherit: e.target.checked })}
            disabled={disabled || role.inherited}
          />
          Inherit members from parent channels
        </label>
        <label className={styles.checkboxLabel}>
          <input
            type="checkbox"
            checked={role.inheritable}
            onChange={(e) => onPatch({ inheritable: e.target.checked })}
            disabled={disabled || role.inherited}
          />
          Allow child channels to inherit from this role
        </label>
      </fieldset>
    </div>
  );
}
