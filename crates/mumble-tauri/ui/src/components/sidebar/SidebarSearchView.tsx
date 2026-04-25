import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { SearchResult, SearchCategory, PhotoEntry } from "../../types";
import HashIcon from "../../assets/icons/general/hash.svg?react";
import ImageIcon from "../../assets/icons/general/image.svg?react";
import SearchIcon from "../../assets/icons/action/search.svg?react";
import UserIcon from "../../assets/icons/user/user.svg?react";
import MessageIcon from "../../assets/icons/communication/message.svg?react";
import styles from "./SidebarSearchView.module.css";

type SearchFilter = "all" | "messages" | "photos" | "users" | "links";

const FILTERS: { key: SearchFilter; label: string }[] = [
  { key: "all", label: "All" },
  { key: "messages", label: "Messages" },
  { key: "photos", label: "Photos" },
  { key: "users", label: "Users" },
  { key: "links", label: "Links" },
];

const CATEGORY_ORDER: SearchCategory[] = ["channel", "user", "message"];
const CATEGORY_LABELS: Record<SearchCategory, string> = {
  channel: "Channels",
  user: "Users",
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
}

export function SidebarSearchView({
  query,
  channelId,
  channelName,
  onSelectChannel,
  onSelectUser,
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
      if (p.channel_id != null) {
        onSelectChannel(p.channel_id);
      }
    },
    [onSelectChannel],
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
        case "message":
          if (r.id != null) onSelectChannel(r.id);
          break;
      }
    },
    [onSelectChannel, onSelectUser],
  );

  return (
    <div className={styles.searchContainer}>
      {/* Channel scope indicator */}
      {channelId != null && channelName && (
        <div className={styles.channelScope}>
          <HashIcon width={14} height={14} />
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
                <ImageIcon className={styles.emptyIcon} width={32} height={32} />
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
                <SearchIcon className={styles.emptyIcon} width={32} height={32} />
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
          <HashIcon width={14} height={14} />
        </div>
      );
    case "user":
      return (
        <div className={`${styles.resultIcon} ${styles.resultIconUser}`}>
          <UserIcon width={14} height={14} />
        </div>
      );
    case "message":
      return (
        <div className={`${styles.resultIcon} ${styles.resultIconMessage}`}>
          <MessageIcon width={14} height={14} />
        </div>
      );
  }
}
