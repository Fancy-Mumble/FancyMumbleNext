import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useLocation, useNavigate } from "react-router-dom";
import { useAppStore } from "../../store";
import type { ServerId, SessionMeta } from "../../types";
import ConfirmDialog from "../elements/ConfirmDialog";
import AddServerPopover from "./AddServerPopover";
import styles from "./ServerTabsBar.module.css";

const TAB_ORDER_STORAGE_KEY = "fancy-mumble:server-tab-order";

function loadStoredOrder(): ServerId[] {
  try {
    const raw = localStorage.getItem(TAB_ORDER_STORAGE_KEY);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (Array.isArray(parsed) && parsed.every((v) => typeof v === "string")) {
      return parsed as ServerId[];
    }
  } catch {
    // Ignore corrupt entries.
  }
  return [];
}

function saveStoredOrder(order: ServerId[]): void {
  try {
    localStorage.setItem(TAB_ORDER_STORAGE_KEY, JSON.stringify(order));
  } catch {
    // Quota exceeded / private mode - ignore.
  }
}

/** Sort `sessions` according to the persisted user order; new sessions
 *  not yet in the saved order are appended in their backend order. */
function applyOrder(sessions: SessionMeta[], order: ServerId[]): SessionMeta[] {
  if (sessions.length <= 1) return sessions;
  const byId = new Map(sessions.map((s) => [s.id, s] as const));
  const seen = new Set<ServerId>();
  const ordered: SessionMeta[] = [];
  for (const id of order) {
    const meta = byId.get(id);
    if (meta && !seen.has(id)) {
      ordered.push(meta);
      seen.add(id);
    }
  }
  for (const meta of sessions) {
    if (!seen.has(meta.id)) ordered.push(meta);
  }
  return ordered;
}

function statusClass(status: SessionMeta["status"]): string {
  if (status === "connected") return styles.statusConnected;
  if (status === "connecting") return styles.statusConnecting;
  return styles.statusDisconnected;
}

function tabLabel(meta: SessionMeta): string {
  if (meta.label && meta.label.trim().length > 0) return meta.label;
  if (meta.host) return meta.host;
  return meta.username || "Server";
}

