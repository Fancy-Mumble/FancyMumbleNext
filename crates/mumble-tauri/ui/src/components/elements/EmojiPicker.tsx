/**
 * Full emoji picker component with category tabs, search, and server
 * custom reactions.  Renders as a fixed-position overlay (portal).
 *
 * Mobile: full-width bottom sheet style.
 * Desktop: positioned near the anchor point.
 */

import { useState, useCallback, useMemo, useRef, useEffect } from "react";
import { createPortal } from "react-dom";
import { getServerCustomReactions } from "../chat/reactionStore";
import styles from "./EmojiPicker.module.css";

// -- Emoji data ---------------------------------------------------
// Minimal built-in set grouped by category.  Keeps the bundle small
// while covering the most common reactions.

interface EmojiCategory {
  readonly id: string;
  readonly icon: string;
  readonly label: string;
  readonly emojis: readonly string[];
}

const CATEGORIES: readonly EmojiCategory[] = [
  {
    id: "people",
    icon: "\uD83D\uDE00",
    label: "Smileys & People",
    emojis: [
      "\uD83D\uDE00", "\uD83D\uDE03", "\uD83D\uDE04", "\uD83D\uDE01", "\uD83D\uDE06",
      "\uD83D\uDE05", "\uD83D\uDE02", "\uD83E\uDD23", "\uD83D\uDE0A", "\uD83D\uDE07",
      "\uD83D\uDE42", "\uD83D\uDE43", "\uD83D\uDE09", "\uD83D\uDE0C", "\uD83D\uDE0D",
      "\uD83E\uDD70", "\uD83D\uDE18", "\uD83D\uDE17", "\uD83D\uDE19", "\uD83D\uDE1A",
      "\uD83D\uDE0B", "\uD83D\uDE1B", "\uD83D\uDE1C", "\uD83E\uDD2A", "\uD83D\uDE1D",
      "\uD83E\uDD11", "\uD83E\uDD17", "\uD83E\uDD2D", "\uD83E\uDD2B", "\uD83E\uDD14",
      "\uD83E\uDD10", "\uD83E\uDD28", "\uD83D\uDE10", "\uD83D\uDE11", "\uD83D\uDE36",
      "\uD83D\uDE0F", "\uD83D\uDE12", "\uD83D\uDE44", "\uD83D\uDE2C", "\uD83E\uDD25",
      "\uD83D\uDE0E", "\uD83E\uDD13", "\uD83E\uDD78", "\uD83E\uDD20", "\uD83E\uDD21",
      "\uD83D\uDE34", "\uD83D\uDE2A", "\uD83D\uDE35", "\uD83E\uDD10", "\uD83E\uDD75",
      "\uD83E\uDD76", "\uD83E\uDD74", "\uD83D\uDE22", "\uD83D\uDE2D", "\uD83D\uDE29",
      "\uD83D\uDE31", "\uD83D\uDE28", "\uD83D\uDE30", "\uD83D\uDE25", "\uD83D\uDE13",
      "\uD83D\uDE2E", "\uD83D\uDE32", "\uD83E\uDD2F", "\uD83D\uDE33", "\uD83D\uDE26",
      "\uD83D\uDE27", "\uD83D\uDE2E", "\uD83D\uDE15", "\uD83D\uDE16", "\uD83D\uDE23",
      "\uD83D\uDE1E", "\uD83D\uDE1F", "\uD83D\uDE24", "\uD83D\uDE21", "\uD83D\uDE20",
      "\uD83E\uDD2C", "\uD83D\uDE08", "\uD83D\uDC7F", "\uD83D\uDC80", "\uD83D\uDCA9",
      "\uD83E\uDD21", "\uD83D\uDC7B", "\uD83D\uDC7D", "\uD83E\uDD16", "\uD83D\uDE3A",
      "\uD83D\uDC4D", "\uD83D\uDC4E", "\uD83D\uDC4F", "\uD83D\uDE4C", "\uD83D\uDC4B",
      "\uD83E\uDD1A", "\u270B", "\uD83D\uDD96", "\uD83D\uDC4C", "\u270C\uFE0F",
      "\uD83E\uDD1E", "\uD83E\uDD1F", "\uD83E\uDD18", "\uD83E\uDD19", "\uD83D\uDC48",
      "\uD83D\uDC49", "\uD83D\uDC46", "\uD83D\uDC47", "\u261D\uFE0F", "\uD83D\uDCAA",
      "\uD83E\uDD1D", "\uD83D\uDE4F",
    ],
  },
  {
    id: "nature",
    icon: "\uD83D\uDC36",
    label: "Animals & Nature",
    emojis: [
      "\uD83D\uDC36", "\uD83D\uDC31", "\uD83D\uDC2D", "\uD83D\uDC39", "\uD83D\uDC30",
      "\uD83E\uDD8A", "\uD83D\uDC3B", "\uD83D\uDC28", "\uD83D\uDC2F", "\uD83E\uDD81",
      "\uD83D\uDC2E", "\uD83D\uDC37", "\uD83D\uDC38", "\uD83D\uDC35", "\uD83D\uDC12",
      "\uD83D\uDC14", "\uD83D\uDC27", "\uD83D\uDC26", "\uD83E\uDD85", "\uD83E\uDD86",
      "\uD83E\uDD89", "\uD83D\uDC1D", "\uD83D\uDC1B", "\uD83E\uDD8B", "\uD83D\uDC0C",
      "\uD83C\uDF37", "\uD83C\uDF39", "\uD83C\uDF3B", "\uD83C\uDF3A", "\uD83C\uDF38",
      "\uD83C\uDF3C", "\uD83C\uDF3E", "\uD83C\uDF32", "\uD83C\uDF33", "\uD83C\uDF34",
      "\uD83C\uDF35", "\uD83C\uDF40", "\uD83C\uDF41", "\uD83C\uDF42", "\uD83C\uDF43",
    ],
  },
  {
    id: "food",
    icon: "\uD83C\uDF54",
    label: "Food & Drink",
    emojis: [
      "\uD83C\uDF4E", "\uD83C\uDF4F", "\uD83C\uDF4A", "\uD83C\uDF4B", "\uD83C\uDF4C",
      "\uD83C\uDF49", "\uD83C\uDF47", "\uD83C\uDF53", "\uD83C\uDF48", "\uD83C\uDF52",
      "\uD83C\uDF51", "\uD83E\uDD6D", "\uD83C\uDF4D", "\uD83E\uDD65", "\uD83E\uDD5D",
      "\uD83C\uDF45", "\uD83C\uDF46", "\uD83E\uDD51", "\uD83E\uDD66", "\uD83E\uDD6C",
      "\uD83C\uDF54", "\uD83C\uDF55", "\uD83C\uDF2E", "\uD83C\uDF2F", "\uD83E\uDD59",
      "\uD83C\uDF5D", "\uD83C\uDF5C", "\uD83C\uDF63", "\uD83C\uDF71", "\uD83C\uDF5B",
      "\uD83C\uDF5A", "\uD83C\uDF70", "\uD83C\uDF82", "\uD83C\uDF66", "\uD83C\uDF69",
      "\u2615", "\uD83C\uDF75", "\uD83E\uDD64", "\uD83C\uDF7A", "\uD83C\uDF77",
    ],
  },
  {
    id: "activities",
    icon: "\u26BD",
    label: "Activities",
    emojis: [
      "\u26BD", "\uD83C\uDFC0", "\uD83C\uDFC8", "\u26BE", "\uD83E\uDD4E",
      "\uD83C\uDFBE", "\uD83C\uDFD0", "\uD83C\uDFC9", "\uD83E\uDD4F", "\uD83C\uDFB1",
      "\uD83C\uDFD3", "\uD83C\uDFF8", "\uD83C\uDFAE", "\uD83C\uDFB2", "\uD83E\uDDE9",
      "\uD83C\uDFAF", "\uD83C\uDFA3", "\uD83C\uDFBF", "\uD83C\uDFC2", "\uD83C\uDFCB\uFE0F",
      "\uD83E\uDD3C", "\uD83E\uDD38", "\u26F9\uFE0F", "\uD83E\uDD3A", "\uD83C\uDFC7",
      "\uD83C\uDFC6", "\uD83E\uDD47", "\uD83E\uDD48", "\uD83E\uDD49", "\uD83C\uDFF5\uFE0F",
    ],
  },
  {
    id: "objects",
    icon: "\uD83D\uDCA1",
    label: "Objects",
    emojis: [
      "\uD83D\uDCA1", "\uD83D\uDD26", "\uD83D\uDCBB", "\uD83D\uDCF1", "\u260E\uFE0F",
      "\uD83D\uDCE7", "\uD83D\uDCE8", "\u2709\uFE0F", "\uD83D\uDCDD", "\uD83D\uDCDA",
      "\uD83D\uDCCA", "\uD83D\uDCC8", "\uD83D\uDCC9", "\uD83D\uDD12", "\uD83D\uDD13",
      "\uD83D\uDD11", "\uD83D\uDEE0\uFE0F", "\u2699\uFE0F", "\uD83D\uDD27", "\uD83D\uDCA3",
      "\uD83D\uDC8E", "\uD83D\uDCB0", "\uD83D\uDCB3", "\uD83C\uDFA4", "\uD83C\uDFB5",
      "\uD83C\uDFB6", "\uD83C\uDFA7", "\uD83D\uDCF7", "\uD83D\uDCF9", "\uD83C\uDFA5",
    ],
  },
  {
    id: "symbols",
    icon: "\u2764\uFE0F",
    label: "Symbols",
    emojis: [
      "\u2764\uFE0F", "\uD83E\uDDE1", "\uD83D\uDC9B", "\uD83D\uDC9A", "\uD83D\uDC99",
      "\uD83D\uDC9C", "\uD83D\uDDA4", "\uD83E\uDD0D", "\uD83E\uDD0E", "\uD83D\uDC94",
      "\u2763\uFE0F", "\uD83D\uDC95", "\uD83D\uDC9E", "\u2705", "\u274C",
      "\u2757", "\u2753", "\u2B50", "\uD83C\uDF1F", "\u26A1",
      "\uD83D\uDD25", "\uD83D\uDCA5", "\uD83C\uDF88", "\uD83C\uDF89", "\u2728",
      "\uD83D\uDC4D", "\uD83D\uDC4E", "\uD83D\uDCAF", "\uD83C\uDD97", "\uD83C\uDD98",
    ],
  },
  {
    id: "flags",
    icon: "\uD83C\uDFF3\uFE0F",
    label: "Flags",
    emojis: [
      "\uD83C\uDFF3\uFE0F", "\uD83C\uDFF4", "\uD83C\uDFC1", "\uD83D\uDEA9",
      "\uD83C\uDDE9\uD83C\uDDEA", "\uD83C\uDDFA\uD83C\uDDF8", "\uD83C\uDDEC\uD83C\uDDE7",
      "\uD83C\uDDEB\uD83C\uDDF7", "\uD83C\uDDEA\uD83C\uDDF8", "\uD83C\uDDEE\uD83C\uDDF9",
      "\uD83C\uDDF7\uD83C\uDDFA", "\uD83C\uDDEF\uD83C\uDDF5", "\uD83C\uDDF0\uD83C\uDDF7",
      "\uD83C\uDDE8\uD83C\uDDF3", "\uD83C\uDDE7\uD83C\uDDF7", "\uD83C\uDDE8\uD83C\uDDE6",
      "\uD83C\uDDE6\uD83C\uDDFA", "\uD83C\uDDF2\uD83C\uDDFD", "\uD83C\uDDEE\uD83C\uDDF3",
      "\uD83C\uDDF3\uD83C\uDDF1", "\uD83C\uDDF8\uD83C\uDDEA",
    ],
  },
];

