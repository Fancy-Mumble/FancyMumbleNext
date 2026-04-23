/**
 * Mentions support (Discord-style adapted for Mumble).
 *
 * Wire format inside the raw markdown / message body:
 *   <@SESSION>        user mention (numeric live session id)
 *   <@&GROUP_NAME>    ACL group / role mention
 *   @everyone         channel-wide mention
 *   @here             "online here" mention
 *
 * On send, `applyMentionsToHtml` rewrites these markers into rendered
 * chip spans with `data-mention-*` attributes so receivers (which just
 * see the HTML body) can both display chips and detect self-mentions
 * for notifications without needing to re-parse the raw markup.
 */

const HTML_ESCAPE: Record<string, string> = {
  "&": "&amp;",
  "<": "&lt;",
  ">": "&gt;",
  "\"": "&quot;",
  "'": "&#39;",
};

function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) => HTML_ESCAPE[c]);
}

// -- Wire format helpers -----------------------------------------------

const USER_MENTION_RE = /<@(\d+)>/g;
const ROLE_MENTION_RE = /<@&([^>\s]+)>/g;

/** Format a user mention marker for insertion into the draft. */
export function formatUserMention(session: number): string {
  return `<@${session}>`;
}

/** Format a role/group mention marker for insertion into the draft. */
export function formatRoleMention(name: string): string {
  return `<@&${name}>`;
}

// -- Trigger detection (autocomplete) ----------------------------------

export interface MentionTrigger {
  /** Index of the `@` character in the draft. */
  readonly anchor: number;
  /** Search query typed after the `@` (excluding the `@` itself). */
  readonly query: string;
  /** Mention kind based on the leading character after `@`. */
  readonly kind: "user" | "role";
}

/**
 * Detect an active mention trigger at the current cursor position.
 *
 * Returns null when no `@` is currently being typed. Triggers terminate
 * on whitespace or when the `@` is preceded by a non-whitespace,
 * non-newline character (so email addresses don't trigger).
 */
export function parseMentionTrigger(
  draft: string,
  cursor: number,
): MentionTrigger | null {
  if (cursor < 1) return null;

  // Walk backwards from cursor looking for an unbroken token.
  let i = cursor - 1;
  while (i >= 0) {
    const ch = draft.charAt(i);
    if (ch === "@") break;
    // Whitespace, control chars, or angle brackets break the token.
    if (/[\s<>]/.test(ch)) return null;
    i -= 1;
  }
  if (i < 0 || draft.charAt(i) !== "@") return null;

  // The `@` must be at start-of-text or preceded by whitespace.
  if (i > 0) {
    const prev = draft.charAt(i - 1);
    if (!/\s/.test(prev)) return null;
  }

  const after = draft.charAt(i + 1);
  const kind: "user" | "role" = after === "&" ? "role" : "user";
  const queryStart = kind === "role" ? i + 2 : i + 1;
  const query = draft.slice(queryStart, cursor);

  // Don't trigger for `@everyone` / `@here` exact matches (those are
  // their own thing and inserted as literal text without a chip
  // converter call).  Treat them as autocomplete suggestions instead.
  return { anchor: i, query, kind };
}

// -- HTML chip rendering -----------------------------------------------

export interface MentionResolver {
  /** Resolve a session ID to a display name for chip rendering. */
  resolveSession(session: number): { name: string } | null;
  /**
   * Resolve a role/group name to its FancyMumble customisation.
   * Optional - resolvers that only render user mentions can omit it.
   */
  resolveRole?(name: string): { color?: string | null } | null;
}

