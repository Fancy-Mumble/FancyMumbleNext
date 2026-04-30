import { ChevronRightIcon } from "../../icons";
import { useState, useEffect, useCallback, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useAppStore } from "../../store";
import type { AclData, AclEntry, AclGroup, ChannelEntry, RegisteredUser } from "../../types";
import { AclRulesPanel } from "./AclRulesPanel";
import { GroupsPanel } from "./GroupsPanel";
import styles from "./AdminPanel.module.css";

type AclTab = "groups" | "rules";

// -- Tree helpers -------------------------------------------------

interface TreeNode {
  channel: ChannelEntry;
  children: TreeNode[];
}

function buildChannelTree(channels: ChannelEntry[]): TreeNode[] {
  const root = channels.find(
    (c) => c.parent_id === null || c.parent_id === c.id,
  );
  if (!root) return [];

  const byParent = new Map<number, ChannelEntry[]>();
  for (const ch of channels) {
    if (ch.id === root.id) continue;
    const pid = ch.parent_id ?? root.id;
    const list = byParent.get(pid);
    if (list) list.push(ch);
    else byParent.set(pid, [ch]);
  }

  function build(ch: ChannelEntry): TreeNode {
    const kids = (byParent.get(ch.id) ?? [])
      .sort((a, b) => a.position - b.position || a.name.localeCompare(b.name));
    return { channel: ch, children: kids.map(build) };
  }
  return [build(root)];
}

/** Returns a set of channel IDs whose subtree contains a match. */
function filterTree(nodes: TreeNode[], query: string): Set<number> {
  const matched = new Set<number>();
  const lq = query.toLowerCase();
  function walk(node: TreeNode): boolean {
    const selfMatch = node.channel.name.toLowerCase().includes(lq);
    let childMatch = false;
    for (const child of node.children) {
      if (walk(child)) childMatch = true;
    }
    if (selfMatch || childMatch) {
      matched.add(node.channel.id);
      return true;
    }
    return false;
  }
  for (const n of nodes) walk(n);
  return matched;
}

// -- Main component -----------------------------------------------

