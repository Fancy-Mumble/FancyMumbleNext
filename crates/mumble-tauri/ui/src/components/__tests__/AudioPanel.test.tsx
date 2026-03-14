/**
 * Regression tests for AudioPanel features:
 * - Mic test start/stop flow
 * - VU meter rendering
 * - Auto input sensitivity toggle
 * - Shortcut event filtering (Pressed vs Released)
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, act } from "@testing-library/react";
import type { AudioSettings } from "../../types";

// -- Tauri mocks (must be declared before importing components) ----

const invokeMock = vi.fn<(cmd: string, args?: unknown) => Promise<unknown>>();
const listenMock = vi.fn<(event: string, handler: (event: { payload: unknown }) => void) => Promise<() => void>>();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(args[0] as string, args[1]),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (...args: unknown[]) => listenMock(args[0] as string, args[1] as (event: { payload: unknown }) => void),
}));

vi.mock("@tauri-apps/plugin-global-shortcut", () => ({
  register: vi.fn(),
  unregister: vi.fn(),
  isRegistered: vi.fn().mockResolvedValue(false),
}));

vi.mock("@tauri-apps/plugin-store", () => ({
  load: vi.fn().mockResolvedValue({
    get: vi.fn().mockResolvedValue(null),
    set: vi.fn().mockResolvedValue(undefined),
  }),
}));

// Import after mocks are in place.
import { AudioPanel } from "../../pages/settings/AudioPanel";
import { applyGlobalShortcut } from "../../pages/settings/shortcutHelpers";
import { register } from "@tauri-apps/plugin-global-shortcut";

// -- Helpers -------------------------------------------------------

function makeSettings(overrides: Partial<AudioSettings> = {}): AudioSettings {
  return {
    selected_device: null,
    auto_gain: true,
    vad_threshold: 0.3,
    max_gain_db: 15,
    noise_gate_close_ratio: 0.8,
    hold_frames: 15,
    push_to_talk: false,
    push_to_talk_key: null,
    bitrate_bps: 72000,
    frame_size_ms: 20,
    noise_suppression: true,
    selected_output_device: null,
    input_volume: 1,
    output_volume: 1,
    auto_input_sensitivity: false,
    ...overrides,
  };
}

function renderPanel(overrides: Partial<AudioSettings> = {}, isExpert = false) {
  const onChange = vi.fn();
  const settings = makeSettings(overrides);
  const result = render(
    <AudioPanel
      devices={[]}
      outputDevices={[]}
      settings={settings}
      onChange={onChange}
      isExpert={isExpert}
    />,
  );
  return { onChange, settings, ...result };
}

// -- Tests ---------------------------------------------------------

beforeEach(() => {
  vi.clearAllMocks();
  invokeMock.mockResolvedValue(undefined);
  listenMock.mockResolvedValue(vi.fn());
});

// ── Auto Input Sensitivity ────────────────────────────────────────

describe("Auto Input Sensitivity toggle", () => {
  it("shows manual threshold slider when auto sensitivity is off", () => {
    renderPanel({ auto_input_sensitivity: false });
    expect(screen.getByText("Threshold")).toBeTruthy();
  });

  it("hides manual threshold slider when auto sensitivity is on", () => {
    renderPanel({ auto_input_sensitivity: true });
    expect(screen.queryByText("Threshold")).toBeNull();
  });

  it("calls onChange with toggled auto_input_sensitivity", () => {
    const { onChange } = renderPanel({ auto_input_sensitivity: false });
    // The toggle is a button[role=switch] following the "Auto Sensitivity" label.
    const toggles = screen.getAllByRole("switch");
    // The first toggle in the voice activation section is Auto Sensitivity.
    const autoSensToggle = toggles.find(
      (btn) => btn.getAttribute("aria-checked") === "false"
        && btn.closest("section")?.textContent?.includes("Auto Sensitivity"),
    );
    expect(autoSensToggle).toBeTruthy();
    fireEvent.click(autoSensToggle!);
    expect(onChange).toHaveBeenCalledWith({ auto_input_sensitivity: true });
  });

  it("calls onChange to disable auto_input_sensitivity", () => {
    const { onChange } = renderPanel({ auto_input_sensitivity: true });
    const toggles = screen.getAllByRole("switch");
    const autoSensToggle = toggles.find(
      (btn) => btn.getAttribute("aria-checked") === "true"
        && btn.closest("section")?.textContent?.includes("Auto Sensitivity"),
    );
    expect(autoSensToggle).toBeTruthy();
    fireEvent.click(autoSensToggle!);
    expect(onChange).toHaveBeenCalledWith({ auto_input_sensitivity: false });
  });
});

// ── Mic Test ──────────────────────────────────────────────────────

describe("Mic Test", () => {
  it("renders the Mic Test button", () => {
    renderPanel();
    expect(screen.getByText("Mic Test")).toBeTruthy();
  });

  it("does not show VU meter when mic test is not active", () => {
    const { container } = renderPanel();
    expect(container.querySelector("[class*='vuMeter']")).toBeNull();
  });

  it("invokes start_mic_test when button is clicked", async () => {
    renderPanel();
    await act(async () => {
      fireEvent.click(screen.getByText("Mic Test"));
    });
    expect(invokeMock).toHaveBeenCalledWith("start_mic_test", undefined);
  });

  it("shows Stop Test button after starting", async () => {
    renderPanel();
    await act(async () => {
      fireEvent.click(screen.getByText("Mic Test"));
    });
    expect(screen.getByText("Stop Test")).toBeTruthy();
  });

  it("shows VU meter after starting mic test", async () => {
    renderPanel();
    await act(async () => {
      fireEvent.click(screen.getByText("Mic Test"));
    });
    // VU meter should contain dB labels
    expect(screen.getByText("-60")).toBeTruthy();
    expect(screen.getByText("0 dB")).toBeTruthy();
  });

  it("subscribes to mic-amplitude events when active", async () => {
    renderPanel();
    await act(async () => {
      fireEvent.click(screen.getByText("Mic Test"));
    });
    expect(listenMock).toHaveBeenCalledWith("mic-amplitude", expect.any(Function));
  });

  it("invokes stop_mic_test when Stop Test is clicked", async () => {
    renderPanel();
    // Start
    await act(async () => {
      fireEvent.click(screen.getByText("Mic Test"));
    });
    invokeMock.mockClear();
    // Stop
    await act(async () => {
      fireEvent.click(screen.getByText("Stop Test"));
    });
    expect(invokeMock).toHaveBeenCalledWith("stop_mic_test", undefined);
  });

  it("hides VU meter after stopping mic test", async () => {
    const { container } = renderPanel();
    // Start
    await act(async () => {
      fireEvent.click(screen.getByText("Mic Test"));
    });
    // Stop
    await act(async () => {
      fireEvent.click(screen.getByText("Stop Test"));
    });
    expect(container.querySelector("[class*='vuMeter']")).toBeNull();
    expect(screen.getByText("Mic Test")).toBeTruthy();
  });

  it("does not start mic test if invoke throws", async () => {
    invokeMock.mockRejectedValueOnce(new Error("no mic"));
    renderPanel();
    await act(async () => {
      fireEvent.click(screen.getByText("Mic Test"));
    });
    // Should still show "Mic Test" (not "Stop Test")
    expect(screen.getByText("Mic Test")).toBeTruthy();
  });

  it("calls stop_mic_test on unmount while active", async () => {
    const { unmount } = renderPanel();
    await act(async () => {
      fireEvent.click(screen.getByText("Mic Test"));
    });
    invokeMock.mockClear();
    unmount();
    expect(invokeMock).toHaveBeenCalledWith("stop_mic_test", undefined);
  });
});

// ── Shortcut event filtering (regression for double-fire bug) ─────

describe("Shortcut event filtering", () => {
  it("only invokes command on Pressed, not Released", async () => {
    const registerFn = vi.mocked(register);
    registerFn.mockImplementation(async (_shortcut, handler) => {
      // Simulate the global-shortcut plugin firing both events.
      handler({ state: "Pressed", shortcut: "Ctrl+M" } as never);
      handler({ state: "Released", shortcut: "Ctrl+M" } as never);
    });

    await applyGlobalShortcut("Ctrl+M", "toggle_mute");

    // invoke should only have been called once (for Pressed).
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith("toggle_mute", undefined);
  });

  it("does not invoke command on Released event", async () => {
    const registerFn = vi.mocked(register);
    registerFn.mockImplementation(async (_shortcut, handler) => {
      handler({ state: "Released", shortcut: "Ctrl+M" } as never);
    });

    await applyGlobalShortcut("Ctrl+M", "toggle_mute");

    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("skips registration for empty shortcut string", async () => {
    const registerFn = vi.mocked(register);
    await applyGlobalShortcut("", "toggle_mute");
    expect(registerFn).not.toHaveBeenCalled();
  });
});
