import { useState, useRef, useCallback, useMemo, useEffect, type ClipboardEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import MarkdownInput, { type MarkdownInputApi } from "./MarkdownInput";
import GifPicker from "./GifPicker";
import MentionAutocomplete, { type MentionCandidate, handleMentionKey } from "./MentionAutocomplete";
import styles from "./ChatView.module.css";
import AttachIcon from "../../assets/icons/action/attach.svg?react";
import GifIcon from "../../assets/icons/communication/gif.svg?react";
import SendIcon from "../../assets/icons/action/send.svg?react";
import CloseIcon from "../../assets/icons/action/close.svg?react";
import EditIcon from "../../assets/icons/action/edit.svg?react";
import { isMobile } from "../../utils/platform";
import { useAppStore } from "../../store";
import { formatUserMention, parseMentionTrigger, type MentionTrigger } from "../../utils/mentions";
import { textureToDataUrl } from "../../profileFormat";
import { rootChannelId } from "../../pages/admin/rootChannel";
import type { AclData, AclGroup } from "../../types";

interface ChatComposerProps {
  readonly draft: string;
  readonly onChange: (value: string) => void;
  readonly onSend: () => void;
  readonly onPaste: (e: ClipboardEvent) => void;
  readonly onFileSelected: (file: File) => Promise<void>;
  readonly onGifSelect: (url: string, alt: string) => Promise<void>;
  readonly disabled?: boolean;
  readonly hasPendingQuotes?: boolean;
  readonly isEditing?: boolean;
  readonly onCancelEdit?: () => void;
}

const MAX_CANDIDATES = 8;

function candidateInsertText(c: MentionCandidate): string {
  switch (c.kind) {
    case "user":
      return formatUserMention(c.session);
    case "role":
      return `<@&${c.name}>`;
    case "everyone":
      return "@everyone";
    case "here":
      return "@here";
  }
}

/**
 * Subscribe to the root-channel ACL so the chat composer can suggest
 * role mentions (e.g. `@admin`). Uses a small local cache and re-fetches
 * lazily on mount.
 */
function useRoleCandidates(): readonly AclGroup[] {
  const channels = useAppStore((s) => s.channels);
  const rootId = useMemo(() => rootChannelId(channels), [channels]);
  const [groups, setGroups] = useState<readonly AclGroup[]>([]);

  useEffect(() => {
    let cancelled = false;
    const unlisten = listen<AclData>("acl", (event) => {
      if (!cancelled && event.payload.channel_id === rootId) {
        setGroups(event.payload.groups);
      }
    });
    invoke("request_acl", { channelId: rootId }).catch(() => {});
    return () => {
      cancelled = true;
      unlisten.then((f) => f());
    };
  }, [rootId]);

  return groups;
}

export default function ChatComposer({
  draft,
  onChange,
  onSend,
  onPaste,
  onFileSelected,
  onGifSelect,
  disabled = false,
  hasPendingQuotes = false,
  isEditing = false,
  onCancelEdit,
}: ChatComposerProps) {
  const [showGifPicker, setShowGifPicker] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const inputApi = useRef<MarkdownInputApi | null>(null);

  const [trigger, setTrigger] = useState<MentionTrigger | null>(null);
  const [activeIndex, setActiveIndex] = useState(0);

  const users = useAppStore((s) => s.users);
  const selectedChannel = useAppStore((s) => s.selectedChannel);
  const ownSession = useAppStore((s) => s.ownSession);
  const roleGroups = useRoleCandidates();

  const candidates = useMemo<MentionCandidate[]>(() => {
    if (!trigger) return [];
    const q = trigger.query.toLowerCase();

    if (trigger.kind === "user") {
      const inChannel = users.filter((u) => {
        if (selectedChannel != null && u.channel_id !== selectedChannel) return false;
        if (u.session === ownSession) return false;
        return u.name.toLowerCase().includes(q);
      });
      const userCandidates: MentionCandidate[] = inChannel
        .slice(0, MAX_CANDIDATES)
        .map((u) => ({
          kind: "user",
          session: u.session,
          name: u.name,
          avatarUrl: u.texture && u.texture.length > 0 ? textureToDataUrl(u.texture) : undefined,
        }));

      const roleCandidates: MentionCandidate[] = roleGroups
        .filter((g) => !g.name.startsWith("~") && g.name.toLowerCase().includes(q))
        .slice(0, MAX_CANDIDATES)
        .map((g) => ({ kind: "role", name: g.name }));

      const extras: MentionCandidate[] = [];
      if ("everyone".startsWith(q)) extras.push({ kind: "everyone" });
      if ("here".startsWith(q)) extras.push({ kind: "here" });
      return [...userCandidates, ...roleCandidates, ...extras];
    }

    if (trigger.kind === "role") {
      return roleGroups
        .filter((g) => !g.name.startsWith("~") && g.name.toLowerCase().includes(q))
        .slice(0, MAX_CANDIDATES)
        .map((g) => ({ kind: "role", name: g.name }));
    }

    return [];
  }, [trigger, users, selectedChannel, ownSession, roleGroups]);

  useEffect(() => {
    if (activeIndex >= candidates.length) setActiveIndex(0);
  }, [candidates.length, activeIndex]);

  const handleAttach = useCallback(() => {
    fileInputRef.current?.click();
  }, []);

  const handleFileChange = useCallback(
    async (e: React.ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0];
      if (!file) return;
      e.target.value = "";
      await onFileSelected(file);
    },
    [onFileSelected],
  );

  const handleSelectionChange = useCallback(
    (start: number, end: number) => {
      if (start !== end) {
        if (trigger) setTrigger(null);
        return;
      }
      const next = parseMentionTrigger(draft, start);
      if (
        next?.anchor === trigger?.anchor &&
        next?.query === trigger?.query &&
        next?.kind === trigger?.kind
      ) {
        return;
      }
      setTrigger(next);
      setActiveIndex(0);
    },
    [draft, trigger],
  );

  useEffect(() => {
    if (trigger && draft.charAt(trigger.anchor) !== "@") {
      setTrigger(null);
    }
  }, [draft, trigger]);

  const closePopup = useCallback(() => setTrigger(null), []);

  const insertCandidate = useCallback(
    (c: MentionCandidate) => {
      if (!trigger) return;
      const replacement = candidateInsertText(c);
      const queryLen = trigger.kind === "role" ? trigger.query.length + 2 : trigger.query.length + 1;
      const end = trigger.anchor + queryLen;
      inputApi.current?.replaceRange(trigger.anchor, end, `${replacement} `);
      setTrigger(null);
    },
    [trigger],
  );

  const handleKeyDownCapture = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>): boolean => {
      if (!trigger || candidates.length === 0) return false;
      const action = handleMentionKey(e, { activeIndex, count: candidates.length });
      if (!action) return false;
      e.preventDefault();
      switch (action.kind) {
        case "move":
          setActiveIndex(action.index);
          return true;
        case "pick":
          insertCandidate(candidates[action.index]);
          return true;
        case "close":
          closePopup();
          return true;
      }
    },
    [trigger, candidates, activeIndex, insertCandidate, closePopup],
  );

  return (
    <div className={styles.composerWrapper}>
      {isEditing && (
        <div className={styles.editBanner}>
          <EditIcon width={14} height={14} />
          <span>Editing message</span>
          <button type="button" className={styles.editBannerClose} onClick={onCancelEdit}>
            <CloseIcon width={14} height={14} />
          </button>
        </div>
      )}
      {showGifPicker && (
        <GifPicker
          onSelect={onGifSelect}
          onClose={() => setShowGifPicker(false)}
        />
      )}
      <div className={styles.composer}>
        <input
          ref={fileInputRef}
          type="file"
          accept="image/*,video/*"
          className={styles.hiddenFileInput}
          onChange={handleFileChange}
        />

        <button
          className={styles.attachBtn}
          onClick={handleAttach}
          disabled={disabled}
          title="Attach image, GIF, or video"
        >
          <AttachIcon width={20} height={20} />
        </button>

        <button
          className={`${styles.attachBtn} ${showGifPicker ? styles.attachBtnActive : ""}`}
          onClick={() => setShowGifPicker((s) => !s)}
          disabled={disabled}
          title="GIF picker"
        >
          <GifIcon width={20} height={20} />
        </button>

        <div className={styles.composerInputWrap}>
          {trigger && (
            <MentionAutocomplete
              candidates={candidates}
              activeIndex={activeIndex}
              onPick={insertCandidate}
              onActiveIndexChange={setActiveIndex}
            />
          )}

          <MarkdownInput
            value={draft}
            onChange={onChange}
            onSubmit={onSend}
            onPaste={onPaste}
            placeholder={isMobile ? "Write a message..." : "Write a message... (Ctrl+B/I/U for formatting)"}
            disabled={disabled}
            apiRef={inputApi}
            onSelectionChange={handleSelectionChange}
            onKeyDownCapture={handleKeyDownCapture}
          />
        </div>

        <button
          className={styles.sendBtn}
          onClick={onSend}
          disabled={(!draft.trim() && !hasPendingQuotes) || disabled}
        >
          <SendIcon width={20} height={20} />
        </button>
      </div>
    </div>
  );
}
