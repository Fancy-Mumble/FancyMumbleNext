/**
 * Unit tests for server password persistence (serverStorage).
 *
 * Verifies that passwords can be stored, retrieved, and removed
 * per server id, and that removeServer also cleans up passwords.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";

// In-memory store backing for the mock.
// Keyed by store file name so servers.json and passwords.json are isolated.
const storeFiles: Record<string, Record<string, unknown>> = {};

vi.mock("@tauri-apps/plugin-store", () => ({
  load: vi.fn().mockImplementation((fileName: string) => {
    if (!storeFiles[fileName]) storeFiles[fileName] = {};
    const data = storeFiles[fileName];
    return Promise.resolve({
      get: vi.fn().mockImplementation((key: string) =>
        Promise.resolve(data[key] ?? null),
      ),
      set: vi.fn().mockImplementation((key: string, value: unknown) => {
        data[key] = value;
        return Promise.resolve();
      }),
    });
  }),
}));

// Import after mocks are in place.
import {
  getServerPassword,
  setServerPassword,
  removeServerPassword,
  removeServer,
  addServer,
} from "../../serverStorage";

beforeEach(() => {
  for (const key of Object.keys(storeFiles)) {
    delete storeFiles[key];
  }
});

describe("Password storage", () => {
  it("returns null for unknown server", async () => {
    const pw = await getServerPassword("nonexistent");
    expect(pw).toBeNull();
  });

  it("stores and retrieves a password", async () => {
    await setServerPassword("server-1", "secret123");
    const pw = await getServerPassword("server-1");
    expect(pw).toBe("secret123");
  });

  it("overwrites an existing password", async () => {
    await setServerPassword("server-1", "old");
    await setServerPassword("server-1", "new");
    const pw = await getServerPassword("server-1");
    expect(pw).toBe("new");
  });

  it("removes a password by setting null", async () => {
    await setServerPassword("server-1", "secret");
    await setServerPassword("server-1", null);
    const pw = await getServerPassword("server-1");
    expect(pw).toBeNull();
  });

  it("removeServerPassword clears the password", async () => {
    await setServerPassword("server-2", "pw");
    await removeServerPassword("server-2");
    expect(await getServerPassword("server-2")).toBeNull();
  });

  it("isolates passwords between servers", async () => {
    await setServerPassword("a", "alpha");
    await setServerPassword("b", "beta");
    expect(await getServerPassword("a")).toBe("alpha");
    expect(await getServerPassword("b")).toBe("beta");
  });

  it("removeServer also removes the stored password", async () => {
    const server = await addServer({
      label: "Test",
      host: "example.com",
      port: 64738,
      username: "user",
      cert_label: null,
    });
    await setServerPassword(server.id, "pw");
    expect(await getServerPassword(server.id)).toBe("pw");

    await removeServer(server.id);
    expect(await getServerPassword(server.id)).toBeNull();
  });
});
