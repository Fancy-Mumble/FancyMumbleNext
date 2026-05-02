/**
 * Drag-and-drop helpers for moving a user into another channel.
 *
 * Uses pointer events + a portal-mounted floating clone (similar to
 * the `ServerTabsBar` tab reorder), because HTML5 drag-and-drop is
 * unreliable inside Tauri's webview (drag ghost suppressed, the
 * `data-tauri-drag-region` attribute can swallow events).  Only the Y
 * axis follows the cursor; X is locked to the source row's left edge
 * because users are arranged vertically in the sidebar.
 *
 * Drop targets register themselves through `useChannelDropTarget`.
 * On `pointerup` we hit-test the cursor against every registered
 * target's bounding rect and invoke `move_user_to_channel` if the
 * drop landed on one.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";

const DRAG_THRESHOLD_PX = 4;

// -- Global drop-target registry ----------------------------------

interface DropRegistration {
  channelId: number;
  el: HTMLElement;
  setActive: (active: boolean) => void;
}

const registry = new Set<DropRegistration>();

function registerDropTarget(reg: DropRegistration): () => void {
  registry.add(reg);
  return () => {
    registry.delete(reg);
  };
}

function hitTest(clientX: number, clientY: number): DropRegistration | null {
  for (const reg of registry) {
    const rect = reg.el.getBoundingClientRect();
    if (
      clientX >= rect.left &&
      clientX <= rect.right &&
      clientY >= rect.top &&
      clientY <= rect.bottom
    ) {
      return reg;
    }
  }
  return null;
}

function clearAllActive(): void {
  for (const reg of registry) {
    reg.setActive(false);
  }
}

function setActiveOnly(target: DropRegistration | null): void {
  for (const reg of registry) {
    reg.setActive(reg === target);
  }
}

// -- Drop-target hook (channel rows) ------------------------------

/**
 * Register a channel as a drop target for user-move drags.
 * Returns a `ref` to attach to the wrapper element and an `active`
 * flag that flips to `true` while a user drag is hovering it.
 */
export function useChannelDropTarget(channelId: number) {
  const [active, setActive] = useState(false);
  const unregisterRef = useRef<(() => void) | null>(null);

  const ref = useCallback(
    (el: HTMLDivElement | null) => {
      // Tear down any previous registration first.
      unregisterRef.current?.();
      unregisterRef.current = null;
      if (el) {
        unregisterRef.current = registerDropTarget({ channelId, el, setActive });
      }
    },
    [channelId],
  );

  useEffect(
    () => () => {
      unregisterRef.current?.();
      unregisterRef.current = null;
    },
    [],
  );

  return { ref, active };
}

// -- Drag-source hook (user rows) ---------------------------------

interface DragState {
  pointerId: number;
  startX: number;
  startY: number;
  grabOffsetX: number;
  grabOffsetY: number;
  width: number;
  height: number;
  initialLeft: number;
  started: boolean;
  rafId: number | null;
  pendingX: number;
  pendingY: number;
}

interface FloatingState {
  width: number;
  height: number;
  initialLeft: number;
  initialTop: number;
  label: string;
  avatarUrl: string | null;
}

/** Result of `useUserDrag`. */
export interface UserDragResult {
  /** Spread on the draggable user row. */
  handlers: {
    onPointerDown: (e: React.PointerEvent<HTMLElement>) => void;
    onPointerMove: (e: React.PointerEvent<HTMLElement>) => void;
    onPointerUp: (e: React.PointerEvent<HTMLElement>) => void;
    onPointerCancel: (e: React.PointerEvent<HTMLElement>) => void;
    onClickCapture: (e: React.MouseEvent) => void;
    style: React.CSSProperties;
  };
  /** Portal-rendered floating clone (or `null` when idle). */
  overlay: React.ReactNode;
  /** True while the user is being dragged (after threshold). */
  isDragging: boolean;
}

/**
 * Make a user row draggable.  When `disabled` is true the hook returns
 * inert handlers and never starts a drag (used for self / offline /
 * mobile rows).
 */
