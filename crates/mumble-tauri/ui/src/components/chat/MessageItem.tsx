import { memo, useState, useCallback, useRef, useMemo, useEffect } from "react";
import { createPortal } from "react-dom";
import type { ChatMessage, UserEntry, TimeFormat } from "../../types";
import type { PollPayload } from "./PollCreator";
import { useAppStore, requestLinkPreview } from "../../store";
import { parseComment } from "../../profileFormat";
import { ProfilePreviewCard } from "../../pages/settings/ProfilePreviewCard";
import { useUserStats } from "../../hooks/useUserStats";
import { isMobile } from "../../utils/platform";
import { formatTimestamp, colorFor } from "../../utils/format";
import { extractUrlsFromMessage } from "../../utils/extractUrls";
import { extractOffloadInfo } from "../../messageOffload";
import { containsSelfMention } from "../../utils/mentions";
import PollCard, { getPoll } from "./PollCard";
import MediaPreview from "./MediaPreview";
import LinkPreviewCard from "./LinkPreviewCard";
import FileAttachmentCard, {
  FANCY_FILE_MARKER_RE,
  decodeFileAttachmentPayload,
} from "./FileAttachmentCard";
import WatchTogetherCard from "./watch/WatchTogetherCard";
import WatchStartButton from "./watch/WatchStartButton";
import QuoteBlock from "../elements/QuoteBlock";
import styles from "./ChatView.module.css";

/** Regex to match quote reference markers in message bodies. */
const QUOTE_RE = /<!-- FANCY_QUOTE:(.+?) -->/g;

/** Regex to match watch-together markers in message bodies. */
const WATCH_RE = /<!-- FANCY_WATCH:(.+?) -->/;

/** Approximate height of the profile hover card, used for viewport clamping. */
const HOVER_CARD_H = 340;
const HOVER_CARD_MARGIN = 10;

/**
 * Set of message IDs we have already processed for self-mention
 * notifications.  Module-scope so it survives unmount/remount cycles
 * (e.g. when scrolling messages out of view) and prevents duplicate
 * sound triggers when the same message re-renders.
 */
const NOTIFIED_SELF_MENTIONS = new Set<string>();

/**
 * Cap to keep memory bounded for long-running sessions.  When the set
 * grows past this many entries, drop the oldest insertion (Set
 * iteration order is insertion order).
 */
const NOTIFIED_SELF_MENTIONS_CAP = 2000;

function markSelfMentionNotified(id: string) {
  if (NOTIFIED_SELF_MENTIONS.size >= NOTIFIED_SELF_MENTIONS_CAP) {
    const oldest = NOTIFIED_SELF_MENTIONS.values().next().value;
    if (oldest !== undefined) NOTIFIED_SELF_MENTIONS.delete(oldest);
  }
  NOTIFIED_SELF_MENTIONS.add(id);
}

// --- Exported group avatar component ------------------------------

interface MessageAvatarProps {
  readonly senderSession: number | null;
  readonly senderName: string;
  readonly avatarUrl: string | undefined;
  readonly user: UserEntry | undefined;
  readonly onAvatarClick?: (session: number) => void;
}

