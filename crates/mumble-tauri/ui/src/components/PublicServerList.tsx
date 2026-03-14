import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { PublicServer, ServerPingResult } from "../types";
import styles from "./PublicServerList.module.css";

type SortKey = "country" | "name" | "ping";
type SortDir = "asc" | "desc";

interface Props {
  onConnect: (host: string, port: number) => void;
  onBack: () => void;
  disabled?: boolean;
}

/** Module-level cache: "host:port" -> last ping epoch-ms. */
const publicPingCache = new Map<string, number>();

/** Country code to flag emoji. */
function countryFlag(code: string): string {
  if (code.length !== 2) return "";
  const offset = 0x1f1e6 - 65; // 'A' = 65
  return String.fromCodePoint(
    (code.codePointAt(0) ?? 65) + offset,
    (code.codePointAt(1) ?? 65) + offset,
  );
}

/** Simple fuzzy matching: all query characters must appear in order. */
function fuzzyMatch(query: string, text: string): boolean {
  const lower = text.toLowerCase();
  let qi = 0;
  for (let i = 0; i < lower.length && qi < query.length; i++) {
    if (lower[i] === query[qi]) qi++;
  }
  return qi === query.length;
}

export default function PublicServerList({
  onConnect,
  onBack,
  disabled,
}: Readonly<Props>) {
  const [consented, setConsented] = useState(false);
  const [servers, setServers] = useState<PublicServer[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [sortKey, setSortKey] = useState<SortKey>("name");
  const [sortDir, setSortDir] = useState<SortDir>("asc");
  const [pings, setPings] = useState<Record<string, ServerPingResult>>({});

  // Fetch list once consent is given
  useEffect(() => {
    if (!consented) return;
    setLoading(true);
    setError(null);

    invoke<PublicServer[]>("fetch_public_servers")
      .then((list) => {
        console.log(`[PublicServerList] Fetched ${list.length} servers`);
        setServers(list);
      })
      .catch((e) => {
        console.error("[PublicServerList] fetch failed:", e);
        setError(String(e));
      })
      .finally(() => setLoading(false));
  }, [consented]);

  // Ping visible servers (throttled)
  const pingServers = useCallback((list: PublicServer[]) => {
    const THROTTLE_MS = 60_000;
    const now = Date.now();

    for (const s of list) {
      const key = `${s.ip}:${s.port}`;
      const last = publicPingCache.get(key);
      if (last !== undefined && now - last < THROTTLE_MS) continue;
      publicPingCache.set(key, now);

      invoke<ServerPingResult>("ping_server", { host: s.ip, port: s.port })
        .then((result) => setPings((prev) => ({ ...prev, [key]: result })))
        .catch(() =>
          setPings((prev) => ({
            ...prev,
            [key]: { online: false, latency_ms: null },
          })),
        );
    }
  }, []);

  // Ping after fetch completes
  useEffect(() => {
    if (servers.length > 0) pingServers(servers);
  }, [servers, pingServers]);

  // Sorting toggle
  const handleSort = (key: SortKey) => {
    if (sortKey === key) {
      setSortDir((d) => (d === "asc" ? "desc" : "asc"));
    } else {
      setSortKey(key);
      setSortDir("asc");
    }
  };

  const sortIndicator = (key: SortKey) => {
    if (sortKey !== key) return null;
    return (
      <span className={styles.sortIndicator}>
        {sortDir === "asc" ? "\u25B2" : "\u25BC"}
      </span>
    );
  };

  // Filter + sort
  const displayed = useMemo(() => {
    const query = search.toLowerCase().trim();
    let list = servers;

    if (query) {
      list = list.filter(
        (s) =>
          fuzzyMatch(query, s.name) ||
          fuzzyMatch(query, s.country) ||
          fuzzyMatch(query, s.region) ||
          fuzzyMatch(query, s.ip),
      );
    }

    const sorted = [...list];
    sorted.sort((a, b) => {
      let cmp = 0;
      if (sortKey === "country") {
        cmp = a.country.localeCompare(b.country);
      } else if (sortKey === "name") {
        cmp = a.name.localeCompare(b.name);
      } else if (sortKey === "ping") {
        const pa = pings[`${a.ip}:${a.port}`]?.latency_ms ?? 9999;
        const pb = pings[`${b.ip}:${b.port}`]?.latency_ms ?? 9999;
        cmp = pa - pb;
      }
      return sortDir === "asc" ? cmp : -cmp;
    });

    return sorted;
  }, [servers, search, sortKey, sortDir, pings]);

  // ── Consent gate ──────────────────────────────────────────────
  if (!consented) {
    return (
      <div className={styles.container}>
        <div className={styles.header}>
          <span className={styles.heading}>Public Servers</span>
          <button
            className={styles.backLink}
            onClick={onBack}
            type="button"
          >
            Saved servers
          </button>
        </div>

        <div className={styles.consent}>
          <p className={styles.consentText}>
            The public server list is fetched from the official Mumble
            directory. When you connect to a public server, your IP address
            will be visible to that server&apos;s operator.
          </p>
          <div className={styles.consentWarning}>
            <span className={styles.consentWarningIcon}>&#x26A0;&#xFE0F;</span>
            <span>
              Loading this list makes a request to{" "}
              <strong>publist.mumble.info</strong> and pinging servers reveals
              your IP address to each server. Only continue if you understand
              and accept this.
            </span>
          </div>
          <button
            className={styles.consentButton}
            onClick={() => setConsented(true)}
            type="button"
          >
            I understand, show servers
          </button>
        </div>
      </div>
    );
  }

  // ── Main list view ────────────────────────────────────────────
  return (
    <div className={styles.container}>
      <div className={styles.header}>
        <span className={styles.heading}>Public Servers</span>
        <button
          className={styles.backLink}
          onClick={onBack}
          type="button"
        >
          Saved servers
        </button>
      </div>

      {/* Search bar */}
      <div className={styles.searchBar}>
        <span className={styles.searchIcon}>&#128269;</span>
        <input
          className={styles.searchInput}
          type="text"
          placeholder="Search servers..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>

      {/* Loading / error / table */}
      {loading && (
        <div className={styles.statusRow}>
          <span className={styles.spinner} />{" "}
          Loading public servers...
        </div>
      )}

      {error && (
        <div className={styles.statusRow}>
          Failed to load: {error}
        </div>
      )}

      {!loading && !error && servers.length > 0 && (
        <div className={styles.tableWrap}>
          <table className={styles.table}>
            <thead>
              <tr>
                <th onClick={() => handleSort("country")}>
                  Country{sortIndicator("country")}
                </th>
                <th onClick={() => handleSort("name")}>
                  Server{sortIndicator("name")}
                </th>
                <th onClick={() => handleSort("ping")}>
                  Ping{sortIndicator("ping")}
                </th>
              </tr>
            </thead>
            <tbody>
              {displayed.map((s) => {
                const key = `${s.ip}:${s.port}`;
                const ping = pings[key];
                return (
                  <tr
                    key={key}
                    onClick={() => !disabled && onConnect(s.ip, s.port)}
                  >
                    <td>
                      <span className={styles.countryCell}>
                        <span className={styles.flag}>
                          {countryFlag(s.country_code)}
                        </span>
                        {s.country}
                      </span>
                    </td>
                    <td title={s.name}>{s.name}</td>
                    <td>
                      <PingCell ping={ping} />
                    </td>
                  </tr>
                );
              })}
              {displayed.length === 0 && (
                <tr>
                  <td colSpan={3} className={styles.statusRow}>
                    No servers match your search.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      )}

      {!loading && !error && servers.length === 0 && (
        <div className={styles.statusRow}>
          No public servers found.
        </div>
      )}
    </div>
  );
}

function PingCell({ ping }: Readonly<{ ping?: ServerPingResult }>) {
  if (!ping) {
    return <span className={styles.pingNa}>...</span>;
  }
  if (!ping.online || ping.latency_ms == null) {
    return <span className={styles.pingNa}>N/A</span>;
  }
  const ms = ping.latency_ms;
  let cls = styles.pingGood;
  if (ms >= 70) cls = styles.pingPoor;
  else if (ms >= 30) cls = styles.pingOkay;

  return (
    <span className={`${styles.pingValue} ${cls}`}>
      {ms} ms
    </span>
  );
}