export function useUserDrag(
  session: number,
  name: string,
  avatarUrl: string | null,
  disabled: boolean,
): UserDragResult {
  const stateRef = useRef<DragState | null>(null);
  const floatingElRef = useRef<HTMLDivElement | null>(null);
  const justDraggedRef = useRef(false);
  const [floating, setFloating] = useState<FloatingState | null>(null);

  const flush = useCallback(() => {
    const st = stateRef.current;
    if (!st) return;
    st.rafId = null;
    const el = floatingElRef.current;
    if (el) {
      // Lock X to the source row's initial left; only Y follows the
      // cursor (users are stacked vertically).
      const y = st.pendingY - st.grabOffsetY;
      el.style.transform = `translate(${st.initialLeft}px, ${y}px)`;
    }
    setActiveOnly(hitTest(st.pendingX, st.pendingY));
  }, []);

  const cleanup = useCallback(() => {
    const st = stateRef.current;
    if (st?.rafId != null) cancelAnimationFrame(st.rafId);
    stateRef.current = null;
    clearAllActive();
    setFloating(null);
  }, []);

  const onPointerDown = useCallback(
    (e: React.PointerEvent<HTMLElement>) => {
      if (disabled || e.button !== 0) return;
      // Allow nested interactive controls (volume sliders, etc.) to
      // claim their own pointer events.
      const targetEl = e.target as HTMLElement;
      if (targetEl.closest("input, [data-no-drag='true']")) return;

      const rect = e.currentTarget.getBoundingClientRect();
      stateRef.current = {
        pointerId: e.pointerId,
        startX: e.clientX,
        startY: e.clientY,
        grabOffsetX: e.clientX - rect.left,
        grabOffsetY: e.clientY - rect.top,
        width: rect.width,
        height: rect.height,
        initialLeft: rect.left,
        started: false,
        rafId: null,
        pendingX: e.clientX,
        pendingY: e.clientY,
      };
      try {
        e.currentTarget.setPointerCapture(e.pointerId);
      } catch {
        // Some webviews reject capture on disabled elements; ignore.
      }
    },
    [disabled],
  );

  const onPointerMove = useCallback(
    (e: React.PointerEvent<HTMLElement>) => {
      const st = stateRef.current;
      if (!st || st.pointerId !== e.pointerId) return;
      st.pendingX = e.clientX;
      st.pendingY = e.clientY;
      if (!st.started) {
        const dx = e.clientX - st.startX;
        const dy = e.clientY - st.startY;
        if (Math.abs(dx) < DRAG_THRESHOLD_PX && Math.abs(dy) < DRAG_THRESHOLD_PX) return;
        st.started = true;
        setFloating({
          width: st.width,
          height: st.height,
          initialLeft: st.initialLeft,
          initialTop: e.clientY - st.grabOffsetY,
          label: name,
          avatarUrl,
        });
      }
      if (st.rafId == null) {
        st.rafId = requestAnimationFrame(flush);
      }
    },
    [flush, name, avatarUrl],
  );

  const commitDrop = useCallback(
    (clientX: number, clientY: number) => {
      const target = hitTest(clientX, clientY);
      if (!target) return;
      invoke("move_user_to_channel", {
        session,
        channelId: target.channelId,
      }).catch((err: unknown) =>
        console.error("move_user_to_channel failed:", err),
      );
    },
    [session],
  );

  const onPointerUp = useCallback(
    (e: React.PointerEvent<HTMLElement>) => {
      const st = stateRef.current;
      if (!st || st.pointerId !== e.pointerId) {
        cleanup();
        return;
      }
      const wasDragging = st.started;
      try {
        if (e.currentTarget.hasPointerCapture(st.pointerId)) {
          e.currentTarget.releasePointerCapture(st.pointerId);
        }
      } catch {
        // Capture may have already been released.
      }
      if (wasDragging) {
        commitDrop(e.clientX, e.clientY);
        // Suppress the synthetic click that normally follows
        // pointerup, otherwise selecting / opening DM would fire.
        justDraggedRef.current = true;
      }
      cleanup();
    },
    [cleanup, commitDrop],
  );

  const onPointerCancel = useCallback(
    (e: React.PointerEvent<HTMLElement>) => {
      const st = stateRef.current;
      if (st) {
        try {
          if (e.currentTarget.hasPointerCapture(st.pointerId)) {
            e.currentTarget.releasePointerCapture(st.pointerId);
          }
        } catch {
          // Capture may have already been released.
        }
      }
      cleanup();
    },
    [cleanup],
  );

  const onClickCapture = useCallback((e: React.MouseEvent) => {
    if (justDraggedRef.current) {
      justDraggedRef.current = false;
      e.preventDefault();
      e.stopPropagation();
    }
  }, []);

  // Render the floating clone via portal so it can travel outside the
  // sidebar's overflow clip box.
  const overlay =
    floating != null
      ? createPortal(
          <FloatingUserClone
            elRef={floatingElRef}
            width={floating.width}
            height={floating.height}
            initialLeft={floating.initialLeft}
            initialTop={floating.initialTop}
            label={floating.label}
            avatarUrl={floating.avatarUrl}
          />,
          document.body,
        )
      : null;

  return {
    handlers: {
      onPointerDown,
      onPointerMove,
      onPointerUp,
      onPointerCancel,
      onClickCapture,
      style: floating ? { visibility: "hidden" } : {},
    },
    overlay,
    isDragging: floating != null,
  };
}

// -- Floating clone (portal child) --------------------------------

interface FloatingUserCloneProps {
  elRef: React.MutableRefObject<HTMLDivElement | null>;
  width: number;
  height: number;
  initialLeft: number;
  initialTop: number;
  label: string;
  avatarUrl: string | null;
}

function FloatingUserClone({
  elRef,
  width,
  height,
  initialLeft,
  initialTop,
  label,
  avatarUrl,
}: FloatingUserCloneProps) {
  return (
    <div
      ref={elRef}
      style={{
        position: "fixed",
        left: 0,
        top: 0,
        width,
        height,
        transform: `translate(${initialLeft}px, ${initialTop}px)`,
        pointerEvents: "none",
        zIndex: 9999,
        display: "flex",
        alignItems: "center",
        gap: 8,
        padding: "0 10px",
        borderRadius: 10,
        background: "rgba(30, 33, 40, 0.85)",
        border: "1px solid rgba(255, 255, 255, 0.18)",
        boxShadow:
          "0 8px 24px rgba(0, 0, 0, 0.45), 0 1px 0 rgba(255, 255, 255, 0.06) inset",
        backdropFilter: "blur(10px) saturate(160%)",
        WebkitBackdropFilter: "blur(10px) saturate(160%)",
        color: "#f5f6f8",
        font: "inherit",
        opacity: 0.95,
      }}
    >
      <div
        style={{
          width: 24,
          height: 24,
          borderRadius: "50%",
          background: avatarUrl ? "transparent" : "#5865f2",
          backgroundImage: avatarUrl ? `url(${avatarUrl})` : undefined,
          backgroundSize: "cover",
          backgroundPosition: "center",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          fontSize: 12,
          fontWeight: 600,
          flexShrink: 0,
        }}
      >
        {!avatarUrl && label.charAt(0).toUpperCase()}
      </div>
      <span
        style={{
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
        }}
      >
        {label}
      </span>
    </div>
  );
}