export function MessageAvatar({
  senderSession,
  senderName,
  avatarUrl,
  user,
  onAvatarClick,
}: Readonly<MessageAvatarProps>) {

  const [showCard, setShowCard] = useState(false);
  const [cardPos, setCardPos] = useState<{ top: number; left: number } | null>(null);
  const avatarRef = useRef<HTMLButtonElement>(null);
  const stats = useUserStats(senderSession, showCard);

  const parsed = useMemo(
    () => (user?.comment ? parseComment(user.comment) : null),
    [user?.comment],
  );

  const handleEnter = useCallback(() => {
    if (isMobile || !avatarRef.current) return;
    const rect = avatarRef.current.getBoundingClientRect();
    const rawTop = rect.top + rect.height / 2;
    const top = Math.max(
      HOVER_CARD_H / 2 + HOVER_CARD_MARGIN,
      Math.min(rawTop, window.innerHeight - HOVER_CARD_H / 2 - HOVER_CARD_MARGIN),
    );
    setCardPos({ top, left: rect.right + 8 });
    setShowCard(true);
  }, [isMobile]);

  const handleLeave = useCallback(() => setShowCard(false), []);

  const handleClick = useCallback(() => {
    if (senderSession !== null) onAvatarClick?.(senderSession);
  }, [senderSession, onAvatarClick]);

  const inner = avatarUrl ? (
    <img src={avatarUrl} alt={senderName} className={styles.messageAvatarImg} />
  ) : (
    <div className={styles.messageAvatar} style={{ background: colorFor(senderName) }}>
      {senderName.charAt(0).toUpperCase()}
    </div>
  );

  return (
    <>
      <button
        ref={avatarRef}
        type="button"
        className={styles.avatarBtn}
        onClick={handleClick}
        onMouseEnter={handleEnter}
        onMouseLeave={handleLeave}
        aria-label={`View ${senderName}'s profile`}
      >
        {inner}
      </button>
      {showCard && cardPos && user && createPortal(
        <div className={styles.avatarPopover} style={{ top: cardPos.top, left: cardPos.left }}>
          <ProfilePreviewCard
            profile={parsed?.profile ?? {}}
            bio={parsed?.bio ?? ""}
            avatar={avatarUrl ?? null}
            displayName={user.name}
            onlinesecs={stats?.onlinesecs}
            idlesecs={stats?.idlesecs}
            isRegistered={user.user_id != null && user.user_id > 0}
          />
        </div>,
        document.body,
      )}
    </>
  );
}

/**
 * Returns true when the message body contains only media elements
 * (<img> / <video>) and no visible text.  Used to strip bubble chrome
 * (padding, border, background) so images/GIFs render edge-to-edge.
 */
function isPureMedia(body: string): boolean {
  if (/<!-- FANCY_POLL:/.test(body)) return false;
  if (/<!-- FANCY_FILE:/.test(body)) return false;
  if (/<!-- FANCY_WATCH:/.test(body)) return false;
  if (QUOTE_RE.test(body)) { QUOTE_RE.lastIndex = 0; return false; }
  const hasMedia = /<img|<video/i.test(body);
  if (!hasMedia) return false;
  const textOnly = body
    .replaceAll(/<img[^>]*\/?>/gi, "")
    .replaceAll(/<video[\s\S]*?<\/video>/gi, "")
    .replaceAll(/<!--[\s\S]*?-->/g, "")
    .replaceAll(/<[^>]+>/g, "")
    .trim();
  return textOnly.length === 0;
}

interface MessageItemProps {
  readonly msg: ChatMessage;
  readonly index: number;
  readonly avatarUrl: string | undefined;
  readonly user: UserEntry | undefined;
  readonly polls: Map<string, PollPayload>;
  readonly ownSession: number | null;
  readonly onVote: (pollId: string, selected: number[]) => Promise<void>;
  readonly onAvatarClick?: (session: number) => void;
  /** Preferred time display format (default "auto"). */
  readonly timeFormat?: TimeFormat;
  /** Display timestamps in local timezone (default true). */
  readonly convertToLocalTime?: boolean;
  /** OS-reported clock format for "auto" mode (true = 24h). */
  readonly systemUses24h?: boolean;
  /** Whether the message content is currently being loaded from offload storage. */
  readonly isRestoring?: boolean;
  /** True when this is the first message in a consecutive same-sender group. */
  readonly isFirstInGroup?: boolean;
  /** Callback to scroll to a quoted message. */
  readonly onScrollToMessage?: (messageId: string) => void;
  /** When provided, media clicks call this instead of opening a per-message lightbox. */
  readonly onOpenLightbox?: (src: string) => void;
  /** Optional content rendered at the bottom of the bubble (e.g. reactions). */
  readonly children?: React.ReactNode;
  /** Optional read receipt indicator rendered next to the timestamp on own messages. */
  readonly readReceiptIndicator?: React.ReactNode;
}

