import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AclGroup, ChannelEntry, RegisteredUser, UserCommentPayload, UserEntry } from "../../types";
import { useAclGroups } from "../../hooks/useAclGroups";
import { UserListItem } from "./UserListItem";
import styles from "./ChannelSidebar.module.css";

interface MembersTabProps {
  readonly users: readonly UserEntry[];
  readonly channels: readonly ChannelEntry[];
  readonly ownSession: number | null;
  readonly selectedDmUser: number | null;
  readonly talkingSessions: ReadonlySet<number>;
  readonly onSelectDm: (session: number) => void;
  readonly onUserContextMenu: (e: React.MouseEvent, user: UserEntry) => void;
}

interface MemberRow {
  readonly entry: UserEntry;
  readonly offline: boolean;
}

interface MemberGroup {
  readonly key: string;
  readonly label: string;
  readonly color: string | null;
  readonly rows: readonly MemberRow[];
}

/** Sentinel keys for the catch-all buckets at the end of the list. */
const KEY_NO_GROUP = "__no_group__";
const KEY_GUESTS = "__guests__";

/**
 * Build a synthetic `UserEntry` for an offline registered user so the
 * shared `UserListItem` component can render them without special-casing.
 *
 * The session id is set to a negative number derived from the user_id
 * to keep it unique and to ensure no DM/talking lookups ever match.
 * Avatar bytes come from the server's `UserList` response when available.
 */
function synthesiseOfflineEntry(
  reg: RegisteredUser,
  fetchedComments: ReadonlyMap<number, string>,
): UserEntry {
  const comment = fetchedComments.get(reg.user_id) ?? reg.comment ?? null;
  return {
    session: -(reg.user_id + 1),
    name: reg.name,
    channel_id: reg.last_channel ?? 0,
    user_id: reg.user_id,
    texture: reg.texture && reg.texture.length > 0 ? reg.texture : null,
    comment,
    mute: false,
    deaf: false,
    suppress: false,
    self_mute: false,
    self_deaf: false,
    priority_speaker: false,
    hash: undefined,
  };
}

/**
 * Build a `user_id -> first-non-system-group-name` mapping in ACL order.
 * Returns the mapping plus the ordered list of distinct group names that
 * actually have at least one assigned member.
 */
function buildUserGroupMap(aclGroups: readonly AclGroup[]): {
  readonly userIdToGroup: ReadonlyMap<number, string>;
  readonly groupOrder: readonly string[];
  readonly groupColors: ReadonlyMap<string, string>;
} {
  const userIdToGroup = new Map<number, string>();
  const groupOrder: string[] = [];
  const groupColors = new Map<string, string>();
  for (const g of aclGroups) {
    if (g.name.startsWith("~")) continue;
    if (g.color && !groupColors.has(g.name)) {
      groupColors.set(g.name, g.color);
    }
    const removeSet = new Set(g.remove);
    let assignedAny = false;
    for (const uid of [...g.add, ...g.inherited_members]) {
      if (removeSet.has(uid)) continue;
      if (!userIdToGroup.has(uid)) {
        userIdToGroup.set(uid, g.name);
        assignedAny = true;
      }
    }
    if (assignedAny && !groupOrder.includes(g.name)) {
      groupOrder.push(g.name);
    }
  }
  return { userIdToGroup, groupOrder, groupColors };
}

/** Order rows online-first, then alphabetical within each tier. */
function compareRows(a: MemberRow, b: MemberRow): number {
  if (a.offline !== b.offline) return a.offline ? 1 : -1;
  return a.entry.name.localeCompare(b.entry.name);
}

/**
 * Bucket member rows into groups according to `userIdToGroup`.  Rows whose
 * user has no group go into `KEY_NO_GROUP`; unregistered (anonymous)
 * online users go into `KEY_GUESTS`.
 */
function bucketRows(
  rows: readonly MemberRow[],
  userIdToGroup: ReadonlyMap<number, string>,
): Map<string, MemberRow[]> {
  const buckets = new Map<string, MemberRow[]>();
  const push = (key: string, row: MemberRow) => {
    const list = buckets.get(key);
    if (list) list.push(row);
    else buckets.set(key, [row]);
  };
  for (const row of rows) {
    const uid = row.entry.user_id;
    if (uid == null || uid <= 0) {
      push(KEY_GUESTS, row);
      continue;
    }
    const groupName = userIdToGroup.get(uid);
    push(groupName ?? KEY_NO_GROUP, row);
  }
  return buckets;
}

/**
 * Combine online + offline registered users, group them by ACL role
 * and produce the final ordered list of `MemberGroup` sections.
 */