export default function ServerTabsBar() {
  const sessions = useAppStore((s) => s.sessions);
  const activeServerId = useAppStore((s) => s.activeServerId);
  const switchServer = useAppStore((s) => s.switchServer);
  const refreshSessions = useAppStore((s) => s.refreshSessions);
  const disconnectSession = useAppStore((s) => s.disconnectSession);
  const sessionUnreadTotals = useAppStore((s) => s.sessionUnreadTotals);
  const navigate = useNavigate();
  const location = useLocation();

  // While the connect page is showing alongside existing sessions we
  // render a synthetic "New connection" tab and treat it as active so
  // the user can navigate back to a real session by clicking its tab.
  const newTabActive = sessions.length > 0 && location.pathname === "/";

  const [pendingDisconnect, setPendingDisconnect] = useState<SessionMeta | null>(null);
  const [isDisconnecting, setIsDisconnecting] = useState(false);
  const [addOpen, setAddOpen] = useState(false);
  const addBtnRef = useRef<HTMLButtonElement | null>(null);

  // -- Drag-and-drop reordering (pointer-based) ----------------------
  // We use pointer events instead of HTML5 drag-and-drop because the
  // latter is unreliable inside Tauri's webview (drag ghost suppressed,
  // drag-region attribute can swallow events).  The dragged tab is
  // rendered as a portal-mounted floating clone with `position: fixed`
  // so it can travel outside the bar's `overflow: auto` clip box.
  const [tabOrder, setTabOrder] = useState<ServerId[]>(() => loadStoredOrder());
  const tabRefs = useRef<Map<ServerId, HTMLDivElement>>(new Map());
  const setTabRef = (id: ServerId) => (el: HTMLDivElement | null) => {
    if (el) tabRefs.current.set(id, el);
    else tabRefs.current.delete(id);
  };
  const floatingRef = useRef<HTMLDivElement | null>(null);
  const dragStateRef = useRef<{
    id: ServerId;
    pointerId: number;
    startX: number;
    startY: number;
    started: boolean;
    grabOffsetX: number;
    grabOffsetY: number;
    width: number;
    height: number;
    rafId: number | null;
    pendingX: number;
    pendingY: number;
  } | null>(null);
  const [draggingId, setDraggingId] = useState<ServerId | null>(null);
  const [floatingMeta, setFloatingMeta] = useState<{
    meta: SessionMeta;
    width: number;
    height: number;
    initialX: number;
    initialY: number;
  } | null>(null);
  const [dropTarget, setDropTarget] = useState<{ id: ServerId; before: boolean } | null>(null);

  const orderedSessions = useMemo(() => applyOrder(sessions, tabOrder), [sessions, tabOrder]);

  // Keep the persisted order in sync whenever sessions appear/disappear so
  // the saved list stays compact and reflects any append-on-connect order.
  useEffect(() => {
    if (sessions.length === 0) return;
    const ids = orderedSessions.map((s) => s.id);
    if (ids.length !== tabOrder.length || ids.some((id, i) => id !== tabOrder[i])) {
      setTabOrder(ids);
      saveStoredOrder(ids);
    }
  }, [sessions, orderedSessions, tabOrder]);

  const DRAG_THRESHOLD_PX = 4;

  const findTabUnderX = (clientX: number, excludeId: ServerId): { id: ServerId; before: boolean } | null => {
    for (const meta of orderedSessions) {
      if (meta.id === excludeId) continue;
      const el = tabRefs.current.get(meta.id);
      if (!el) continue;
      const rect = el.getBoundingClientRect();
      if (clientX >= rect.left && clientX <= rect.right) {
        return { id: meta.id, before: clientX < rect.left + rect.width / 2 };
      }
    }
    // Outside any tab: clamp to first/last.
    const first = orderedSessions.find((s) => s.id !== excludeId);
    const last = [...orderedSessions].reverse().find((s) => s.id !== excludeId);
    if (!first || !last) return null;
    const firstRect = tabRefs.current.get(first.id)?.getBoundingClientRect();
    const lastRect = tabRefs.current.get(last.id)?.getBoundingClientRect();
    if (firstRect && clientX < firstRect.left) return { id: first.id, before: true };
    if (lastRect && clientX > lastRect.right) return { id: last.id, before: false };
    return null;
  };

  const handlePointerDown = (e: React.PointerEvent<HTMLDivElement>, id: ServerId) => {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest(`.${styles.closeBtn}`)) return;
    const el = e.currentTarget;
    const rect = el.getBoundingClientRect();
    dragStateRef.current = {
      id,
      pointerId: e.pointerId,
      startX: e.clientX,
      startY: e.clientY,
      started: false,
      grabOffsetX: e.clientX - rect.left,
      grabOffsetY: e.clientY - rect.top,
      width: rect.width,
      height: rect.height,
      rafId: null,
      pendingX: e.clientX,
      pendingY: e.clientY,
    };
    el.setPointerCapture(e.pointerId);
  };

  const beginDrag = (id: ServerId, e: React.PointerEvent<HTMLDivElement>) => {
    const meta = orderedSessions.find((s) => s.id === id);
    const st = dragStateRef.current;
    if (!meta || !st) return;
    setDraggingId(id);
    setFloatingMeta({
      meta,
      width: st.width,
      height: st.height,
      initialX: e.clientX - st.grabOffsetX,
      initialY: st.startY - st.grabOffsetY,
    });
  };

  const flushFloatingPosition = () => {
    const st = dragStateRef.current;
    if (!st) return;
    st.rafId = null;
    const el = floatingRef.current;
    if (el) {
      // Lock Y to the tab's original top so the clone stays on the
      // tab bar; only X follows the cursor.
      const x = st.pendingX - st.grabOffsetX;
      const y = st.startY - st.grabOffsetY;
      el.style.transform = `translate(${x}px, ${y}px)`;
    }
    const target = findTabUnderX(st.pendingX, st.id);
    setDropTarget((prev) => {
      if (!target) return prev === null ? prev : null;
      if (prev?.id === target.id && prev.before === target.before) return prev;
      return target;
    });
  };

  const handlePointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const st = dragStateRef.current;
    if (!st || st.pointerId !== e.pointerId) return;
    const dx = e.clientX - st.startX;
    const dy = e.clientY - st.startY;
    if (!st.started) {
      if (Math.abs(dx) < DRAG_THRESHOLD_PX && Math.abs(dy) < DRAG_THRESHOLD_PX) return;
      st.started = true;
      beginDrag(st.id, e);
    }
    st.pendingX = e.clientX;
    st.pendingY = e.clientY;
    if (st.rafId === null) {
      st.rafId = requestAnimationFrame(flushFloatingPosition);
    }
  };

  const finishDrag = (commit: boolean) => {
    const st = dragStateRef.current;
    dragStateRef.current = null;
    if (st?.rafId !== null && st?.rafId !== undefined) {
      cancelAnimationFrame(st.rafId);
    }
    if (st && commit && st.started) {
      const dropAt = dropTarget;
      if (dropAt && dropAt.id !== st.id) {
        const current = orderedSessions.map((s) => s.id);
        const fromIdx = current.indexOf(st.id);
        if (fromIdx !== -1) {
          const next = [...current];
          next.splice(fromIdx, 1);
          let insertIdx = next.indexOf(dropAt.id);
          if (!dropAt.before) insertIdx += 1;
          next.splice(insertIdx, 0, st.id);
          setTabOrder(next);
          saveStoredOrder(next);
        }
      }
    }
    setDraggingId(null);
    setFloatingMeta(null);
    setDropTarget(null);
    return st;
  };

  const handlePointerUp = (e: React.PointerEvent<HTMLDivElement>, id: ServerId) => {
    const st = dragStateRef.current;
    const wasDragging = st?.started === true;
    const tabEl = e.currentTarget;
    if (st && tabEl.hasPointerCapture(st.pointerId)) {
      tabEl.releasePointerCapture(st.pointerId);
    }
    finishDrag(true);
    if (!wasDragging) {
      handleSwitch(id);
    }
  };

  const handlePointerCancel = (e: React.PointerEvent<HTMLDivElement>) => {
    const st = dragStateRef.current;
    if (st && e.currentTarget.hasPointerCapture(st.pointerId)) {
      e.currentTarget.releasePointerCapture(st.pointerId);
    }
    finishDrag(false);
  };


  const handleSwitch = (id: ServerId) => {
    // If we're on the connect page (new-tab dummy active), navigate
    // back to the chat view; switchServer is a no-op when id matches
    // the current activeServerId so this also handles the
    // single-session case correctly.
    if (location.pathname !== "/chat") {
      navigate("/chat");
    }
    if (id === activeServerId) return;
    void switchServer(id);
  };

  const handleDismissNewTab = (e?: React.MouseEvent) => {
    e?.stopPropagation();
    setAddOpen(false);
    navigate("/chat");
  };

  const handleCloseClick = (e: React.MouseEvent, meta: SessionMeta) => {
    e.stopPropagation();
    setPendingDisconnect(meta);
  };

  const handleConfirmDisconnect = async () => {
    if (!pendingDisconnect) return;
    setIsDisconnecting(true);
    try {
      await disconnectSession(pendingDisconnect.id);
    } finally {
      setIsDisconnecting(false);
      setPendingDisconnect(null);
      await refreshSessions();
    }
  };

  const handleCancelDisconnect = () => {
    setPendingDisconnect(null);
  };

  const handleAddClick = () => {
    // When there are no saved sessions yet, fall back to the full
    // connect page; otherwise toggle the dropdown.
    if (sessions.length === 0) {
      navigate("/");
      return;
    }
    setAddOpen((open) => !open);
  };

  const renderAddButton = () => (
    <button
      ref={addBtnRef}
      type="button"
      className={styles.addBtn}
      onClick={handleAddClick}
      aria-label="Connect to a server"
      aria-expanded={addOpen}
      aria-haspopup="dialog"
      title="Connect to a server"
    >
      +
    </button>
  );

  const popover = addOpen ? (
    <AddServerPopover anchor={addBtnRef.current} onClose={() => setAddOpen(false)} />
  ) : null;

  if (sessions.length === 0) {
    return (
      <>
        <div className={styles.bar} data-tauri-drag-region>
          {renderAddButton()}
        </div>
        {popover}
      </>
    );
  }

  return (
    <>
      <div className={styles.bar} role="tablist" aria-label="Connected servers" data-tauri-drag-region>
        {orderedSessions.map((meta) => {
          const isActive = !newTabActive && meta.id === activeServerId;
          const unreadTotal = isActive ? 0 : (sessionUnreadTotals[meta.id] ?? 0);
          const isDragging = draggingId === meta.id;
          const dropClass =
            dropTarget?.id === meta.id && draggingId && draggingId !== meta.id
              ? (dropTarget.before ? styles.tabDropBefore : styles.tabDropAfter)
              : "";
          const style: React.CSSProperties = isDragging
            ? { visibility: "hidden" }
            : {};
          return (
            <div
              key={meta.id}
              ref={setTabRef(meta.id)}
              role="tab"
              tabIndex={0}
              aria-selected={isActive}
              data-tauri-drag-region="false"
              className={`${styles.tab} ${isActive ? styles.tabActive : ""} ${dropClass}`.trim()}
              style={style}
              onPointerDown={(e) => handlePointerDown(e, meta.id)}
              onPointerMove={handlePointerMove}
              onPointerUp={(e) => handlePointerUp(e, meta.id)}
              onPointerCancel={handlePointerCancel}
              onMouseDown={(e) => {
                // Prevent Tauri's drag-region from claiming the mouse
                // before our pointer handlers can run.
                e.stopPropagation();
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  handleSwitch(meta.id);
                }
              }}
              title={`${meta.username}@${meta.host}:${meta.port}`}
            >
              <span className={`${styles.statusDot} ${statusClass(meta.status)}`} />
              <span className={styles.label}>{tabLabel(meta)}</span>
              {unreadTotal > 0 && (
                <span className={styles.unreadBadge} aria-label={`${unreadTotal} unread`}>
                  {unreadTotal > 99 ? "99+" : unreadTotal}
                </span>
              )}
              <button
                type="button"
                className={styles.closeBtn}
                onClick={(e) => handleCloseClick(e, meta)}
                aria-label={`Disconnect from ${tabLabel(meta)}`}
                title="Disconnect"
              >
                x
              </button>
            </div>
          );
        })}
        {newTabActive && (
          <div
            role="tab"
            aria-selected
            tabIndex={0}
            className={`${styles.tab} ${styles.tabActive} ${styles.newTab}`}
            title="New connection"
          >
            <span className={`${styles.statusDot} ${styles.statusNew}`} />
            <span className={styles.label}>New connection</span>
            <button
              type="button"
              className={styles.closeBtn}
              onClick={handleDismissNewTab}
              aria-label="Dismiss new connection tab"
              title="Dismiss"
            >
              x
            </button>
          </div>
        )}
        {renderAddButton()}
      </div>

      {popover}

      {floatingMeta && createPortal(
        <div
          ref={floatingRef}
          className={`${styles.tab} ${styles.tabFloating}`}
          style={{
            position: "fixed",
            left: 0,
            top: 0,
            width: floatingMeta.width,
            height: floatingMeta.height,
            transform: `translate(${floatingMeta.initialX}px, ${floatingMeta.initialY}px)`,
            pointerEvents: "none",
            zIndex: 9999,
          }}
        >
          <span className={`${styles.statusDot} ${statusClass(floatingMeta.meta.status)}`} />
          <span className={styles.label}>{tabLabel(floatingMeta.meta)}</span>
        </div>,
        document.body,
      )}

      {pendingDisconnect && (
        <ConfirmDialog
          title="Disconnect from server"
          body={`Disconnect from ${tabLabel(pendingDisconnect)}?`}
          confirmLabel="Disconnect"
          cancelLabel="Cancel"
          danger
          isConfirming={isDisconnecting}
          onConfirm={() => void handleConfirmDisconnect()}
          onCancel={handleCancelDisconnect}
        />
      )}
    </>
  );
}
