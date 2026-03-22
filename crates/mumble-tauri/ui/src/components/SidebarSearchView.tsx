import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { SearchResult, SearchCategory, PhotoEntry } from "../types";
import styles from "./SidebarSearchView.module.css";

type SearchFilter = "all" | "messages" | "photos" | "users" | "links";

const FILTERS: { key: SearchFilter; label: string }[] = [
  { key: "all", label: "All" },
  { key: "messages", label: "Messages" },
  { key: "photos", label: "Photos" },
  { key: "users", label: "Users" },
  { key: "links", label: "Links" },
];

const CATEGORY_ORDER: SearchCategory[] = ["channel", "user", "group", "message"];
const CATEGORY_LABELS: Record<SearchCategory, string> = {
  channel: "Channels",
  user: "Users",
  group: "Group Chats",
  message: "Messages",
};

const PHOTOS_PAGE_SIZE = 20;

function dedupePhotos(photos: PhotoEntry[]): PhotoEntry[] {
  const seen = new Set<string>();
  return photos.filter((p) => {
    if (seen.has(p.src)) return false;
    seen.add(p.src);
    return true;
  });
}

interface SidebarSearchViewProps {
  readonly query: string;
  readonly channelId?: number | null;
  readonly channelName?: string;
  readonly onSelectChannel: (id: number) => void;
  readonly onSelectUser: (session: number) => void;
  readonly onSelectGroup: (id: string) => void;
}

