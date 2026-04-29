import { ChevronRightIcon } from "../../icons";
import { useState } from "react";
import type { AclEntry } from "../../types";
import styles from "./AdminPanel.module.css";
import { PERMISSIONS } from "../../utils/permissions";

export function AclRulesPanel({
  acls,
  onAdd,
  onRemove,
  onPatch,
  onToggleBit,
}: Readonly<{
  acls: AclEntry[];
  onAdd: () => void;
  onRemove: (idx: number) => void;
  onPatch: (idx: number, patch: Partial<AclEntry>) => void;
  onToggleBit: (idx: number, field: "grant" | "deny", bit: number) => void;
}>) {
  const [expandedIdx, setExpandedIdx] = useState<number | null>(null);

  return (
    <>
      <div className={styles.aclSectionHeader}>
        <span className={styles.aclSectionTitle}>ACL Rules</span>
        <button type="button" className={styles.addBtn} onClick={onAdd}>
          + Add Rule
        </button>
      </div>
      {acls.length === 0 ? (
        <div className={styles.dimText}>No ACL rules defined</div>
      ) : (
        acls.map((entry, i) => (
          <AclRuleCard
            key={`acl-${i}`}
            entry={entry}
            index={i}
            isOpen={expandedIdx === i}
            onToggleOpen={() => setExpandedIdx(expandedIdx === i ? null : i)}
            onPatch={onPatch}
            onRemove={onRemove}
            onToggleBit={onToggleBit}
          />
        ))
      )}
    </>
  );
}

function AclRuleCard({
  entry,
  index,
  isOpen,
  onToggleOpen,
  onPatch,
  onRemove,
  onToggleBit,
}: Readonly<{
  entry: AclEntry;
  index: number;
  isOpen: boolean;
  onToggleOpen: () => void;
  onPatch: (idx: number, patch: Partial<AclEntry>) => void;
  onRemove: (idx: number) => void;
  onToggleBit: (idx: number, field: "grant" | "deny", bit: number) => void;
}>) {
  const label = entry.group
    ? `@${entry.group}`
    : entry.user_id != null
      ? `User #${entry.user_id}`
      : "Unknown";

  return (
    <div className={styles.aclCard}>
      <button type="button" className={styles.aclCardHeader} onClick={onToggleOpen}>
        <ChevronRightIcon
          width={12}
          height={12}
          className={styles.aclCardChevron}
          style={{ transform: isOpen ? "rotate(90deg)" : "rotate(0deg)" }}
        />
        <span className={styles.aclRuleLabel}>{label}</span>
        {entry.inherited && <span className={styles.inheritBadge}>Inherited</span>}
        {!entry.inherited && (
          <span
            className={styles.removeSmallBtn}
            role="button"
            tabIndex={0}
            onClick={(e) => { e.stopPropagation(); onRemove(index); }}
            onKeyDown={(e) => { if (e.key === "Enter") { e.stopPropagation(); onRemove(index); } }}
          >
            &times;
          </span>
        )}
      </button>

      {isOpen && (
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

          <div className={styles.permGrid}>
            <div className={styles.permHeader}>
              <span>Permission</span>
              <span>Allow</span>
              <span>Deny</span>
            </div>
            {PERMISSIONS.map(({ bit, label: permLabel }) => (
              <div key={bit} className={styles.permRow}>
                <span className={styles.permLabel}>{permLabel}</span>
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
      )}
    </div>
  );
}
