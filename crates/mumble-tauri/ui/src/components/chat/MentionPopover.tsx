import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { UserEntry } from "../../types";
import { useAppStore } from "../../store";
import { useAclGroups } from "../../hooks/useAclGroups";
import { useUserStats } from "../../hooks/useUserStats";
import { parseComment } from "../../profileFormat";
import { useUserAvatars } from "../../lazyBlobs";
import { colorFor } from "../../utils/format";
import { ProfilePreviewCard } from "../../pages/settings/ProfilePreviewCard";
import styles from "./ChatView.module.css";

/** Maximum number of members rendered in a group/everyone/here popover. */
export const MAX_DISPLAYED_MEMBERS = 30;

/** Approximate dimensions used to clamp the popover within the viewport. */
const POPOVER_WIDTH = 280;
const USER_CARD_WIDTH = 320;
const POPOVER_VERTICAL_GAP = 8;
const POPOVER_VIEWPORT_MARGIN = 12;

type MentionKind = "user" | "role" | "everyone" | "here";

interface MentionState {
  readonly kind: MentionKind;
  /** For user mentions: session id. For role mentions: group name. */
  readonly target: string;
  readonly anchorRect: DOMRect;
}

interface MentionRowProps {
  readonly user: UserEntry;
  readonly avatarUrl: string | undefined;
}

function MentionRow({ user, avatarUrl }: MentionRowProps) {
  return (
    <div className={styles.mentionMemberRow}>
      {avatarUrl ? (
        <img src={avatarUrl} alt="" className={styles.mentionMemberAvatarImg} />
      ) : (
        <div
          className={styles.mentionMemberAvatar}
          style={{ background: colorFor(user.name) }}
        >
          {user.name.charAt(0).toUpperCase()}
        </div>
      )}
      <span className={styles.mentionMemberName}>{user.name}</span>
    </div>
  );
}

interface UserMentionCardProps {
  readonly session: number;
  readonly user: UserEntry;
  readonly avatarUrl: string | undefined;
}

function UserMentionCard({ session, user, avatarUrl }: UserMentionCardProps) {
  const stats = useUserStats(session, true);
  const parsed = useMemo(
    () => (user.comment ? parseComment(user.comment) : null),
    [user.comment],
  );
  return (
    <ProfilePreviewCard
      profile={parsed?.profile ?? {}}
      bio={parsed?.bio ?? ""}
      avatar={avatarUrl ?? null}
      displayName={user.name}
      onlinesecs={stats?.onlinesecs}
      idlesecs={stats?.idlesecs}
      isRegistered={user.user_id != null && user.user_id > 0}
    />
  );
}

interface MemberListProps {
  readonly title: string;
  readonly subtitle?: string;
  readonly members: readonly UserEntry[];
  readonly avatarBySession: ReadonlyMap<number, string>;
}

function MemberList({ title, subtitle, members, avatarBySession }: MemberListProps) {
  const total = members.length;
  const displayed = total > MAX_DISPLAYED_MEMBERS
    ? members.slice(0, MAX_DISPLAYED_MEMBERS)
    : members;
  const overflow = total - displayed.length;

  return (
    <div className={styles.mentionMemberList}>
      <div className={styles.mentionMemberHeader}>
        <span className={styles.mentionMemberTitle}>{title}</span>
        <span className={styles.mentionMemberCount}>{total}</span>
      </div>
      {subtitle && <div className={styles.mentionMemberSubtitle}>{subtitle}</div>}
      {total === 0 ? (
        <div className={styles.mentionMemberEmpty}>No members</div>
      ) : (
        <div className={styles.mentionMemberScroll}>
          {displayed.map((u) => (
            <MentionRow
              key={u.session}
              user={u}
              avatarUrl={avatarBySession.get(u.session)}
            />
          ))}
          {overflow > 0 && (
            <div className={styles.mentionMemberOverflow}>
              +{overflow} more
            </div>
          )}
        </div>
      )}
    </div>
  );
}

/** Find the closest ancestor mention span and return its mention info. */
function readMentionFromTarget(target: EventTarget | null): MentionState | null {
  if (!(target instanceof Element)) return null;
  const el = target.closest<HTMLElement>(
    "[data-mention-session], [data-mention-role], [data-mention-everyone], [data-mention-here]",
  );
  if (!el) return null;
  // Only intercept clicks inside a chat message body.
  if (!el.closest(`.${CSS.escape(styles.messageBody)}`)) return null;
  const rect = el.getBoundingClientRect();
  if (el.dataset.mentionSession) {
    return { kind: "user", target: el.dataset.mentionSession, anchorRect: rect };
  }
  if (el.dataset.mentionRole) {
    return { kind: "role", target: el.dataset.mentionRole, anchorRect: rect };
  }
  if (el.dataset.mentionEveryone) {
    return { kind: "everyone", target: "", anchorRect: rect };
  }
  if (el.dataset.mentionHere) {
    return { kind: "here", target: "", anchorRect: rect };
  }
  return null;
}