export function SidebarSearchView({
  query,
  channelId,
  channelName,
  onSelectChannel,
  onSelectUser,
  onSelectGroup,
}: SidebarSearchViewProps) {
  const [filter, setFilter] = useState<SearchFilter>("all");
  const [results, setResults] = useState<SearchResult[]>([]);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // --- Photo grid state ---
  const [photos, setPhotos] = useState<PhotoEntry[]>([]);
  const [photosLoading, setPhotosLoading] = useState(false);
  const sentinelRef = useRef<HTMLDivElement>(null);
  const photosLoadingRef = useRef(false);
  const photosHasMoreRef = useRef(true);
  const photosOffsetRef = useRef(0);
  const photosGenRef = useRef(0);

  // Whether to show the photo grid (photos filter, no query text).
  const showPhotoGrid = filter === "photos" && !query.trim();

  // Debounced search - re-runs when query or filter changes.
  const doSearch = useCallback(
    async (q: string, f: SearchFilter) => {
      if (!q.trim()) {
        setResults([]);
        return;
      }
      try {
        const res = await invoke<SearchResult[]>("super_search", {
          query: q,
          filter: f === "all" ? null : f,
          channelId: channelId ?? null,
        });
        setResults(res);
      } catch {
        setResults([]);
      }
    },
    [channelId],
  );

  useEffect(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => doSearch(query, filter), 120);
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [query, filter, doSearch]);

  // --- Photo grid loading ---

  const loadPhotos = useCallback(async (offset: number, append: boolean, gen: number) => {
    if (photosLoadingRef.current) return;
    photosLoadingRef.current = true;
    setPhotosLoading(true);
    try {
      const batch = await invoke<PhotoEntry[]>("get_photos", {
        offset,
        limit: PHOTOS_PAGE_SIZE,
      });
      if (photosGenRef.current !== gen) return;
      const hasMore = batch.length >= PHOTOS_PAGE_SIZE;
      photosHasMoreRef.current = hasMore;
      photosOffsetRef.current = offset + batch.length;
      if (append) {
        setPhotos((prev) => {
          const seen = new Set(prev.map((p) => p.src));
          return [...prev, ...batch.filter((p) => !seen.has(p.src))];
        });
      } else {
        setPhotos(dedupePhotos(batch));
      }
    } catch {
      if (photosGenRef.current !== gen) return;
      if (!append) setPhotos([]);
      photosHasMoreRef.current = false;
    } finally {
      if (photosGenRef.current !== gen) return;
      photosLoadingRef.current = false;
      setPhotosLoading(false);
    }
  }, []);

  // Load initial page when entering photo-grid mode.
  useEffect(() => {
    if (!showPhotoGrid) return;
    const gen = ++photosGenRef.current;
    setPhotos([]);
    photosHasMoreRef.current = true;
    photosOffsetRef.current = 0;
    photosLoadingRef.current = false;
    loadPhotos(0, false, gen);
    return () => {
      // Invalidate in-flight loads when leaving the photo grid.
      photosGenRef.current++;
    };
  }, [showPhotoGrid, loadPhotos]);

  // IntersectionObserver for infinite scroll.
  //
  // Depends on `photosLoading` so the observer is recreated after each load.
  // `IntersectionObserver.observe()` fires an initial callback with the
  // current intersection state, so a fresh observer after a load effectively
  // checks "is the sentinel still visible?" without a separate useEffect.
  useEffect(() => {
    if (!showPhotoGrid) return;
    const el = sentinelRef.current;
    if (!el) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (
          entries[0]?.isIntersecting &&
          photosHasMoreRef.current &&
          !photosLoadingRef.current
        ) {
          loadPhotos(photosOffsetRef.current, true, photosGenRef.current);
        }
      },
      { threshold: 0 },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [showPhotoGrid, loadPhotos, photosLoading]);

  const selectPhoto = useCallback(
    (p: PhotoEntry) => {
      if (p.group_id) {
        onSelectGroup(p.group_id);
      } else if (p.channel_id != null) {
        onSelectChannel(p.channel_id);
      }
    },
    [onSelectChannel, onSelectGroup],
  );

  // Group results by category.
  const grouped = useMemo(() => {
    const map = new Map<SearchCategory, SearchResult[]>();
    for (const r of results) {
      const list = map.get(r.category) ?? [];
      list.push(r);
      map.set(r.category, list);
    }
    return CATEGORY_ORDER
      .filter((c) => map.has(c))
      .map((c) => ({ category: c, items: map.get(c) ?? [] }));
  }, [results]);

  const selectResult = useCallback(
    (r: SearchResult) => {
      switch (r.category) {
        case "channel":
          if (r.id != null) onSelectChannel(r.id);
          break;
        case "user":
          if (r.id != null) onSelectUser(r.id);
          break;
        case "group":
          if (r.string_id) onSelectGroup(r.string_id);
          break;
        case "message":
          if (r.string_id && !r.id) {
            onSelectGroup(r.string_id);
          } else if (r.id != null) {
            onSelectChannel(r.id);
          }
          break;
      }
    },
    [onSelectChannel, onSelectUser, onSelectGroup],
  );

  return (
    <div className={styles.searchContainer}>
      {/* Channel scope indicator */}
      {channelId != null && channelName && (
        <div className={styles.channelScope}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <line x1="4" y1="9" x2="20" y2="9" />
            <line x1="4" y1="15" x2="20" y2="15" />
            <line x1="10" y1="3" x2="8" y2="21" />
            <line x1="16" y1="3" x2="14" y2="21" />
          </svg>
          <span className={styles.channelScopeName}>{channelName}</span>
        </div>
      )}

      {/* Filter tabs */}
      <div className={styles.filterTabs}>
        {FILTERS.map((f) => (
          <button
            key={f.key}
            type="button"
            className={`${styles.filterTab} ${filter === f.key ? styles.filterTabActive : ""}`}
            onClick={() => setFilter(f.key)}
          >
            {f.label}
          </button>
        ))}
      </div>

      {/* Results / Photo Grid */}
      <div className={styles.results}>
        {showPhotoGrid ? (
          <>
            {photos.length === 0 && !photosLoading && (
              <div className={styles.empty}>
                <svg className={styles.emptyIcon} width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                  <rect x="3" y="3" width="18" height="18" rx="2" ry="2" />
                  <circle cx="8.5" cy="8.5" r="1.5" />
                  <polyline points="21 15 16 10 5 21" />
                </svg>
                <span className={styles.emptyText}>No photos yet</span>
                <span className={styles.emptyHint}>
                  Photos shared in channels will appear here
                </span>
              </div>
            )}
            {photos.length > 0 && (
              <div className={styles.photoGrid}>
                {photos.map((p, idx) => (
                  <button
                    key={`photo-${idx}`}
                    type="button"
                    className={styles.photoThumb}
                    onClick={() => selectPhoto(p)}
                    title={`${p.sender_name} ${p.context}`}
                  >
                    <img src={p.src} alt={`${p.sender_name} ${p.context}`} loading="lazy" />
                  </button>
                ))}
              </div>
            )}
            {photosLoading && (
              <div className={styles.photoLoading}>Loading...</div>
            )}
            <div ref={sentinelRef} className={styles.sentinel} />
          </>
        ) : (
          <>
            {!query.trim() && (
              <div className={styles.empty}>
                <svg className={styles.emptyIcon} width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                  <circle cx="11" cy="11" r="8" />
                  <line x1="21" y1="21" x2="16.65" y2="16.65" />
                </svg>
                <span className={styles.emptyText}>Type to search</span>
                <span className={styles.emptyHint}>
                  Search channels, users, messages, and more
                </span>
              </div>
            )}

            {query.trim() && results.length === 0 && (
              <div className={styles.empty}>
                <span className={styles.emptyText}>No results found</span>
                <span className={styles.emptyHint}>
                  Try a different search term or filter
                </span>
              </div>
            )}

            {grouped.map((group) => (
              <div key={group.category}>
                <div className={styles.categoryLabel}>
                  {CATEGORY_LABELS[group.category]}
                </div>
                {group.items.map((r, idx) => (
                  <button
                    key={`${r.category}-${r.id ?? r.string_id}-${idx}`}
                    type="button"
                    className={styles.resultItem}
                    onClick={() => selectResult(r)}
                  >
                    <ResultIcon category={r.category} />
                    <div className={styles.resultText}>
                      <span className={styles.resultTitle}>{r.title}</span>
                      {r.subtitle && (
                        <span className={styles.resultSubtitle}>{r.subtitle}</span>
                      )}
                    </div>
                  </button>
                ))}
              </div>
            ))}
          </>
        )}
      </div>
    </div>
  );
}

function ResultIcon({ category }: { readonly category: SearchCategory }) {
  switch (category) {
    case "channel":
      return (
        <div className={`${styles.resultIcon} ${styles.resultIconChannel}`}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <line x1="4" y1="9" x2="20" y2="9" />
            <line x1="4" y1="15" x2="20" y2="15" />
            <line x1="10" y1="3" x2="8" y2="21" />
            <line x1="16" y1="3" x2="14" y2="21" />
          </svg>
        </div>
      );
    case "user":
      return (
        <div className={`${styles.resultIcon} ${styles.resultIconUser}`}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" />
            <circle cx="12" cy="7" r="4" />
          </svg>
        </div>
      );
    case "group":
      return (
        <div className={`${styles.resultIcon} ${styles.resultIconGroup}`}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2" />
            <circle cx="9" cy="7" r="4" />
            <path d="M23 21v-2a4 4 0 0 0-3-3.87" />
            <path d="M16 3.13a4 4 0 0 1 0 7.75" />
          </svg>
        </div>
      );
    case "message":
      return (
        <div className={`${styles.resultIcon} ${styles.resultIconMessage}`}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
          </svg>
        </div>
      );
  }
}
