import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { useNavigate } from "react-router-dom";
import { useAppStore } from "../../store";
import { getSavedServers, getServerPassword } from "../../serverStorage";
import type { SavedServer, ServerPingResult } from "../../types";
import styles from "./AddServerPopover.module.css";

interface Props {
  /** The button (or other element) the popover should anchor under. */
  anchor: HTMLElement | null;
  /** Called when the popover requests to close (outside-click, Esc, or
   *  after a connect was issued). */
  onClose: () => void;
}

/** Simple per-render ping cache to avoid re-pinging on every open within
 *  a short window. */
const POPOVER_PING_TTL_MS = 60_000;
const popoverPingCache = new Map<string, { at: number; result: ServerPingResult }>();

function pingClass(ping?: ServerPingResult): string {
  if (!ping) return styles.dotProbing;
  if (!ping.online) return styles.dotOffline;
  const ms = ping.latency_ms ?? 0;
  if (ms < 30) return styles.dotGreat;
  if (ms < 70) return styles.dotOkay;
  return styles.dotPoor;
}

function pingTitle(ping?: ServerPingResult): string {
  if (!ping) return "Checking...";
  if (!ping.online) return "Offline";
  const ms = ping.latency_ms ?? 0;
  return `${ms} ms`;
}

export default function AddServerPopover({ anchor, onClose }: Readonly<Props>) {
  const navigate = useNavigate();
  const connect = useAppStore((s) => s.connect);
  const sessions = useAppStore((s) => s.sessions);

  const [servers, setServers] = useState<SavedServer[] | null>(null);
  const [pings, setPings] = useState<Record<string, ServerPingResult>>({});
  const [query, setQuery] = useState("");
  const popoverRef = useRef<HTMLDivElement | null>(null);
  const searchInputRef = useRef<HTMLInputElement | null>(null);
  const [pos, setPos] = useState<{ top: number; right: number } | null>(null);

  // Position the popover under the anchor element using fixed coords so
  // overflow on the parent tab bar doesn't clip it.
  useLayoutEffect(() => {
    if (!anchor) return;
    const update = () => {
      const rect = anchor.getBoundingClientRect();
      setPos({
        top: rect.bottom + 6,
        right: Math.max(8, window.innerWidth - rect.right),
      });
    };
    update();
    window.addEventListener("resize", update);
    window.addEventListener("scroll", update, true);
    return () => {
      window.removeEventListener("resize", update);
      window.removeEventListener("scroll", update, true);
    };
  }, [anchor]);

  // Load saved servers on mount.
  useEffect(() => {
    let cancelled = false;
    void getSavedServers().then((list) => {
      if (cancelled) return;
      setServers(list);
      // Seed cached pings.
      const seeded: Record<string, ServerPingResult> = {};
      const now = Date.now();
      for (const s of list) {
        const key = `${s.host}:${s.port}`;
        const hit = popoverPingCache.get(key);
        if (hit && now - hit.at < POPOVER_PING_TTL_MS) {
          seeded[s.id] = hit.result;
        }
      }
      setPings(seeded);
      // Fire fresh pings for the rest.
      for (const s of list) {
        const key = `${s.host}:${s.port}`;
        const hit = popoverPingCache.get(key);
        if (hit && now - hit.at < POPOVER_PING_TTL_MS) continue;
        invoke<ServerPingResult>("ping_server", { host: s.host, port: s.port })
          .then((result) => {
            popoverPingCache.set(key, { at: Date.now(), result });
            if (!cancelled) setPings((prev) => ({ ...prev, [s.id]: result }));
          })
          .catch(() => {
            const result: ServerPingResult = {
              online: false,
              latency_ms: null,
              user_count: null,
              max_user_count: null,
              server_version: null,
            };
            popoverPingCache.set(key, { at: Date.now(), result });
            if (!cancelled) setPings((prev) => ({ ...prev, [s.id]: result }));
          });
      }
    });
    return () => {
      cancelled = true;
    };
  }, []);

  // Outside-click + Esc to close.
  useEffect(() => {
    const onPointer = (e: PointerEvent) => {
      const root = popoverRef.current;
      if (!root) return;
      const target = e.target instanceof Node ? e.target : null;
      if (target && root.contains(target)) return;
      // Don't close when clicking the anchor itself; let the anchor's
      // onClick toggle decide.
      if (target && anchor && anchor.contains(target)) return;
      onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("pointerdown", onPointer);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("pointerdown", onPointer);
      window.removeEventListener("keydown", onKey);
    };
  }, [anchor, onClose]);

  const handleQuickConnect = useCallback(
    async (server: SavedServer) => {
      onClose();
      const storedPw = await getServerPassword(server.id);
      await connect(server.host, server.port, server.username, server.cert_label, storedPw);
    },
    [connect, onClose],
  );

  const handleAddNew = () => {
    onClose();
    navigate("/");
  };

  // Sessions keyed for quick lookup so we can mark already-connected entries.
  const connectedKeys = new Set(
    sessions.map((s) => `${s.host}:${s.port}:${s.username}`.toLowerCase()),
  );

  const filteredServers = (() => {
    if (servers === null) return null;
    const q = query.trim().toLowerCase();
    if (!q) return servers;
    return servers.filter((s) => {
      const haystack = `${s.label ?? ""} ${s.host} ${s.username} ${s.port}`.toLowerCase();
      return haystack.includes(q);
    });
  })();

  if (!pos) return null;

  return createPortal(
    <div
      ref={popoverRef}
      className={styles.popover}
      style={{ top: pos.top, right: pos.right }}
      role="dialog"
      aria-label="Connect to a server"
    >
      <div className={styles.header}>
        <span className={styles.title}>Connect to a server</span>
        <button
          type="button"
          className={styles.headerBtn}
          onClick={handleAddNew}
          title="Add a new server"
        >
          + New
        </button>
      </div>

      {servers === null && <div className={styles.empty}>Loading...</div>}

      {servers !== null && servers.length === 0 && (
        <div className={styles.empty}>
          <p>No saved servers yet.</p>
          <button type="button" className={styles.primaryBtn} onClick={handleAddNew}>
            Add your first server
          </button>
        </div>
      )}

      {servers !== null && servers.length > 0 && (
        <>
          <div className={styles.searchRow}>
            <input
              ref={searchInputRef}
              type="text"
              className={styles.searchInput}
              placeholder="Search servers..."
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              autoFocus
            />
          </div>
          {filteredServers && filteredServers.length === 0 ? (
            <div className={styles.empty}>No matches</div>
          ) : (
            <ul className={styles.list} role="list">
              {filteredServers?.map((s) => {
            const key = `${s.host}:${s.port}:${s.username}`.toLowerCase();
            const alreadyConnected = connectedKeys.has(key);
            const ping = pings[s.id];
            return (
              <li key={s.id}>
                <button
                  type="button"
                  className={styles.item}
                  disabled={alreadyConnected}
                  onClick={() => void handleQuickConnect(s)}
                  title={alreadyConnected ? "Already connected" : `${s.username}@${s.host}:${s.port}`}
                >
                  <span className={`${styles.pingDot} ${pingClass(ping)}`} title={pingTitle(ping)} />
                  <span className={styles.itemBody}>
                    <span className={styles.itemLabel}>{s.label || s.host}</span>
                    <span className={styles.itemMeta}>
                      {s.username}
                      {alreadyConnected && <span className={styles.connectedTag}>Connected</span>}
                    </span>
                  </span>
                </button>
              </li>
            );
          })}
            </ul>
          )}
        </>
      )}
    </div>,
    document.body,
  );
}
