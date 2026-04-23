import { describe, it, expect } from "vitest";
import {
  parseMentionTrigger,
  formatUserMention,
  formatRoleMention,
  applyMentionsToHtml,
  extractMentionTargets,
  containsSelfMention,
  type MentionResolver,
} from "../../utils/mentions";

const resolver: MentionResolver = {
  resolveSession(session) {
    if (session === 1) return { name: "Alice" };
    if (session === 2) return { name: "Bob" };
    return null;
  },
};

describe("parseMentionTrigger", () => {
  it("returns null when there is no @", () => {
    expect(parseMentionTrigger("hello world", 5)).toBeNull();
  });

  it("detects a user trigger at the start", () => {
    const t = parseMentionTrigger("@al", 3);
    expect(t).toEqual({ anchor: 0, query: "al", kind: "user" });
  });

  it("detects a role trigger with @& prefix", () => {
    const t = parseMentionTrigger("hi @&mod", 8);
    expect(t).toEqual({ anchor: 3, query: "mod", kind: "role" });
  });

  it("does not trigger for inline @ (no preceding whitespace)", () => {
    expect(parseMentionTrigger("a@b", 3)).toBeNull();
  });

  it("does not trigger across whitespace", () => {
    expect(parseMentionTrigger("@al ", 4)).toBeNull();
  });

  it("does not trigger when cursor is before the @", () => {
    expect(parseMentionTrigger("@al", 0)).toBeNull();
  });

  it("trigger continues with empty query right after @", () => {
    const t = parseMentionTrigger("hi @", 4);
    expect(t).toEqual({ anchor: 3, query: "", kind: "user" });
  });
});

describe("applyMentionsToHtml", () => {
  it("replaces escaped <@N> with a chip", () => {
    const out = applyMentionsToHtml("&lt;@1&gt; hello", resolver);
    expect(out).toContain('class="mention mention-user"');
    expect(out).toContain('data-mention-session="1"');
    expect(out).toContain("@Alice");
  });

  it("falls back to user-N when session is unknown", () => {
    const out = applyMentionsToHtml("&lt;@99&gt;", resolver);
    expect(out).toContain("@user-99");
  });

  it("renders @everyone and @here chips", () => {
    const out = applyMentionsToHtml("hello @everyone and @here", resolver);
    expect(out).toContain('class="mention mention-everyone"');
    expect(out).toContain('class="mention mention-here"');
    expect(out).toContain('data-mention-everyone="1"');
    expect(out).toContain('data-mention-here="1"');
  });

  it("renders role chips", () => {
    const out = applyMentionsToHtml("&lt;@&amp;moderators&gt;", resolver);
    expect(out).toContain('data-mention-role="moderators"');
    expect(out).toContain("@moderators");
  });

  it("does not match @everyone mid-word", () => {
    const out = applyMentionsToHtml("foo@everyone", resolver);
    expect(out).not.toContain("mention-everyone");
  });
});

describe("extractMentionTargets", () => {
  it("collects sessions, roles, everyone, here from chip HTML", () => {
    const html =
      '<span data-mention-session="1">@A</span> ' +
      '<span data-mention-role="mods">@mods</span> ' +
      '<span data-mention-everyone="1">@everyone</span> ' +
      '<span data-mention-here="1">@here</span>';
    const t = extractMentionTargets(html);
    expect(t.sessions.has(1)).toBe(true);
    expect(t.roles.has("mods")).toBe(true);
    expect(t.everyone).toBe(true);
    expect(t.here).toBe(true);
  });

  it("also recognises raw markers from older clients", () => {
    const t = extractMentionTargets("hi <@5> and <@&team> @everyone");
    expect(t.sessions.has(5)).toBe(true);
    expect(t.roles.has("team")).toBe(true);
    expect(t.everyone).toBe(true);
  });
});

describe("containsSelfMention", () => {
  const html = '<span data-mention-session="42">@me</span>';

  it("matches by own session id", () => {
    expect(
      containsSelfMention(html, { ownSession: 42, isInMessageChannel: false }),
    ).toBe(true);
  });

  it("does not match other sessions", () => {
    expect(
      containsSelfMention(html, { ownSession: 7, isInMessageChannel: false }),
    ).toBe(false);
  });

  it("matches @everyone only when in the message's channel", () => {
    const ev = '<span data-mention-everyone="1">@everyone</span>';
    expect(
      containsSelfMention(ev, { ownSession: 1, isInMessageChannel: true }),
    ).toBe(true);
    expect(
      containsSelfMention(ev, { ownSession: 1, isInMessageChannel: false }),
    ).toBe(false);
  });

  it("matches a role when receiver belongs to it", () => {
    const r = '<span data-mention-role="mods">@mods</span>';
    expect(
      containsSelfMention(r, {
        ownSession: 1,
        ownRoles: new Set(["mods"]),
        isInMessageChannel: false,
      }),
    ).toBe(true);
    expect(
      containsSelfMention(r, {
        ownSession: 1,
        ownRoles: new Set(["admins"]),
        isInMessageChannel: false,
      }),
    ).toBe(false);
  });
});

describe("format helpers", () => {
  it("formatUserMention produces the wire marker", () => {
    expect(formatUserMention(7)).toBe("<@7>");
  });
  it("formatRoleMention produces the wire marker", () => {
    expect(formatRoleMention("admins")).toBe("<@&admins>");
  });
});
