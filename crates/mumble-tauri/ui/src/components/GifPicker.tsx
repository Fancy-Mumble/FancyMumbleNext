/**
 * GifPicker - popup GIF / Sticker search & browse powered by Klipy.
 *
 * Tabs: GIFs | Stickers
 * Features:
 *   - Category grid on initial open.
 *   - Text search with debounced API calls.
 *   - Masonry-style result grid.
 *   - Click → inserts the GIF as an <img> message.
 */

import { useState, useEffect, useCallback, useRef } from "react";
import styles from "./GifPicker.module.css";

// ─── Klipy API Types ──────────────────────────────────────────────

interface KlipyGif {
  id: number;
  title: string;
  url: string;
  /** Preview / thumbnail URL. */
  preview: string;
  /** Width of the original. */
  width: number;
  /** Height of the original. */
  height: number;
}

interface KlipyCategory {
  id: string;
  name: string;
}

type TabId = "gifs" | "stickers";

// ─── Klipy API helpers ────────────────────────────────────────────

/**
 * Klipy v1 API - the API key is part of the URL path, NOT a query parameter.
 * Full URL pattern: https://api.klipy.com/api/v1/{API_KEY}/{content_type}/{action}
 */
const KLIPY_BASE = "https://api.klipy.com/api/v1";

/** Module-level custom API key. Set from preferences on component mount. */
let customApiKey: string | undefined;

/** Called by the GifPicker to apply a user-provided API key. */
export function setKlipyApiKey(key: string | undefined) {
  customApiKey = key?.trim() || undefined;
}

function getActiveApiKey(): string | undefined {
  return customApiKey || undefined;
}

/**
 * Low-level Klipy API fetch. The API key is embedded in the URL path.
 * `path` should start with `/gifs/...` or `/stickers/...`.
 */
async function klipyFetch<T>(path: string, params: Record<string, string> = {}): Promise<T> {
  const apiKey = getActiveApiKey();
  if (!apiKey) {
    throw new Error("No Klipy API key configured. Set one in Settings → Advanced.");
  }
  const url = new URL(`${KLIPY_BASE}/${apiKey}${path}`);
  for (const [k, v] of Object.entries(params)) {
    url.searchParams.set(k, v);
  }
  const res = await fetch(url.toString());
  if (!res.ok) throw new Error(`Klipy API error: ${res.status}`);
  const text = await res.text();
  if (!text) throw new Error("Klipy API returned empty response");
  return JSON.parse(text) as T;
}

// ─── Klipy API response shapes ───────────────────────────────────

/** A single media file variant (gif, webp, or mp4). */
interface KlipyFileMeta {
  url: string;
  width: number;
  height: number;
  size: number;
}

/** Each size variant (hd, md, sm, xs) contains gif + webp + optional mp4. */
interface KlipyFileFormats {
  gif: KlipyFileMeta;
  webp: KlipyFileMeta;
  mp4?: KlipyFileMeta;
}

/** Size variants for a media item. */
interface KlipySizeVariants {
  hd: KlipyFileFormats;
  md: KlipyFileFormats;
  sm: KlipyFileFormats;
  xs: KlipyFileFormats;
}

/** A GIF/sticker item from the Klipy API. */
interface KlipyMediaItem {
  id: number;
  title?: string;
  slug?: string;
  blur_preview?: string;
  file?: KlipySizeVariants;
  type: string;
  /** Some items use inline content instead of file variants. */
  content?: string;
  width?: number;
  height?: number;
}

/** Paginated wrapper: `{ result, data: { data: [...], current_page, per_page, has_next, meta } }`. */
interface KlipyPaginatedResponse {
  result: boolean;
  data: {
    data: KlipyMediaItem[];
    current_page: number;
    per_page: number;
    has_next: boolean;
  };
}

/** Categories response - uses the same paginated wrapper as trending/search:
 * `{ result, data: { data: ["cat1", ...], current_page, per_page, has_next, meta } }`. */
interface KlipyCategoriesResponse {
  result: boolean;
  data: {
    data: string[];
    current_page: number;
    per_page: number;
    has_next: boolean;
  };
}

// ─── Mapping helpers ─────────────────────────────────────────────