export function buildMemberGroups(
  users: readonly UserEntry[],
  registered: readonly RegisteredUser[],
  ownSession: number | null,
  aclGroups: readonly AclGroup[],
  fetchedComments: ReadonlyMap<number, string> = new Map(),
): readonly MemberGroup[] {
  const onlineUserIds = new Set<number>();
  const onlineRows: MemberRow[] = [];
  for (const u of users) {
    if (u.session === ownSession) continue;
    if (u.user_id != null && u.user_id > 0) onlineUserIds.add(u.user_id);
    onlineRows.push({ entry: u, offline: false });
  }
  const offlineRows: MemberRow[] = registered
    .filter((r) => !onlineUserIds.has(r.user_id))
    .map((r) => ({ entry: synthesiseOfflineEntry(r, fetchedComments), offline: true }));

  const { userIdToGroup, groupOrder, groupColors } = buildUserGroupMap(aclGroups);
  const buckets = bucketRows([...onlineRows, ...offlineRows], userIdToGroup);

  const result: MemberGroup[] = [];
  for (const name of groupOrder) {
    const rows = buckets.get(name);
    if (!rows || rows.length === 0) continue;
    rows.sort(compareRows);
    result.push({
      key: name,
      label: name,
      color: groupColors.get(name) ?? null,
      rows,
    });
  }
  const noGroupRows = buckets.get(KEY_NO_GROUP);
  if (noGroupRows && noGroupRows.length > 0) {
    noGroupRows.sort(compareRows);
    result.push({ key: KEY_NO_GROUP, label: "Members", color: null, rows: noGroupRows });
  }
  const guestRows = buckets.get(KEY_GUESTS);
  if (guestRows && guestRows.length > 0) {
    guestRows.sort(compareRows);
    result.push({ key: KEY_GUESTS, label: "Guests", color: null, rows: guestRows });
  }
  return result;
}

/**
 * Members tab for the sidebar.  Lists every user (online + offline
 * registered) grouped by their primary ACL role.  The whole tab scrolls
 * as a single non-nested list so groups flow consecutively.
 */
export function MembersTab({
  users,
  channels,
  ownSession,
  selectedDmUser,
  talkingSessions,
  onSelectDm,
  onUserContextMenu,
}: MembersTabProps) {
  const [registered, setRegistered] = useState<readonly RegisteredUser[]>([]);
  const [fetchedComments, setFetchedComments] = useState<ReadonlyMap<number, string>>(new Map());
  /** Tracks user_ids for which a blob request has already been sent
   * to avoid redundant requests if the hover card is opened repeatedly. */
  const requestedRef = useRef<Set<number>>(new Set());
  const aclGroups = useAclGroups();

  useEffect(() => {
    const unlistenList = listen<RegisteredUser[]>("user-list", (event) => {
      setRegistered(event.payload);
    });
    const unlistenComment = listen<UserCommentPayload>("user-comment", (event) => {
      const { user_id, comment } = event.payload;
      setFetchedComments((prev) => {
        const next = new Map(prev);
        next.set(user_id, comment);
        return next;
      });
    });
    invoke("request_user_list").catch(() => {});
    return () => {
      unlistenList.then((f) => f());
      unlistenComment.then((f) => f());
    };
  }, []);

  const handleRequestComment = (userId: number) => {
    if (requestedRef.current.has(userId)) return;
    requestedRef.current.add(userId);
    invoke("request_user_comment", { userId }).catch(() => {});
  };

  const channelName = (channelId: number): string => {
    const ch = channels.find((c) => c.id === channelId);
    return ch?.name || "Root";
  };

  const groups = useMemo(
    () => buildMemberGroups(users, registered, ownSession, aclGroups, fetchedComments),
    [users, registered, ownSession, aclGroups, fetchedComments],
  );

  const totalMembers = useMemo(
    () => groups.reduce((sum, g) => sum + g.rows.length, 0),
    [groups],
  );

  if (totalMembers === 0) {
    return (
      <div className={styles.membersTab}>
        <div className={styles.membersEmpty}>No other members</div>
      </div>
    );
  }

  return (
    <div className={styles.membersTab}>
      {groups.map((group) => (
        <section key={group.key} className={styles.memberGroup}>
          <div
            className={styles.membersGroupTitle}
            style={group.color ? { color: group.color } : undefined}
          >
            {group.label} - {group.rows.length}
          </div>
          <div className={styles.memberGroupBody}>
            {group.rows.map((row) => (
              <UserListItem
                key={row.entry.session}
                user={row.entry}
                channelName={row.offline ? undefined : channelName(row.entry.channel_id)}
                active={!row.offline && selectedDmUser === row.entry.session}
                isTalking={!row.offline && talkingSessions.has(row.entry.session)}
                offline={row.offline}
                onClick={row.offline ? undefined : () => onSelectDm(row.entry.session)}
                onContextMenu={row.offline ? undefined : (e) => onUserContextMenu(e, row.entry)}
                onRequestComment={row.offline ? handleRequestComment : undefined}
              />
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}
