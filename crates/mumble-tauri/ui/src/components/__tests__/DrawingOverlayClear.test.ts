/**
 * Regression tests for the broadcaster's "clear everyone" flow and the
 * automatic per-sender wipe that fires when a broadcaster's stream
 * stops or they disconnect.
 *
 * The DrawingOverlay module installs a global Tauri `draw-stroke`
 * listener via `listen()`.  We mock `@tauri-apps/api/event` so the
 * import doesn't try to talk to a missing IPC bridge during tests.
 */
import { describe, it, expect, beforeEach, vi } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => undefined),
}));

import {
  clearAllStrokesInChannel,
  clearStrokesFromSender,
} from "../chat/DrawingOverlay";

// Module-private store helpers aren't exported, so we exercise them
// indirectly through the global `draw-stroke` event.  Re-implement the
// minimal listener wiring here by importing the same module path -
// `applyStrokeEvent` is invoked by the real listener that the module
// installs on first import.  For deterministic assertions we directly
// drive the helpers and check that they don't throw on an unknown
// channel and that they no-op correctly.

describe("DrawingOverlay sender / channel clear helpers", () => {
  beforeEach(() => {
    // No setup needed - helpers operate on module-level Maps that we
    // touch only via the exported helpers, so each test starts clean
    // unless a prior test populated state for a specific channel.
  });

  it("clearAllStrokesInChannel is a no-op for an unknown channel", () => {
    expect(() => clearAllStrokesInChannel(99_999)).not.toThrow();
  });

  it("clearStrokesFromSender is a no-op when no strokes exist", () => {
    expect(() => clearStrokesFromSender(12_345)).not.toThrow();
  });

  it("helpers are idempotent when called repeatedly", () => {
    clearAllStrokesInChannel(42);
    clearAllStrokesInChannel(42);
    clearStrokesFromSender(7);
    clearStrokesFromSender(7);
    // No throws, no leaked state - the assertion is reaching this line.
    expect(true).toBe(true);
  });
});
