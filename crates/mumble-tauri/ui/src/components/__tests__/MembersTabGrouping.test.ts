import { describe, it, expect } from "vitest";
import { buildMemberGroups } from "../sidebar/MembersTab";
import type { AclGroup, RegisteredUser, UserEntry } from "../../types";

function user(session: number, name: string, userId: number | null = null): UserEntry {
  return {
    session,
    name,
    channel_id: 0,
    user_id: userId,
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

function reg(userId: number, name: string): RegisteredUser {
  return { user_id: userId, name };
}

function aclGroup(
  name: string,
  add: number[],
  inherited: number[] = [],
  color: string | null = null,
): AclGroup {
  return {
    name,
    inherited: false,
    inherit: true,
    inheritable: true,
    add,
    remove: [],
    inherited_members: inherited,
    color,
  };
}

describe("buildMemberGroups", () => {
  it("groups online users by their first non-system ACL group", () => {
    const groups = buildMemberGroups(
      [user(1, "Alice", 10), user(2, "Bob", 20), user(3, "Carol", 30)],
      [],
      null,
      [aclGroup("admin", [10]), aclGroup("mods", [20, 30])],
    );
    expect(groups.map((g) => g.label)).toEqual(["admin", "mods"]);
    expect(groups[0].rows.map((r) => r.entry.name)).toEqual(["Alice"]);
    expect(groups[1].rows.map((r) => r.entry.name)).toEqual(["Bob", "Carol"]);
  });

  it("merges offline registered users into the same role groups, online first", () => {
    const groups = buildMemberGroups(
      [user(1, "OnlineAdmin", 10)],
      [reg(11, "OfflineAdmin"), reg(20, "OfflineMod")],
      null,
      [aclGroup("admin", [10, 11]), aclGroup("mods", [20])],
    );
    expect(groups[0].rows.map((r) => [r.entry.name, r.offline])).toEqual([
      ["OnlineAdmin", false],
      ["OfflineAdmin", true],
    ]);
    expect(groups[1].rows.map((r) => [r.entry.name, r.offline])).toEqual([
      ["OfflineMod", true],
    ]);
  });

  it("skips system groups starting with ~ and falls through to Members", () => {
    const groups = buildMemberGroups(
      [user(1, "Alice", 10)],
      [],
      null,
      [aclGroup("~admin", [10]), aclGroup("~all", [10])],
    );
    expect(groups).toHaveLength(1);
    expect(groups[0].label).toBe("Members");
    expect(groups[0].rows[0].entry.name).toBe("Alice");
  });

  it("buckets unregistered online users into Guests", () => {
    const groups = buildMemberGroups(
      [user(1, "Anon", null), user(2, "Member", 5)],
      [],
      null,
      [aclGroup("staff", [5])],
    );
    expect(groups.map((g) => g.label)).toEqual(["staff", "Guests"]);
    expect(groups[1].rows[0].entry.name).toBe("Anon");
  });

  it("excludes the own session from any bucket", () => {
    const groups = buildMemberGroups(
      [user(1, "Me", 10), user(2, "Other", 20)],
      [],
      1,
      [aclGroup("staff", [10, 20])],
    );
    expect(groups[0].rows.map((r) => r.entry.name)).toEqual(["Other"]);
  });

  it("does not duplicate users present both online and in registered list", () => {
    const groups = buildMemberGroups(
      [user(1, "Alice", 10)],
      [reg(10, "Alice")],
      null,
      [aclGroup("staff", [10])],
    );
    const total = groups.reduce((s, g) => s + g.rows.length, 0);
    expect(total).toBe(1);
    expect(groups[0].rows[0].offline).toBe(false);
  });

  it("propagates the role color from the ACL group", () => {
    const groups = buildMemberGroups(
      [user(1, "Alice", 10)],
      [],
      null,
      [aclGroup("staff", [10], [], "#ff00aa")],
    );
    expect(groups[0].color).toBe("#ff00aa");
  });

  it("uses texture from the server UserList response for offline users", () => {
    const textureBytes = [1, 2, 3, 4];
    const reg = { user_id: 10, name: "Bob", texture: textureBytes };
    const groups = buildMemberGroups([], [reg], null, []);
    expect(groups).toHaveLength(1);
    const row = groups[0].rows[0];
    expect(row.offline).toBe(true);
    expect(row.entry.texture).toEqual(textureBytes);
  });

  it("leaves texture null when the server UserList response has no texture", () => {
    const reg = { user_id: 99, name: "Carol" };
    const groups = buildMemberGroups([], [reg], null, []);
    expect(groups[0].rows[0].entry.texture).toBeNull();
  });

  it("uses inline comment from RegisteredUser when no fetchedComments entry exists", () => {
    const reg = { user_id: 10, name: "Alice", comment: "Hello world" };
    const groups = buildMemberGroups([], [reg], null, []);
    expect(groups[0].rows[0].entry.comment).toBe("Hello world");
  });

  it("leaves comment null when RegisteredUser has no comment", () => {
    const reg = { user_id: 10, name: "Alice" };
    const groups = buildMemberGroups([], [reg], null, []);
    expect(groups[0].rows[0].entry.comment).toBeNull();
  });

  it("prefers fetchedComments over the inline reg.comment", () => {
    const reg = { user_id: 10, name: "Alice", comment: "short inline" };
    const fetched = new Map([[10, "full fetched comment"]]);
    const groups = buildMemberGroups([], [reg], null, [], fetched);
    expect(groups[0].rows[0].entry.comment).toBe("full fetched comment");
  });

  it("uses fetchedComments when reg.comment is null (lazy blob fetch scenario)", () => {
    const reg = { user_id: 10, name: "Alice", comment: null };
    const fetched = new Map([[10, "fetched after hover"]]);
    const groups = buildMemberGroups([], [reg], null, [], fetched);
    expect(groups[0].rows[0].entry.comment).toBe("fetched after hover");
  });
});
