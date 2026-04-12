import { useState, useEffect, useRef, useCallback } from "react";

export type Broadcaster = { session: number; name: string };

export type DragTarget = number | "primary" | null;

const DRAG_THRESHOLD_PX = 5;

export interface PointerDragItemProps {
  readonly isDragOver: boolean;
  readonly onItemPointerDown: (e: React.PointerEvent, session: number) => void;
  readonly onItemClick: (session: number) => void;
}

export interface PointerDragHandlers {
  readonly onItemPointerDown: (e: React.PointerEvent, session: number) => void;
  readonly onItemClick: (session: number) => void;
}

export function useBroadcasterOrder(broadcasters: Broadcaster[]) {
  const [order, setOrder] = useState<number[]>(() => broadcasters.map((b) => b.session));

  useEffect(() => {
    setOrder((prev) => {
      const incoming = new Set(broadcasters.map((b) => b.session));
      const kept = prev.filter((s) => incoming.has(s));
      const added = broadcasters.map((b) => b.session).filter((s) => !kept.includes(s));
      return [...kept, ...added];
    });
  }, [broadcasters]);

  const orderedList = order
    .map((s) => broadcasters.find((b) => b.session === s))
    .filter((b): b is Broadcaster => b !== undefined);

  const reorder = useCallback((fromSession: number, toSession: number) => {
    setOrder((prev) => {
      const next = [...prev];
      const fromIdx = next.indexOf(fromSession);
      const toIdx = next.indexOf(toSession);
      if (fromIdx === -1 || toIdx === -1) return prev;
      next.splice(fromIdx, 1);
      next.splice(toIdx, 0, fromSession);
      return next;
    });
  }, []);

  return { orderedList, reorder };
}

export function useDragStream(onWatch: (session: number) => void, reorder: (from: number, to: number) => void) {
  const dragSessionRef = useRef<number | null>(null);
  const dragOverTargetRef = useRef<DragTarget>(null);
  const dragStartPos = useRef<{ x: number; y: number } | null>(null);
  const wasDragRef = useRef(false);
  // Latest-value refs so the closures created inside onItemPointerDown always
  // call the current onWatch/reorder even if they changed between renders.
  const onWatchRef = useRef(onWatch);
  const reorderRef = useRef(reorder);
  onWatchRef.current = onWatch;
  reorderRef.current = reorder;

  const [dragOverTarget, setDragOverTargetState] = useState<DragTarget>(null);

  // Suppresses the synthetic click the browser fires on the source element
  // after a drag gesture.  wasDragRef is set by the pointer handlers and
  // cleared here so only genuine taps call onWatch.
  const onItemClick = useCallback((session: number) => {
    if (wasDragRef.current) {
      wasDragRef.current = false;
      return;
    }
    onWatchRef.current(session);
  }, []);

  // Listeners are attached directly inside the handler (not via useEffect) so
  // they are guaranteed to be active for the entire pointer gesture.  Using
  // useEffect caused a render-cycle gap: pointermove/pointerup could fire
  // before the effect ran, leaving wasDragRef unset and causing the subsequent
  // click event to be mistaken for a tap.
  const onItemPointerDown = useCallback((e: React.PointerEvent, session: number) => {
    dragStartPos.current = { x: e.clientX, y: e.clientY };
    wasDragRef.current = false;
    dragSessionRef.current = session;

    // Closures reference each other via let declarations so cleanup can remove
    // all three listeners using the exact same function objects that were added.
    let moveFn: (e: PointerEvent) => void;
    let upFn: () => void;
    let cancelFn: () => void;

    const cleanup = () => {
      document.removeEventListener("pointermove", moveFn);
      document.removeEventListener("pointerup", upFn);
      document.removeEventListener("pointercancel", cancelFn);
      dragStartPos.current = null;
      dragOverTargetRef.current = null;
      setDragOverTargetState(null);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };

    moveFn = (ev: PointerEvent) => {
      if (dragSessionRef.current === null) return;
      if (!wasDragRef.current && dragStartPos.current) {
        const dx = Math.abs(ev.clientX - dragStartPos.current.x);
        const dy = Math.abs(ev.clientY - dragStartPos.current.y);
        if (dx > DRAG_THRESHOLD_PX || dy > DRAG_THRESHOLD_PX) {
          wasDragRef.current = true;
        }
      }
      const els = document.elementsFromPoint(ev.clientX, ev.clientY);
      const dropEl = els.find((el) => el.hasAttribute("data-drop-zone"));
      const zone = dropEl?.getAttribute("data-drop-zone") ?? null;
      const newTarget: DragTarget =
        zone === "primary" ? "primary"
        : zone !== null ? (isNaN(Number(zone)) ? null : Number(zone))
        : null;
      if (newTarget !== dragOverTargetRef.current) {
        dragOverTargetRef.current = newTarget;
        setDragOverTargetState(newTarget);
      }
    };

    upFn = () => {
      const from = dragSessionRef.current;
      const target = dragOverTargetRef.current;
      const wasDrag = wasDragRef.current;
      dragSessionRef.current = null;
      cleanup();
      // wasDragRef intentionally left set so onItemClick can read it to
      // suppress the synthetic click the browser fires after pointerup.
      if (from === null || !wasDrag) return;
      if (target === "primary") onWatchRef.current(from);
      else if (typeof target === "number" && target !== from) reorderRef.current(from, target);
    };

    cancelFn = () => {
      dragSessionRef.current = null;
      wasDragRef.current = false;
      cleanup();
    };

    document.addEventListener("pointermove", moveFn);
    document.addEventListener("pointerup", upFn);
    document.addEventListener("pointercancel", cancelFn);
    document.body.style.cursor = "grabbing";
    document.body.style.userSelect = "none";
  }, []);

  const dragHandlers: PointerDragHandlers = { onItemPointerDown, onItemClick };
  return { dragOverTarget, dragHandlers };
}