function mapMediaItems(items: KlipyMediaItem[]): KlipyGif[] {
  return items
    .map((item) => {
      // Content-based items (e.g. inline lottie/svg)
      if (item.content && !item.file) {
        return null; // We can't display these as <img>, skip
      }
      if (!item.file) return null;

      // Pick the best quality for full-size, small for preview
      const full = item.file.hd?.gif ?? item.file.md?.gif ?? item.file.sm?.gif;
      const thumb = item.file.sm?.gif ?? item.file.xs?.gif ?? full;
      if (!full) return null;

      return {
        id: item.id,
        title: item.title ?? "",
        url: full.url,
        preview: thumb.url,
        width: full.width ?? 200,
        height: full.height ?? 200,
      };
    })
    .filter(Boolean) as KlipyGif[];
}

interface PagedResult {
  items: KlipyGif[];
  hasNext: boolean;
}

async function searchGifs(query: string, tab: TabId, page = 1): Promise<PagedResult> {
  const contentType = tab === "stickers" ? "/stickers" : "/gifs";
  const data = await klipyFetch<KlipyPaginatedResponse>(`${contentType}/search`, {
    q: query,
    per_page: "30",
    page: String(page),
  });
  return { items: mapMediaItems(data.data.data), hasNext: data.data.has_next };
}

async function trendingGifs(tab: TabId, page = 1): Promise<PagedResult> {
  const contentType = tab === "stickers" ? "/stickers" : "/gifs";
  const data = await klipyFetch<KlipyPaginatedResponse>(`${contentType}/trending`, {
    per_page: "30",
    page: String(page),
  });
  return { items: mapMediaItems(data.data.data), hasNext: data.data.has_next };
}

async function fetchCategories(tab: TabId): Promise<KlipyCategory[]> {
  const contentType = tab === "stickers" ? "/stickers" : "/gifs";
  const data = await klipyFetch<KlipyCategoriesResponse>(`${contentType}/categories`);
  return (data.data?.data ?? []).map((name) => ({
    id: name,
    name,
  }));
}

// ─── Component ────────────────────────────────────────────────────

interface GifPickerProps {
  /** Called when user picks a GIF. Receives the image URL. */
  onSelect: (url: string, alt: string) => void;
  /** Close the picker. */
  onClose: () => void;
}

