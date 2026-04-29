import { describe, it, expect } from "vitest";
import type { UserEntry, AclGroup } from "../../types";
import {
  membersForRole,
  membersForChannelMention,
  MAX_DISPLAYED_MEMBERS,
} from "../chat/MentionPopover";

function user(session: number, channel_id: number, user_id: number | null = null): UserEntry {
  return {
    session,
    name: `user-${session}`,
    channel_id,
    user_id,
    texture: null,
    comment: null,
    mute: false,
    deaf: false,
    suppress: false,
    self_mute: false,
    self_deaf: false,
    priority_speaker: false,
  };
}

function group(name: string, add: number[], inherited: number[] = [], remove: number[] = []): AclGroup {
  return {
    name,
    inherited: false,
    inherit: true,
    inheritable: true,
    add,
    remove,
    inherited_members: inherited,
  };
}

describe("MentionPopover helpers", () => {
  describe("membersForChannelMention", () => {
    it("returns only users in the selected channel", () => {
      const users = [user(1, 10, 100), user(2, 10, 101), user(3, 20, 102)];
      const out = membersForChannelMention(users, 10);
      expect(out.map((u) => u.session)).toEqual([1, 2]);
    });

    it("returns all users when no channel is selected", () => {
      const users = [user(1, 10), user(2, 20)];
      expect(membersForChannelMention(users, null)).toHaveLength(2);
    });
  });

  describe("membersForRole", () => {
    it("returns matching online users from add + inherited minus remove", () => {
      const users = [
        user(1, 0, 100),
        user(2, 0, 101),
        user(3, 0, 102),
        user(4, 0, 103),
      ];
      const groups = [group("admin", [100, 101], [102], [101])];
      const out = membersForRole(users, "admin", groups);
      // 100 from add, 102 from inherited, 101 removed.
      expect(out.map((u) => u.user_id)).toEqual([100, 102]);
    });

    it("returns empty when group is unknown", () => {
      expect(membersForRole([user(1, 0, 1)], "missing", [])).toHaveLength(0);
    });

    it("ignores users without a registered user_id", () => {
      const groups = [group("admin", [100], [])];
      const out = membersForRole([user(1, 0, null), user(2, 0, 100)], "admin", groups);
      expect(out.map((u) => u.session)).toEqual([2]);
    });
  });

  describe("MAX_DISPLAYED_MEMBERS", () => {
    it("is a sane positive limit", () => {
      expect(MAX_DISPLAYED_MEMBERS).toBeGreaterThan(0);
      expect(MAX_DISPLAYED_MEMBERS).toBeLessThanOrEqual(100);
    });
  });
});
