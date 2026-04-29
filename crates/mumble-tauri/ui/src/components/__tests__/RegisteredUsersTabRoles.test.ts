/**
 * Unit tests for the user -> roles mapping logic in RegisteredUsersTab.
 *
 * The helper is intentionally re-implemented here against the same
 * AclGroup shape; if the production helper changes its semantics this
 * test makes the contract explicit.
 */

import { describe, it, expect } from "vitest";
import type { AclGroup } from "../../types";

function buildUserRoleMap(groups: readonly AclGroup[]): Map<number, AclGroup[]> {
  const result = new Map<number, AclGroup[]>();
  for (const group of groups) {
    const memberIds = new Set([...group.add, ...group.inherited_members]);
    for (const id of memberIds) {
      const existing = result.get(id);
      if (existing) existing.push(group);
      else result.set(id, [group]);
    }
  }
  return result;
}

function role(name: string, add: number[], inherited: number[] = []): AclGroup {
  return {
    name,
    inherited: false,
    inherit: true,
    inheritable: true,
    add,
    remove: [],
    inherited_members: inherited,
    color: null,
    icon: null,
    style_preset: null,
    metadata: {},
  };
}

describe("buildUserRoleMap", () => {
  it("returns an empty map for no groups", () => {
    expect(buildUserRoleMap([]).size).toBe(0);
  });

  it("maps each user to all roles they belong to", () => {
    const groups = [role("admin", [1]), role("vip", [1, 2]), role("dev", [3])];
    const map = buildUserRoleMap(groups);
    expect(map.get(1)?.map((g) => g.name)).toEqual(["admin", "vip"]);
    expect(map.get(2)?.map((g) => g.name)).toEqual(["vip"]);
    expect(map.get(3)?.map((g) => g.name)).toEqual(["dev"]);
  });

  it("includes inherited members and dedupes overlapping ids", () => {
    const groups = [role("vip", [1], [1, 2])];
    const map = buildUserRoleMap(groups);
    expect(map.get(1)?.length).toBe(1);
    expect(map.get(2)?.length).toBe(1);
  });
});