export default memo(function MessageItem({
  msg,
  index,
  polls,
  ownSession,
  onVote,
  timeFormat = "auto",
  convertToLocalTime = true,
  systemUses24h,
  isRestoring = false,
  isFirstInGroup = true,
  onScrollToMessage,
  onOpenLightbox,
  children,
  readReceiptIndicator,
}: MessageItemProps) {
  const offloadInfo = extractOffloadInfo(msg.body);
  const offloaded = offloadInfo !== null;
  const pureMedia = !offloaded && isPureMedia(msg.body);

  const linkEmbeds = useAppStore((s) => msg.message_id ? s.linkEmbeds.get(msg.message_id) : undefined);
  const disableLinkPreviews = useAppStore((s) => s.disableLinkPreviews);
  const currentChannel = useAppStore((s) => s.currentChannel);

  const watchMarkerMatch = WATCH_RE.exec(msg.body);
  const watchSessionId = watchMarkerMatch ? watchMarkerMatch[1] : null;
  const watchSessionExists = useAppStore((s) =>
    watchSessionId ? s.watchSessions.has(watchSessionId) : false,
  );

  // Detect whether the receiver is mentioned by this message.  Pure
  // memoised function over the body and own session.
  const selfMention = useMemo(() => {
    if (msg.is_own || ownSession == null) return false;
    return containsSelfMention(msg.body, {
      ownSession,
      isInMessageChannel:
        msg.channel_id != null && currentChannel === msg.channel_id,
    });
  }, [msg.body, msg.is_own, msg.channel_id, ownSession, currentChannel]);

  // Fire the mention notification once per message id.
  useEffect(() => {
    if (!selfMention) return;
    const key = msg.message_id ?? `${msg.sender_session ?? "?"}-${msg.timestamp ?? 0}`;
    if (NOTIFIED_SELF_MENTIONS.has(key)) return;
    // Only notify for recent arrivals (within 30s) to avoid replaying
    // historical notifications when scrolling old messages.
    const ts = msg.timestamp ?? 0;
    if (ts > 0 && Date.now() - ts > 30_000) {
      markSelfMentionNotified(key);
      return;
    }
    markSelfMentionNotified(key);
    globalThis.dispatchEvent(new CustomEvent("fancy:self-mention"));
  }, [selfMention, msg.message_id, msg.sender_session, msg.timestamp]);

  useEffect(() => {
    if (!msg.message_id) return;
    if (disableLinkPreviews) return;
    const urls = extractUrlsFromMessage(msg.body);
    if (urls.length === 0) return;
    requestLinkPreview(urls, msg.message_id);
  }, [msg.message_id, msg.body, disableLinkPreviews]);

  // Always resolve a displayable timestamp: prefer server-side, fall back to local time.
  const displayTimestamp = msg.timestamp ?? Date.now();

  const renderBody = () => {
    if (offloaded || isRestoring) {
      // Estimate skeleton height from the original content byte-length.
      // Images/videos encoded as data-URLs are ~1.37x larger than the
      // decoded pixels, so a rough heuristic of 1 byte ~= 0.003 px
      // gives a decent approximation without knowing the actual
      // dimensions.  Clamp to a reasonable range.
      const contentLen = offloadInfo?.contentLength ?? 0;
      const estimatedHeight = contentLen > 0
        ? Math.max(80, Math.min(Math.round(contentLen * 0.003), 600))
        : 80;

      return (
        <div>
          <div className={styles.skeleton} style={{ minHeight: estimatedHeight }} />
          <span className={styles.skeletonLabel}>
            {isRestoring ? "Decrypting\u2026" : "Content offloaded"}
          </span>
        </div>
      );
    }

    // Extract quote references before other content checks.
    const quoteIds: string[] = [];
    for (const m of msg.body.matchAll(QUOTE_RE)) quoteIds.push(m[1]);
    const bodyWithoutQuotes = quoteIds.length > 0
      ? msg.body.replaceAll(QUOTE_RE, "").trim()
      : msg.body;

    const quoteBlocks = quoteIds.map((id) => (
      <QuoteBlock key={id} messageId={id} onScrollTo={onScrollToMessage} />
    ));

    const pollMatch = /<!-- FANCY_POLL:(.+?) -->/.exec(bodyWithoutQuotes);
    if (pollMatch) {
      const pollId = pollMatch[1];
      const poll = polls.get(pollId) ?? getPoll(pollId);
      if (poll) {
        return (
          <>
            {quoteBlocks}
            <PollCard
              poll={poll}
              ownSession={ownSession}
              isOwn={msg.is_own}
              onVote={onVote}
            />
          </>
        );
      }
    }

    const fileMatch = FANCY_FILE_MARKER_RE.exec(bodyWithoutQuotes);
    if (fileMatch) {
      const info = decodeFileAttachmentPayload(fileMatch[1]);
      if (info) {
        const textBeforeFile = bodyWithoutQuotes.slice(0, fileMatch.index).trim();
        return (
          <>
            {quoteBlocks}
            {textBeforeFile && (
              <MediaPreview html={textBeforeFile} messageId={`${index}-text`} compact={false} timeFormat={timeFormat} convertToLocalTime={convertToLocalTime} systemUses24h={systemUses24h} senderName={msg.sender_name} messageTimestamp={displayTimestamp} onOpenLightbox={onOpenLightbox} />
            )}
            <FileAttachmentCard info={info} />
          </>
        );
      }
    }

    const watchMatch = WATCH_RE.exec(bodyWithoutQuotes);
    if (watchMatch) {
      const watchSessionId = watchMatch[1];
      const textAround = bodyWithoutQuotes.replace(WATCH_RE, "").trim();
      return (
        <>
          {quoteBlocks}
          {textAround && (
            <MediaPreview html={textAround} messageId={`${index}-watch-text`} compact={false} timeFormat={timeFormat} convertToLocalTime={convertToLocalTime} systemUses24h={systemUses24h} senderName={msg.sender_name} messageTimestamp={displayTimestamp} onOpenLightbox={onOpenLightbox} />
          )}
          <WatchTogetherCard sessionId={watchSessionId} />
        </>
      );
    }

    if (quoteBlocks.length > 0 && !bodyWithoutQuotes) {
      return <>{quoteBlocks}</>;
    }

    return (
      <>
        {quoteBlocks}
        <MediaPreview html={bodyWithoutQuotes} messageId={`${index}`} compact={pureMedia} timestamp={pureMedia ? displayTimestamp : undefined} timeFormat={timeFormat} convertToLocalTime={convertToLocalTime} systemUses24h={systemUses24h} senderName={msg.sender_name} messageTimestamp={displayTimestamp} onOpenLightbox={onOpenLightbox} />
      </>
    );
  };

  // A watch-together start message becomes meaningless once the session has
  // ended (the body is only the marker, with no surrounding user text).
  // Hide the entire row so the chat looks as if no message was ever sent.
  if (watchSessionId && !watchSessionExists) {
    return null;
  }

  return (
    <div
      className={`${styles.messageRow} ${msg.is_own ? styles.own : ""} ${msg.pinned ? styles.pinnedRow : ""} ${selfMention ? styles.selfMention : ""}`}
    >
      <div
        className={`${styles.bubble} ${msg.is_own ? styles.ownBubble : ""} ${pureMedia ? styles.bubbleMedia : ""} ${msg.is_legacy ? styles.legacyBubble : ""} ${selfMention ? styles.selfMentionBubble : ""}`}
      >
        {!pureMedia && isFirstInGroup && (
          <span
            className={styles.senderName}
            style={{ color: msg.is_own ? "rgba(255,255,255,0.85)" : colorFor(msg.sender_name) }}
          >
            {msg.sender_name}
            {msg.is_legacy && <span className={styles.legacyBadge}>legacy</span>}
            <time className={styles.messageTime} dateTime={new Date(displayTimestamp).toISOString()}>
              {formatTimestamp(displayTimestamp, timeFormat, convertToLocalTime, systemUses24h)}
            </time>
            {msg.edited_at != null && <span className={styles.editedBadge}>(edited)</span>}
            {msg.pinned && <span className={styles.pinnedBadge}>📌 pinned</span>}
            {msg.is_own && readReceiptIndicator}
          </span>
        )}
        {!pureMedia && !isFirstInGroup && (
          <span className={`${styles.messageTime} ${styles.messageTimeContinuation}`}>
            <time dateTime={new Date(displayTimestamp).toISOString()}>
              {formatTimestamp(displayTimestamp, timeFormat, convertToLocalTime, systemUses24h)}
            </time>
            {msg.edited_at != null && <span className={styles.editedBadge}>(edited)</span>}
            {msg.pinned && <span className={styles.pinnedBadge}>📌 pinned</span>}
            {msg.is_own && readReceiptIndicator}
          </span>
        )}
        <div className={styles.messageBody}>{renderBody()}</div>
        {!offloaded && !isRestoring && !WATCH_RE.test(msg.body) && (
          <WatchStartButton body={msg.body} channelId={msg.channel_id} />
        )}
        {!disableLinkPreviews && linkEmbeds && linkEmbeds.length > 0 && (
          <LinkPreviewCard embeds={linkEmbeds} allowExternalResources />
        )}
        {children}
      </div>
    </div>
  );
});
