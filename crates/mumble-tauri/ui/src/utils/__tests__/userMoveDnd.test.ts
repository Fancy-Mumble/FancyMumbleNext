import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import type { PointerEvent as ReactPointerEvent } from "react";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invokeMock(...args) }));

import { useChannelDropTarget, useUserDrag } from "../userMoveDnd";

function attach(el: HTMLElement, rect: { x: number; y: number; w: number; h: number }) {
  el.getBoundingClientRect = () => ({
    left: rect.x,
    top: rect.y,
    right: rect.x + rect.w,
    bottom: rect.y + rect.h,
    width: rect.w,
    height: rect.h,
    x: rect.x,
    y: rect.y,
    toJSON: () => ({}),
  });
}

function makePointerEvent(
  init: { clientX: number; clientY: number; pointerId?: number; button?: number },
  currentTarget: HTMLElement,
): ReactPointerEvent<HTMLElement> {
  return {
    clientX: init.clientX,
    clientY: init.clientY,
    pointerId: init.pointerId ?? 1,
    button: init.button ?? 0,
    currentTarget,
    target: currentTarget,
    setPointerCapture: vi.fn(),
    releasePointerCapture: vi.fn(),
    hasPointerCapture: vi.fn(() => true),
    preventDefault: vi.fn(),
    stopPropagation: vi.fn(),
  } as unknown as ReactPointerEvent<HTMLElement>;
}

describe("userMoveDnd (pointer-event drag)", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);
  });

  it("useChannelDropTarget exposes a stable ref and inactive state", () => {
    const { result } = renderHook(() => useChannelDropTarget(42));
    expect(typeof result.current.ref).toBe("function");
    expect(result.current.active).toBe(false);
  });

  it("useUserDrag is inert when disabled", () => {
    const { result } = renderHook(() => useUserDrag(7, "alice", null, true));
    const sourceEl = document.createElement("div");
    attach(sourceEl, { x: 0, y: 0, w: 200, h: 30 });
    act(() => {
      result.current.handlers.onPointerDown(makePointerEvent({ clientX: 5, clientY: 5 }, sourceEl));
      result.current.handlers.onPointerMove(makePointerEvent({ clientX: 100, clientY: 100 }, sourceEl));
      result.current.handlers.onPointerUp(makePointerEvent({ clientX: 100, clientY: 100 }, sourceEl));
    });
    expect(result.current.isDragging).toBe(false);
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("commits move_user_to_channel on drop over a registered channel", () => {
    const dropEl = document.createElement("div");
    attach(dropEl, { x: 0, y: 100, w: 300, h: 60 });
    const { result: drop } = renderHook(() => useChannelDropTarget(99));
    act(() => drop.current.ref(dropEl as unknown as HTMLDivElement));

    const srcEl = document.createElement("div");
    attach(srcEl, { x: 0, y: 0, w: 300, h: 30 });
    const { result: drag } = renderHook(() => useUserDrag(7, "alice", null, false));

    act(() => {
      drag.current.handlers.onPointerDown(makePointerEvent({ clientX: 10, clientY: 10 }, srcEl));
      drag.current.handlers.onPointerMove(makePointerEvent({ clientX: 10, clientY: 130 }, srcEl));
      drag.current.handlers.onPointerUp(makePointerEvent({ clientX: 10, clientY: 130 }, srcEl));
    });

    expect(invokeMock).toHaveBeenCalledWith("move_user_to_channel", {
      session: 7,
      channelId: 99,
    });
  });

  it("does NOT commit when drop lands outside any registered channel", () => {
    const dropEl = document.createElement("div");
    attach(dropEl, { x: 0, y: 100, w: 300, h: 60 });
    const { result: drop } = renderHook(() => useChannelDropTarget(99));
    act(() => drop.current.ref(dropEl as unknown as HTMLDivElement));

    const srcEl = document.createElement("div");
    attach(srcEl, { x: 0, y: 0, w: 300, h: 30 });
    const { result: drag } = renderHook(() => useUserDrag(7, "alice", null, false));

    act(() => {
      drag.current.handlers.onPointerDown(makePointerEvent({ clientX: 10, clientY: 10 }, srcEl));
      drag.current.handlers.onPointerMove(makePointerEvent({ clientX: 10, clientY: 500 }, srcEl));
      drag.current.handlers.onPointerUp(makePointerEvent({ clientX: 10, clientY: 500 }, srcEl));
    });

    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("does not start a drag below the threshold", () => {
    const dropEl = document.createElement("div");
    attach(dropEl, { x: 0, y: 100, w: 300, h: 60 });
    const { result: drop } = renderHook(() => useChannelDropTarget(99));
    act(() => drop.current.ref(dropEl as unknown as HTMLDivElement));

    const srcEl = document.createElement("div");
    attach(srcEl, { x: 0, y: 0, w: 300, h: 30 });
    const { result: drag } = renderHook(() => useUserDrag(7, "alice", null, false));

    act(() => {
      drag.current.handlers.onPointerDown(makePointerEvent({ clientX: 10, clientY: 10 }, srcEl));
      drag.current.handlers.onPointerMove(makePointerEvent({ clientX: 12, clientY: 12 }, srcEl));
      drag.current.handlers.onPointerUp(makePointerEvent({ clientX: 12, clientY: 12 }, srcEl));
    });

    expect(drag.current.isDragging).toBe(false);
    expect(invokeMock).not.toHaveBeenCalled();
  });
});