export default function GifPicker({ onSelect, onClose }: Readonly<GifPickerProps>) {
  const [tab, setTab] = useState<TabId>("gifs");
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<KlipyGif[]>([]);
  const [categories, setCategories] = useState<KlipyCategory[]>([]);
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [showCategories, setShowCategories] = useState(true);
  const [page, setPage] = useState(1);
  const [hasNext, setHasNext] = useState(false);
  const searchTimerRef = useRef<ReturnType<typeof setTimeout>>(undefined);
  const panelRef = useRef<HTMLDivElement>(null);
  /** Holds the active IntersectionObserver so we can disconnect on unmount. */
  const observerRef = useRef<IntersectionObserver | null>(null);
  /**
   * Stable ref to the "load next page" callback so the IntersectionObserver
   * closure never captures stale state.
   */
  const loadMoreRef = useRef<() => void>(() => {});

  /**
   * Callback ref for the sentinel div.  Called with the element when it mounts
   * (after the first results arrive) and with null when it unmounts.  Using a
   * callback ref instead of useRef + useEffect means the observer is only
   * created after the element actually exists in the DOM.
   */
  const sentinelCallbackRef = useCallback((node: HTMLDivElement | null) => {
    if (observerRef.current) {
      observerRef.current.disconnect();
      observerRef.current = null;
    }
    if (!node) return;
    observerRef.current = new IntersectionObserver(
      (entries) => {
        if (entries[0]?.isIntersecting) {
          loadMoreRef.current();
        }
      },
      { threshold: 0.1 },
    );
    observerRef.current.observe(node);
  }, []);

  // Close when clicking outside.
  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (panelRef.current && !panelRef.current.contains(e.target as Node)) {
        onClose();
      }
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [onClose]);

  // Load categories on mount and when tab changes.
  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    fetchCategories(tab)
      .then((cats) => {
        if (!cancelled) {
          setCategories(cats);
          setShowCategories(true);
        }
      })
      .catch(console.error)
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    // Also load trending as initial results (page 1).
    trendingGifs(tab, 1)
      .then(({ items, hasNext: hn }) => {
        if (!cancelled) {
          setResults(items);
          setHasNext(hn);
          setPage(1);
        }
      })
      .catch(console.error);

    return () => {
      cancelled = true;
    };
  }, [tab]);

  // Debounced search - resets to page 1.
  useEffect(() => {
    if (!query.trim()) {
      setShowCategories(true);
      trendingGifs(tab, 1)
        .then(({ items, hasNext: hn }) => {
          setResults(items);
          setHasNext(hn);
          setPage(1);
        })
        .catch(console.error);
      return;
    }
    setShowCategories(false);
    clearTimeout(searchTimerRef.current);
    searchTimerRef.current = setTimeout(() => {
      setLoading(true);
      searchGifs(query, tab, 1)
        .then(({ items, hasNext: hn }) => {
          setResults(items);
          setHasNext(hn);
          setPage(1);
        })
        .catch(console.error)
        .finally(() => setLoading(false));
    }, 350);
    return () => clearTimeout(searchTimerRef.current);
  }, [query, tab]);

  // Keep loadMoreRef up to date with the latest state without re-creating the observer.
  useEffect(() => {
    async function fetchNextPage(nextPage: number) {
      const result = query.trim()
        ? await searchGifs(query, tab, nextPage)
        : await trendingGifs(tab, nextPage);
      setResults((prev) => [...prev, ...result.items]);
      setHasNext(result.hasNext);
      setPage(nextPage);
    }

    loadMoreRef.current = () => {
      if (!hasNext || loadingMore) return;
      const nextPage = page + 1;
      setLoadingMore(true);
      fetchNextPage(nextPage)
        .catch(console.error)
        .finally(() => setLoadingMore(false));
    };
  });

  const handleCategoryClick = useCallback((cat: KlipyCategory) => {
    setQuery(cat.id);
    setShowCategories(false);
  }, []);

  const handleGifClick = useCallback(
    (gif: KlipyGif) => {
      onSelect(gif.url, gif.title || "GIF");
      onClose();
    },
    [onSelect, onClose],
  );

  return (
    <div ref={panelRef} className={styles.picker}>
      {/* Header with tabs */}
      <div className={styles.header}>
        <div className={styles.tabs}>
          <button
            className={`${styles.tab} ${tab === "gifs" ? styles.active : ""}`}
            onClick={() => { setTab("gifs"); setQuery(""); }}
          >
            GIFs
          </button>
          <button
            className={`${styles.tab} ${tab === "stickers" ? styles.active : ""}`}
            onClick={() => { setTab("stickers"); setQuery(""); }}
          >
            Stickers
          </button>
        </div>
        <button className={styles.closeBtn} onClick={onClose}>
          ✕
        </button>
      </div>

      {/* Search bar */}
      <div className={styles.searchBar}>
        <svg className={styles.searchIcon} width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <circle cx="11" cy="11" r="8" />
          <path d="M21 21l-4.35-4.35" />
        </svg>
        <input
          className={styles.searchInput}
          placeholder={`Search ${tab}…`}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          autoFocus
        />
      </div>

      {/* Content */}
      <div className={styles.content}>
        {loading && results.length === 0 && (
          <div className={styles.loadingMsg}>Loading…</div>
        )}

        {/* Category grid */}
        {showCategories && categories.length > 0 && (
          <div className={styles.categories}>
            {categories.slice(0, 12).map((cat) => (
              <button
                key={cat.id}
                className={styles.categoryCard}
                onClick={() => handleCategoryClick(cat)}
              >
                <span className={styles.categoryLabel}>{cat.name}</span>
              </button>
            ))}
          </div>
        )}

        {/* GIF results grid */}
        {results.length > 0 && (
          <div className={styles.grid}>
            {results.map((gif) => (
              <button
                key={gif.id}
                className={styles.gifCard}
                onClick={() => handleGifClick(gif)}
                title={gif.title}
              >
                <img
                  src={gif.preview}
                  alt={gif.title}
                  loading="lazy"
                  className={styles.gifImg}
                />
              </button>
            ))}
            {/* Infinite scroll sentinel - uses a callback ref so the
                IntersectionObserver is created only after this element mounts. */}
            <div ref={sentinelCallbackRef} className={styles.sentinel} />
          </div>
        )}

        {/* Spinner shown while loading subsequent pages */}
        {loadingMore && (
          <div className={styles.loadingMore}>Loading…</div>
        )}

        {!loading && results.length === 0 && query && (
          <div className={styles.emptyMsg}>
            No {tab} found for "{query}"
          </div>
        )}
      </div>

      {/* Footer / attribution */}
      <div className={styles.footer}>
        <span>Powered by Klipy</span>
      </div>
    </div>
  );
}