export function ChannelAclTab() {
  const channels = useAppStore((s) => s.channels);
  const users = useAppStore((s) => s.users);
  const [selectedChannel, setSelectedChannel] = useState<number | null>(null);
  const [aclData, setAclData] = useState<AclData | null>(null);
  const [loading, setLoading] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [search, setSearch] = useState("");
  const [activeTab, setActiveTab] = useState<AclTab>("rules");
  const [expanded, setExpanded] = useState<Set<number>>(() => new Set());
  const [registeredNames, setRegisteredNames] = useState<Map<number, string>>(new Map());

  const tree = useMemo(() => buildChannelTree(channels), [channels]);
  const matchedIds = useMemo(
    () => (search ? filterTree(tree, search) : null),
    [tree, search],
  );

  // Auto-expand root on first render.
  useEffect(() => {
    if (tree.length > 0 && expanded.size === 0) {
      setExpanded(new Set([tree[0].channel.id]));
    }
  }, [tree, expanded.size]);

  // Fetch registered user names for ID resolution.
  useEffect(() => {
    const unlisten = listen<RegisteredUser[]>("user-list", (event) => {
      const map = new Map<number, string>();
      for (const u of event.payload) {
        map.set(u.user_id, u.name);
      }
      setRegisteredNames(map);
    });
    invoke("request_user_list").catch(() => {});
    return () => { unlisten.then((f) => f()); };
  }, []);

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

  const toggleExpand = useCallback((id: number) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  // -- ACL data mutations --

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
      setAclData({ ...aclData, groups: aclData.groups.filter((_, i) => i !== idx) });
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
      setAclData({ ...aclData, acls: aclData.acls.filter((_, i) => i !== idx) });
      setDirty(true);
    },
    [aclData],
  );

  const togglePermBit = useCallback(
    (aclIdx: number, field: "grant" | "deny", bit: number) => {
      if (!aclData) return;
      const entry = aclData.acls[aclIdx];
      patchAcl(aclIdx, { [field]: entry[field] ^ bit });
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

  const selectedName = channels.find((c) => c.id === selectedChannel)?.name ?? "";

  return (
    <>
      <h2 className={styles.panelTitle}>Channel ACL Editor</h2>

      <div className={styles.aclSplitView}>
        {/* Left: Channel tree */}
        <div className={styles.aclTreePane}>
          <div className={styles.aclTreeSearch}>
            <input
              className={styles.searchInput}
              type="text"
              placeholder="Search channels..."
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
            {search && (
              <button
                type="button"
                className={styles.clearBtn}
                onClick={() => setSearch("")}
              >
                &times;
              </button>
            )}
          </div>
          <div className={styles.aclTreeList}>
            {tree.map((node) => (
              <ChannelTreeNode
                key={node.channel.id}
                node={node}
                depth={0}
                selected={selectedChannel}
                expanded={expanded}
                matchedIds={matchedIds}
                onSelect={handleChannelSelect}
                onToggle={toggleExpand}
              />
            ))}
          </div>
        </div>

        {/* Right: ACL detail */}
        <div className={styles.aclDetailPane}>
          {selectedChannel === null && !loading && (
            <div className={styles.detailEmpty}>Select a channel to edit its ACL</div>
          )}
          {loading && <div className={styles.detailEmpty}>Loading ACL...</div>}

          {aclData && !loading && (
            <>
              <div className={styles.aclDetailHeader}>
                <h3 className={styles.aclDetailTitle}>{selectedName}</h3>
                {dirty && (
                  <button type="button" className={styles.saveBtn} onClick={handleSave}>
                    Save Changes
                  </button>
                )}
              </div>

              <label className={styles.checkboxLabel}>
                <input
                  type="checkbox"
                  checked={aclData.inherit_acls}
                  onChange={handleToggleInherit}
                />
                Inherit ACLs from parent channel
              </label>

              {/* Tab switcher */}
              <div className={styles.aclTabs}>
                <button
                  type="button"
                  className={`${styles.aclTabBtn} ${activeTab === "rules" ? styles.aclTabActive : ""}`}
                  onClick={() => setActiveTab("rules")}
                >
                  ACL Rules ({aclData.acls.length})
                </button>
                <button
                  type="button"
                  className={`${styles.aclTabBtn} ${activeTab === "groups" ? styles.aclTabActive : ""}`}
                  onClick={() => setActiveTab("groups")}
                >
                  Groups ({aclData.groups.length})
                </button>
              </div>

              {/* Tab content */}
              <div className={styles.aclTabContent}>
                {activeTab === "rules" && (
                  <AclRulesPanel
                    acls={aclData.acls}
                    onAdd={addAcl}
                    onRemove={removeAcl}
                    onPatch={patchAcl}
                    onToggleBit={togglePermBit}
                  />
                )}
                {activeTab === "groups" && (
                  <GroupsPanel
                    groups={aclData.groups}
                    users={users}
                    registeredNames={registeredNames}
                    onAdd={addGroup}
                    onRemove={removeGroup}
                    onPatch={patchGroup}
                  />
                )}
              </div>
            </>
          )}
        </div>
      </div>
    </>
  );
}

// -- Channel tree node --------------------------------------------

function ChannelTreeNode({
  node,
  depth,
  selected,
  expanded,
  matchedIds,
  onSelect,
  onToggle,
}: Readonly<{
  node: TreeNode;
  depth: number;
  selected: number | null;
  expanded: Set<number>;
  matchedIds: Set<number> | null;
  onSelect: (id: number) => void;
  onToggle: (id: number) => void;
}>) {
  const id = node.channel.id;
  const isExpanded = expanded.has(id);
  const hasChildren = node.children.length > 0;
  const isSelected = selected === id;

  // If filtering and this node isn't in matched set, hide it.
  if (matchedIds && !matchedIds.has(id)) return null;

  return (
    <>
      <button
        type="button"
        className={`${styles.aclTreeItem} ${isSelected ? styles.aclTreeItemActive : ""}`}
        style={{ paddingLeft: 8 + depth * 16 }}
        onClick={() => onSelect(id)}
      >
        {hasChildren && (
          <span
            className={styles.aclTreeChevron}
            role="button"
            tabIndex={-1}
            onClick={(e) => { e.stopPropagation(); onToggle(id); }}
            onKeyDown={(e) => { if (e.key === "Enter") { e.stopPropagation(); onToggle(id); } }}
          >
            <ChevronRightIcon
              width={12}
              height={12}
              style={{ transform: isExpanded ? "rotate(90deg)" : "rotate(0deg)", transition: "transform 0.15s" }}
            />
          </span>
        )}
        {!hasChildren && <span className={styles.aclTreeChevronSpacer} />}
        <span className={styles.aclTreeLabel}>{node.channel.name}</span>
      </button>
      {isExpanded &&
        node.children.map((child) => (
          <ChannelTreeNode
            key={child.channel.id}
            node={child}
            depth={depth + 1}
            selected={selected}
            expanded={expanded}
            matchedIds={matchedIds}
            onSelect={onSelect}
            onToggle={onToggle}
          />
        ))}
    </>
  );
}