/** Sanitise a CSS color before embedding it in an inline style attribute. */
function sanitiseColor(color: string): string | null {
  const trimmed = color.trim();
  if (trimmed.length === 0 || trimmed.length > 64) return null;
  // Allow simple #rgb/#rgba hex, rgb()/rgba()/hsl()/hsla() and a-z color
  // names. Reject anything containing characters that could break out of
  // the inline style attribute.
  if (!/^[#A-Za-z0-9 .,()%/]+$/.test(trimmed)) return null;
  return trimmed;
}

/**
 * Convert mention markers in already-HTML-escaped text into chip spans.
 *
 * Called AFTER `markdownToHtml` (which escapes `<`/`>`/`&`), so the
 * markers appear as `&lt;@123&gt;` and `&lt;@&amp;NAME&gt;` in the
 * input.  The output is safe HTML containing `<span class="mention"...>`
 * elements that the receiver renders directly.
 */
export function applyMentionsToHtml(
  escapedHtml: string,
  resolver: MentionResolver,
): string {
  let out = escapedHtml;

  // <@SESSION>  ->  &lt;@SESSION&gt;
  out = out.replace(/&lt;@(\d+)&gt;/g, (_match, sid) => {
    const session = Number(sid);
    const resolved = resolver.resolveSession(session);
    const name = resolved?.name ?? `user-${session}`;
    return `<span class="mention mention-user" data-mention-session="${session}">@${escapeHtml(name)}</span>`;
  });

  // <@&NAME>  ->  &lt;@&amp;NAME&gt;
  out = out.replace(/&lt;@&amp;([^&\s<>]+)&gt;/g, (_match, name) => {
    const resolved = resolver.resolveRole?.(name);
    const safeColor = resolved?.color ? sanitiseColor(resolved.color) : null;
    const styleAttr = safeColor
      ? ` style="color:${safeColor};background:color-mix(in srgb, ${safeColor} 22%, transparent)"`
      : "";
    return `<span class="mention mention-role" data-mention-role="${escapeHtml(name)}"${styleAttr}>@${escapeHtml(name)}</span>`;
  });

  // @everyone (only at start of text or after whitespace)
  out = out.replace(/(^|\s)@everyone\b/g, (_m, lead) =>
    `${lead}<span class="mention mention-everyone" data-mention-everyone="1">@everyone</span>`);

  // @here
  out = out.replace(/(^|\s)@here\b/g, (_m, lead) =>
    `${lead}<span class="mention mention-here" data-mention-here="1">@here</span>`);

  return out;
}

// -- Receive-side detection (notifications) ----------------------------

export interface MentionTargets {
  readonly sessions: ReadonlySet<number>;
  readonly roles: ReadonlySet<string>;
  readonly everyone: boolean;
  readonly here: boolean;
}

/** Extract the set of mention targets from a rendered HTML body. */
export function extractMentionTargets(html: string): MentionTargets {
  const sessions = new Set<number>();
  const roles = new Set<string>();
  let everyone = false;
  let here = false;

  for (const m of html.matchAll(/data-mention-session="(\d+)"/g)) {
    const n = Number(m[1]);
    if (Number.isFinite(n)) sessions.add(n);
  }
  for (const m of html.matchAll(/data-mention-role="([^"]+)"/g)) {
    roles.add(m[1]);
  }
  if (/data-mention-everyone="1"/.test(html)) everyone = true;
  if (/data-mention-here="1"/.test(html)) here = true;

  // Also support legacy/raw markers in case the body wasn't run through
  // applyMentionsToHtml (e.g. messages from older clients).
  USER_MENTION_RE.lastIndex = 0;
  for (const m of html.matchAll(USER_MENTION_RE)) {
    sessions.add(Number(m[1]));
  }
  ROLE_MENTION_RE.lastIndex = 0;
  for (const m of html.matchAll(ROLE_MENTION_RE)) {
    roles.add(m[1]);
  }
  if (/(^|\s|>)@everyone\b/.test(html)) everyone = true;
  if (/(^|\s|>)@here\b/.test(html)) here = true;

  return { sessions, roles, everyone, here };
}

export interface SelfMentionContext {
  readonly ownSession: number | null;
  /** Group/role names the receiving user is a member of. */
  readonly ownRoles?: ReadonlySet<string>;
  /** True when the receiver is currently in the message's channel. */
  readonly isInMessageChannel: boolean;
}

/** Return true if the message body mentions the receiving user. */
export function containsSelfMention(
  html: string,
  ctx: SelfMentionContext,
): boolean {
  const targets = extractMentionTargets(html);
  if (ctx.ownSession != null && targets.sessions.has(ctx.ownSession)) {
    return true;
  }
  if (ctx.isInMessageChannel && (targets.everyone || targets.here)) {
    return true;
  }
  if (ctx.ownRoles && targets.roles.size > 0) {
    for (const role of targets.roles) {
      if (ctx.ownRoles.has(role)) return true;
    }
  }
  return false;
}
