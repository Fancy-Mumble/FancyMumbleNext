/**
 * Unit tests for per-identity profile storage (profileData).
 *
 * Verifies that profiles are loaded/saved per identity label,
 * migration from global format works, and deletion cleans up.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";

// In-memory store backing for the mock.
let storeData: Record<string, unknown> = {};

vi.mock("@tauri-apps/plugin-store", () => ({
  load: vi.fn().mockImplementation(() =>
    Promise.resolve({
      get: vi.fn().mockImplementation((key: string) =>
        Promise.resolve(storeData[key] ?? null),
      ),
      set: vi.fn().mockImplementation((key: string, value: unknown) => {
        storeData[key] = value;
        return Promise.resolve();
      }),
      delete: vi.fn().mockImplementation((key: string) => {
        delete storeData[key];
        return Promise.resolve();
      }),
    }),
  ),
}));

// Import after mocks are in place.
import {
  loadProfileData,
  saveProfileData,
  deleteProfileData,
  migrateProfilesToIdentities,
} from "../../pages/settings/profileData";

import type { ProfileData } from "../../pages/settings/profileData";

const SAMPLE_PROFILE: ProfileData = {
  profile: { decoration: "sparkle", status: "testing" },
  bio: "Hello world",
  avatarDataUrl: "data:image/png;base64,abc",
};

describe("Per-identity profile storage", () => {
  beforeEach(() => {
    storeData = {};
  });

  it("returns defaults when no profile exists (no identity)", async () => {
    const pd = await loadProfileData();
    expect(pd.profile).toEqual({});
    expect(pd.bio).toBe("");
    expect(pd.avatarDataUrl).toBeNull();
  });

  it("returns defaults when no profile exists (with identity)", async () => {
    const pd = await loadProfileData("my-identity");
    expect(pd.profile).toEqual({});
    expect(pd.bio).toBe("");
    expect(pd.avatarDataUrl).toBeNull();
  });

  it("saves and loads profile without identity (global)", async () => {
    await saveProfileData(SAMPLE_PROFILE);
    const pd = await loadProfileData();
    expect(pd.profile.decoration).toBe("sparkle");
    expect(pd.bio).toBe("Hello world");
    expect(pd.avatarDataUrl).toBe("data:image/png;base64,abc");
  });

  it("saves and loads profile for a specific identity", async () => {
    await saveProfileData(SAMPLE_PROFILE, "work");
    const pd = await loadProfileData("work");
    expect(pd.profile.decoration).toBe("sparkle");
    expect(pd.bio).toBe("Hello world");
  });

  it("isolates profiles between different identities", async () => {
    await saveProfileData(SAMPLE_PROFILE, "work");
    await saveProfileData(
      { profile: { decoration: "fire" }, bio: "Other", avatarDataUrl: null },
      "personal",
    );

    const work = await loadProfileData("work");
    const personal = await loadProfileData("personal");

    expect(work.profile.decoration).toBe("sparkle");
    expect(personal.profile.decoration).toBe("fire");
    expect(work.bio).toBe("Hello world");
    expect(personal.bio).toBe("Other");
  });

  it("identity profiles do not affect global profile", async () => {
    await saveProfileData(SAMPLE_PROFILE);
    await saveProfileData(
      { profile: { decoration: "fire" }, bio: "Identity-only", avatarDataUrl: null },
      "work",
    );

    const global = await loadProfileData();
    expect(global.profile.decoration).toBe("sparkle");
    expect(global.bio).toBe("Hello world");
  });

  it("deletes profile for a specific identity", async () => {
    await saveProfileData(SAMPLE_PROFILE, "work");
    await deleteProfileData("work");
    const pd = await loadProfileData("work");
    expect(pd.profile).toEqual({});
    expect(pd.bio).toBe("");
  });

  it("deletion of one identity does not affect another", async () => {
    await saveProfileData(SAMPLE_PROFILE, "work");
    await saveProfileData(
      { profile: { decoration: "fire" }, bio: "Keep me", avatarDataUrl: null },
      "personal",
    );
    await deleteProfileData("work");

    const personal = await loadProfileData("personal");
    expect(personal.profile.decoration).toBe("fire");
    expect(personal.bio).toBe("Keep me");
  });
});

describe("Profile migration", () => {
  beforeEach(() => {
    storeData = {};
  });

  it("copies global profile to all identities on first migration", async () => {
    storeData["data"] = SAMPLE_PROFILE;

    await migrateProfilesToIdentities(["alpha", "beta"]);

    const alpha = await loadProfileData("alpha");
    const beta = await loadProfileData("beta");
    expect(alpha.profile.decoration).toBe("sparkle");
    expect(beta.bio).toBe("Hello world");
  });

  it("does not overwrite existing per-identity profiles", async () => {
    storeData["data"] = SAMPLE_PROFILE;
    storeData["profile:alpha"] = {
      profile: { decoration: "fire" },
      bio: "Existing",
      avatarDataUrl: null,
    };

    await migrateProfilesToIdentities(["alpha", "beta"]);

    const alpha = await loadProfileData("alpha");
    expect(alpha.profile.decoration).toBe("fire");
    expect(alpha.bio).toBe("Existing");

    const beta = await loadProfileData("beta");
    expect(beta.profile.decoration).toBe("sparkle");
  });

  it("does not run migration twice", async () => {
    storeData["data"] = SAMPLE_PROFILE;
    await migrateProfilesToIdentities(["alpha"]);

    // Clear global and alpha to simulate fresh state
    delete storeData["profile:alpha"];
    storeData["data"] = {
      profile: { decoration: "ice" },
      bio: "Changed",
      avatarDataUrl: null,
    };

    await migrateProfilesToIdentities(["alpha"]);

    // Alpha should NOT have been re-populated since migration flag is set.
    const alpha = await loadProfileData("alpha");
    expect(alpha.profile).toEqual({});
  });

  it("handles missing global profile gracefully", async () => {
    await migrateProfilesToIdentities(["alpha"]);

    const alpha = await loadProfileData("alpha");
    expect(alpha.profile).toEqual({});
    expect(alpha.bio).toBe("");
  });

  it("handles empty identity list", async () => {
    storeData["data"] = SAMPLE_PROFILE;
    await migrateProfilesToIdentities([]);

    // Global should still be accessible
    const global = await loadProfileData();
    expect(global.profile.decoration).toBe("sparkle");
  });
});