// -- Props --------------------------------------------------------

interface EmojiPickerProps {
  /** Anchor point for desktop positioning (ignored on mobile). */
  readonly anchorX: number;
  readonly anchorY: number;
  /** Called when an emoji is selected. */
  readonly onSelect: (emoji: string) => void;
  /** Called to close the picker. */
  readonly onClose: () => void;
}

// -- Component ----------------------------------------------------

export default function EmojiPicker({
  anchorX,
  anchorY,
  onSelect,
  onClose,
}: EmojiPickerProps) {
  const [search, setSearch] = useState("");
  const [activeCategory, setActiveCategory] = useState(CATEGORIES[0].id);
  const gridRef = useRef<HTMLDivElement>(null);
  const pickerRef = useRef<HTMLDivElement>(null);

  // Server custom reactions (loaded once per connection).
  const serverReactions = useMemo(() => getServerCustomReactions(), []);

  // Flatten + filter by search term.
  const filteredCategories = useMemo(() => {
    const term = search.trim().toLowerCase();
    const result: { id: string; label: string; emojis: string[] }[] = [];

    // Server custom reactions first when present.
    if (serverReactions.length > 0) {
      const filtered = term
        ? serverReactions.filter(
            (r) =>
              r.shortcode.toLowerCase().includes(term) ||
              r.label?.toLowerCase().includes(term) ||
              r.display.includes(term),
          )
        : serverReactions;
      if (filtered.length > 0) {
        result.push({
          id: "server",
          label: "Server",
          emojis: filtered.map((r) => r.display),
        });
      }
    }

    for (const cat of CATEGORIES) {
      if (!term) {
        result.push({ id: cat.id, label: cat.label, emojis: [...cat.emojis] });
        continue;
      }
      // Unicode search: match emoji by label or by the emoji character.
      const matched = cat.emojis.filter((e) => e.includes(term));
      if (matched.length > 0) {
        result.push({ id: cat.id, label: cat.label, emojis: matched });
      }
    }
    return result;
  }, [search, serverReactions]);

  // Scroll to category on tab click.
  const handleCategoryClick = useCallback((catId: string) => {
    setActiveCategory(catId);
    const el = gridRef.current?.querySelector<HTMLElement>(`[data-cat="${catId}"]`);
    el?.scrollIntoView({ behavior: "smooth", block: "start" });
  }, []);

  // Close on Escape.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);

  // Compute picker position (desktop only; CSS overrides for mobile).
  const pickerStyle = useMemo((): React.CSSProperties => {
    const w = 320;
    const h = 380;
    let left = anchorX;
    let top = anchorY;

    if (left + w > window.innerWidth - 12) left = window.innerWidth - w - 12;
    if (left < 12) left = 12;
    if (top + h > window.innerHeight - 12) top = anchorY - h;
    if (top < 12) top = 12;

    return { left, top };
  }, [anchorX, anchorY]);

  // All category tab icons (including server tab when present).
  const tabs = useMemo(() => {
    const base = CATEGORIES.map((c) => ({ id: c.id, icon: c.icon }));
    if (serverReactions.length > 0) {
      return [{ id: "server", icon: "\uD83C\uDFE2" }, ...base];
    }
    return base;
  }, [serverReactions]);

  return createPortal(
    <>
      <div className={styles.pickerOverlay} onClick={onClose} />
      <div ref={pickerRef} className={styles.picker} style={pickerStyle}>
        {/* Search */}
        <div className={styles.searchRow}>
          <input
            type="text"
            className={styles.searchInput}
            placeholder="Search emoji..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            autoFocus
          />
        </div>

        {/* Category tabs */}
        <div className={styles.categoryTabs} role="tablist">
          {tabs.map((t) => (
            <button
              key={t.id}
              type="button"
              role="tab"
              aria-selected={activeCategory === t.id}
              className={`${styles.categoryTab} ${activeCategory === t.id ? styles.categoryTabActive : ""}`}
              onClick={() => handleCategoryClick(t.id)}
            >
              {t.icon}
            </button>
          ))}
        </div>

        {/* Emoji grid */}
        <div ref={gridRef} className={styles.emojiGrid}>
          {filteredCategories.length === 0 && (
            <div className={styles.emptyState}>No emoji found</div>
          )}
          {filteredCategories.map((cat) => (
            <div key={cat.id} data-cat={cat.id}>
              <p className={cat.id === "server" ? styles.serverLabel : styles.categoryLabel}>
                {cat.label}
              </p>
              <div className={styles.emojiRow}>
                {cat.emojis.map((emoji, i) => (
                  <button
                    key={`${cat.id}-${i}`}
                    type="button"
                    className={styles.emojiBtn}
                    onClick={() => {
                      onSelect(emoji);
                      onClose();
                    }}
                  >
                    {emoji}
                  </button>
                ))}
              </div>
            </div>
          ))}
        </div>
      </div>
    </>,
    document.body,
  );
}