/** Compute clamped popover position relative to the anchor rect. */
function computePosition(rect: DOMRect, popoverWidth: number): { top: number; left: number } {
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const left = Math.max(
    POPOVER_VIEWPORT_MARGIN,
    Math.min(rect.left, vw - popoverWidth - POPOVER_VIEWPORT_MARGIN),
  );
  const preferredTop = rect.bottom + POPOVER_VERTICAL_GAP;
  const top = preferredTop;
  // Vertical clamping is best-effort; the popover itself is scrollable
  // and constrained by max-height in CSS.
  return { top: Math.min(top, vh - POPOVER_VIEWPORT_MARGIN), left };
}

/**
 * Build the displayed member list for an everyone/here mention.
 *
 * Uses the currently selected channel as the scope - matches how
 * the renderer treats `@everyone` (everyone in this channel).
 */
export function membersForChannelMention(
  users: readonly UserEntry[],
  selectedChannel: number | null,
): readonly UserEntry[] {
  if (selectedChannel == null) return users;
  return users.filter((u) => u.channel_id === selectedChannel);
}

/** Build the displayed member list for a role mention. */
export function membersForRole(
  users: readonly UserEntry[],
  groupName: string,
  groups: readonly { name: string; add: number[]; remove: number[]; inherited_members: number[] }[],
): readonly UserEntry[] {
  const group = groups.find((g) => g.name === groupName);
  if (!group) return [];
  const memberIds = new Set<number>([...group.add, ...group.inherited_members]);
  for (const id of group.remove) memberIds.delete(id);
  return users.filter((u) => u.user_id != null && memberIds.has(u.user_id));
}

/**
 * Renders an interactive popover when a user clicks an `@mention`
 * chip inside a chat message body.
 *
 * - `@user` shows the full profile preview card.
 * - `@role` / `@everyone` / `@here` show a scrollable list of matching
 *   members, capped at MAX_DISPLAYED_MEMBERS with a `+N more` footer.
 *
 * Mounted once at the chat root; uses event delegation on `document`
 * so it works regardless of how/when message HTML is injected.
 */
export default function MentionPopover() {
  const [state, setState] = useState<MentionState | null>(null);
  const popoverRef = useRef<HTMLDivElement>(null);

  const users = useAppStore((s) => s.users);
  const selectedChannel = useAppStore((s) => s.selectedChannel);
  const aclGroups = useAclGroups();

  const usersBySession = useMemo(() => {
    const m = new Map<number, UserEntry>();
    for (const u of users) m.set(u.session, u);
    return m;
  }, [users]);

  // Lazy-fetched avatar data-URLs keyed by session.
  const avatarBySession = useUserAvatars(users);

  // Document-level click delegation so mention chips become interactive
  // regardless of which component injected the HTML.
  useEffect(() => {
    const onClick = (ev: MouseEvent) => {
      // Allow the popover itself to be clicked without re-triggering.
      if (popoverRef.current?.contains(ev.target as Node)) return;

      const next = readMentionFromTarget(ev.target);
      if (next) {
        ev.preventDefault();
        ev.stopPropagation();
        setState(next);
      } else {
        setState(null);
      }
    };
    const onKey = (ev: KeyboardEvent) => {
      if (ev.key === "Escape") setState(null);
    };
    document.addEventListener("click", onClick, true);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("click", onClick, true);
      document.removeEventListener("keydown", onKey);
    };
  }, []);

  // Reposition on scroll/resize - simplest is to just close.
  useEffect(() => {
    if (!state) return;
    const close = () => setState(null);
    window.addEventListener("resize", close);
    window.addEventListener("scroll", close, true);
    return () => {
      window.removeEventListener("resize", close);
      window.removeEventListener("scroll", close, true);
    };
  }, [state]);

  const close = useCallback(() => setState(null), []);

  if (!state) return null;

  const isUserMention = state.kind === "user";
  const popoverWidth = isUserMention ? USER_CARD_WIDTH : POPOVER_WIDTH;
  const pos = computePosition(state.anchorRect, popoverWidth);

  let body: React.ReactNode;
  if (isUserMention) {
    const session = Number(state.target);
    const user = Number.isFinite(session) ? usersBySession.get(session) : undefined;
    if (!user) {
      body = <div className={styles.mentionMemberEmpty}>User is offline</div>;
    } else {
      body = (
        <UserMentionCard
          session={session}
          user={user}
          avatarUrl={avatarBySession.get(session)}
        />
      );
    }
  } else if (state.kind === "role") {
    const members = membersForRole(users, state.target, aclGroups);
    body = (
      <MemberList
        title={`@${state.target}`}
        subtitle="Online members"
        members={members}
        avatarBySession={avatarBySession}
      />
    );
  } else {
    const members = membersForChannelMention(users, selectedChannel);
    const label = state.kind === "everyone" ? "@everyone" : "@here";
    body = (
      <MemberList
        title={label}
        subtitle="Members in this channel"
        members={members}
        avatarBySession={avatarBySession}
      />
    );
  }

  return createPortal(
    <div
      ref={popoverRef}
      className={styles.mentionPopover}
      style={{
        top: pos.top,
        left: pos.left,
        width: popoverWidth,
      }}
      role="dialog"
      onClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        if (e.key === "Escape") close();
      }}
    >
      {body}
    </div>,
    document.body,
  );
}
