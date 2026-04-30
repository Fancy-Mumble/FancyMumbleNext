import { useCallback, useMemo, useRef, useState, type KeyboardEvent } from "react";
import styles from "./MemberPicker.module.css";

export interface MemberCandidate {
  readonly user_id: number;
  readonly name: string;
}

export interface MemberPickerProps {
  /** Currently selected user IDs. */
  readonly value: readonly number[];
  /** Pool of users that may be picked. */
  readonly candidates: readonly MemberCandidate[];
  /** Optional name resolver for IDs that aren't in `candidates`. */
  readonly resolveName?: (userId: number) => string;
  /** Optional avatar resolver. Returns raw image bytes (PNG/JPEG) or null. */
  readonly getAvatar?: (userId: number) => string | null | undefined;
  readonly onChange: (next: number[]) => void;
  readonly placeholder?: string;
  readonly disabled?: boolean;
  readonly emptyLabel?: string;
}

const MAX_SUGGESTIONS = 8;

/**
 * Combobox-style member picker. Renders the current selection as removable
 * chips and offers an autocomplete dropdown over the supplied `candidates`.
 */
export function MemberPicker({
  value,
  candidates,
  resolveName,
  getAvatar,
  onChange,
  placeholder = "Add user by name or ID",
  disabled,
  emptyLabel = "No members",
}: MemberPickerProps) {
  const [query, setQuery] = useState("");
  const [highlight, setHighlight] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  const selectedSet = useMemo(() => new Set(value), [value]);

  const suggestions = useMemo(() => {
    const trimmed = query.trim().toLowerCase();
    if (!trimmed) return [] as MemberCandidate[];
    const result: MemberCandidate[] = [];
    for (const c of candidates) {
      if (selectedSet.has(c.user_id)) continue;
      if (c.name.toLowerCase().includes(trimmed) || String(c.user_id) === trimmed) {
        result.push(c);
        if (result.length >= MAX_SUGGESTIONS) break;
      }
    }
    return result;
  }, [query, candidates, selectedSet]);

  const addUser = useCallback(
    (userId: number) => {
      if (selectedSet.has(userId)) return;
      onChange([...value, userId]);
      setQuery("");
      setHighlight(0);
      inputRef.current?.focus();
    },
    [onChange, value, selectedSet],
  );

  const tryCommitFromInput = useCallback(() => {
    const trimmed = query.trim();
    if (!trimmed) return;
    if (suggestions.length > 0) {
      addUser(suggestions[Math.min(highlight, suggestions.length - 1)].user_id);
      return;
    }
    const asNum = Number(trimmed);
    if (Number.isFinite(asNum) && asNum >= 0 && Number.isInteger(asNum)) {
      addUser(asNum);
    }
  }, [query, suggestions, highlight, addUser]);

  const handleKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setHighlight((h) => Math.min(h + 1, Math.max(0, suggestions.length - 1)));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setHighlight((h) => Math.max(0, h - 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      tryCommitFromInput();
    } else if (e.key === "Escape") {
      setQuery("");
    } else if (e.key === "Backspace" && query.length === 0 && value.length > 0) {
      onChange(value.slice(0, -1));
    }
  };

  const removeUser = (userId: number) => {
    onChange(value.filter((id) => id !== userId));
  };

  const labelFor = (userId: number): string => {
    const fromCandidates = candidates.find((c) => c.user_id === userId);
    if (fromCandidates) return fromCandidates.name;
    if (resolveName) return resolveName(userId);
    return `User #${userId}`;
  };

  return (
    <div className={styles.wrapper}>
      <div className={styles.chips}>
        {value.length === 0 && <span className={styles.empty}>{emptyLabel}</span>}
        {value.map((id) => {
          const avatarSrc = getAvatar?.(id) ?? null;
          return (
            <span key={id} className={styles.chip}>
              {avatarSrc ? (
                <img className={styles.chipAvatar} src={avatarSrc} alt="" />
              ) : (
                <span className={styles.chipAvatarPlaceholder} aria-hidden="true">
                  {labelFor(id).charAt(0).toUpperCase()}
                </span>
              )}
              <span className={styles.chipName}>{labelFor(id)}</span>
              {!disabled && (
                <button
                  type="button"
                  className={styles.chipRemove}
                  onClick={() => removeUser(id)}
                  aria-label={`Remove ${labelFor(id)}`}
                >
                  &times;
                </button>
              )}
            </span>
          );
        })}
      </div>
      {!disabled && (
        <div className={styles.inputRow}>
          <input
            ref={inputRef}
            type="text"
            className={styles.input}
            placeholder={placeholder}
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setHighlight(0);
            }}
            onKeyDown={handleKeyDown}
          />
          {suggestions.length > 0 && (
            <ul className={styles.suggestions}>
              {suggestions.map((s, idx) => (
                <li key={s.user_id}>
                  <button
                    type="button"
                    className={`${styles.suggestion} ${idx === highlight ? styles.active : ""}`}
                    onMouseDown={(e) => {
                      e.preventDefault();
                      addUser(s.user_id);
                    }}
                  >
                    {s.name} <span style={{ opacity: 0.5 }}>#{s.user_id}</span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}
