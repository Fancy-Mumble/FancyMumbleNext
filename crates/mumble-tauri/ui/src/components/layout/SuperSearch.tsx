import { HashIcon, MessageIcon, SearchIcon, UserIcon } from "../../icons";
import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import type { SearchResult, SearchCategory } from "../../types";
import styles from "./SuperSearch.module.css";

const CATEGORY_ORDER: SearchCategory[] = ["channel", "user", "message"];
const CATEGORY_LABELS: Record<SearchCategory, string> = {
  channel: "Channels",
  user: "Users",
  message: "Messages",
};

interface SuperSearchProps {
  readonly open: boolean;
  readonly onClose: () => void;
  readonly onSelectChannel: (id: number) => void;
  readonly onSelectUser: (session: number) => void;
}

export function SuperSearch({
  open,
  onClose,
  onSelectChannel,
  onSelectUser,
}: SuperSearchProps) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResult[]>([]);
  const [activeIdx, setActiveIdx] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const backdropRef = useRef<HTMLDivElement>(null);
  const resultsRef = useRef<HTMLDivElement>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Focus input when opened.
  useEffect(() => {
    if (open) {
      setQuery("");
      setResults([]);
      setActiveIdx(0);
      // Small delay so the portal is mounted before we focus.
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  // Debounced search.
  const doSearch = useCallback(async (q: string) => {
    if (!q.trim()) {
      setResults([]);
      setActiveIdx(0);
      return;
    }
    try {
      const res = await invoke<SearchResult[]>("super_search", { query: q });
      setResults(res);
      setActiveIdx(0);
    } catch {
      setResults([]);
    }
  }, []);

  const handleChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const val = e.target.value;
      setQuery(val);
      if (debounceRef.current) clearTimeout(debounceRef.current);
      debounceRef.current = setTimeout(() => doSearch(val), 120);
    },
    [doSearch],
  );

  // Group results by category in display order.
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

  // Flat list for keyboard nav.
  const flatItems = useMemo(
    () => grouped.flatMap((g) => g.items),
    [grouped],
  );

  // Scroll active item into view.
  useEffect(() => {
    if (!resultsRef.current) return;
    const el = resultsRef.current.querySelector(`[data-idx="${activeIdx}"]`);
    if (el) el.scrollIntoView({ block: "nearest" });
  }, [activeIdx]);

  const selectResult = useCallback(
    (r: SearchResult) => {
      onClose();
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
    [onClose, onSelectChannel, onSelectUser],
  );

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setActiveIdx((i) => (i + 1) % Math.max(flatItems.length, 1));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setActiveIdx((i) => (i - 1 + flatItems.length) % Math.max(flatItems.length, 1));
      } else if (e.key === "Enter") {
        e.preventDefault();
        const item = flatItems[activeIdx];
        if (item) selectResult(item);
      } else if (e.key === "Escape") {
        onClose();
      }
    },
    [flatItems, activeIdx, selectResult, onClose],
  );

  const handleBackdropClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === backdropRef.current) onClose();
    },
    [onClose],
  );

  if (!open) return null;

  let flatIdx = 0;

  return createPortal(
    // eslint-disable-next-line jsx-a11y/no-static-element-interactions
    <div
      ref={backdropRef}
      className={styles.backdrop}
      onClick={handleBackdropClick}
      onKeyDown={handleKeyDown}
    >
      <div className={styles.panel}>
        {/* Search input */}
        <div className={styles.inputRow}>
          <SearchIcon
            className={styles.inputIcon}
            width={16}
            height={16}
          />
          <input
            ref={inputRef}
            className={styles.input}
            type="text"
            placeholder="Search channels, users, messages..."
            value={query}
            onChange={handleChange}
          />
        </div>

        {/* Results */}
        <div ref={resultsRef} className={styles.results}>
          {query.trim() && results.length === 0 && (
            <div className={styles.empty}>No results found</div>
          )}

          {grouped.map((group) => (
            <div key={group.category}>
              <div className={styles.categoryLabel}>
                {CATEGORY_LABELS[group.category]}
              </div>
              {group.items.map((r) => {
                const idx = flatIdx++;
                return (
                  <button
                    key={`${r.category}-${r.id ?? r.string_id}-${idx}`}
                    type="button"
                    data-idx={idx}
                    className={`${styles.resultItem} ${idx === activeIdx ? styles.resultItemActive : ""}`}
                    onClick={() => selectResult(r)}
                    onMouseEnter={() => setActiveIdx(idx)}
                  >
                    <ResultIcon category={r.category} />
                    <div className={styles.resultText}>
                      <span className={styles.resultTitle}>{r.title}</span>
                      {r.subtitle && (
                        <span className={styles.resultSubtitle}>{r.subtitle}</span>
                      )}
                    </div>
                  </button>
                );
              })}
            </div>
          ))}
        </div>

        {/* Footer hints */}
        <div className={styles.footer}>
          <span><span className={styles.footerKey}>↑↓</span> navigate</span>
          <span><span className={styles.footerKey}>↵</span> select</span>
          <span><span className={styles.footerKey}>esc</span> close</span>
        </div>
      </div>
    </div>,
    document.body,
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
