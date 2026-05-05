import { BellIcon, BellOffIcon, CloseIcon, DatabaseIcon, FolderIcon, PollIcon, ScreenShareIcon, SearchIcon, UsersGroupIcon } from "../../icons";
import { isMobile } from "../../utils/platform";
import type { KeyTrustLevel } from "../../types";
import KeyTrustIndicator from "../security/KeyTrustIndicator";
import KebabMenu, { type KebabMenuItem } from "../elements/KebabMenu";
import styles from "./ChatView.module.css";
import { colorFor } from "../sidebar/UserListItem";

/** Info about the active broadcast, passed in when streaming is active. */
export interface BroadcastInfo {
  /** Name of the broadcaster. */
  broadcasterName: string;
  /** Avatar data URL (or null for initial-based avatar). */
  avatarUrl: string | null;
  /** Number of viewers in the channel (excluding the broadcaster). */
  viewerCount: number;
  /** Whether the current user is the broadcaster. */
  isOwnBroadcast: boolean;
  /** Channel name the broadcast is happening in. */
  channelName: string;
  /** Called when the user clicks the close/stop button in the stream header. */
  onClose: () => void;
}

interface ChatHeaderProps {
  readonly channelName: string;
  readonly memberCount: number;
  readonly isInChannel: boolean;
  readonly isDm?: boolean;
  readonly isPersisted?: boolean;
  readonly onJoin?: () => void;
  readonly onChannelInfoToggle?: () => void;
  readonly onChannelSearch?: () => void;
  readonly keyTrustLevel?: KeyTrustLevel;
  readonly onVerifyClick?: () => void;
  readonly onPollCreate?: () => void;
  readonly isSilenced?: boolean;
  readonly onToggleSilence?: () => void;
  readonly isScreenSharing?: boolean;
  readonly onToggleScreenShare?: () => void;
  /** True when the server has a WebRTC SFU module for server-relayed screen sharing. */
  readonly sfuAvailable?: boolean;
  /** When a stream is active, display broadcast info in the header. */
  readonly broadcastInfo?: BroadcastInfo;
  /** Whether there are unseen pin changes (shows red dot on kebab & menu item). */
  readonly hasNewPins?: boolean;
  /** Called when the user opens the pinned messages panel. */
  readonly onPinnedMessages?: () => void;
  /** Whether the user has unseen completed downloads. */
  readonly hasNewDownloads?: boolean;
  /** Called when the user opens the downloads panel. */
  readonly onDownloads?: () => void;
}

function buildKebabItems({
  onPollCreate,
  isSilenced,
  onToggleSilence,
  hasNewPins,
  onPinnedMessages,
  hasNewDownloads,
  onDownloads,
}: Pick<ChatHeaderProps, "onPollCreate" | "isSilenced" | "onToggleSilence" | "hasNewPins" | "onPinnedMessages" | "hasNewDownloads" | "onDownloads">): KebabMenuItem[] {
  const items: KebabMenuItem[] = [];
  if (onPinnedMessages) {
    items.push({
      id: "pinned-messages",
      label: "Pinned messages",
      icon: <span style={{ fontSize: 15, lineHeight: 1 }}>📌</span>,
      badge: hasNewPins,
      onClick: onPinnedMessages,
    });
  }
  if (onDownloads) {
    items.push({
      id: "downloads",
      label: "Downloads",
      icon: <FolderIcon width={16} height={16} />,
      badge: hasNewDownloads,
      onClick: onDownloads,
    });
  }
  if (onPollCreate) {
    items.push({
      id: "create-poll",
      label: "Create poll",
      icon: <PollIcon width={16} height={16} />,
      onClick: onPollCreate,
    });
  }
  if (onToggleSilence) {
    items.push({
      id: "toggle-silence",
      label: isSilenced ? "Unmute channel" : "Mute channel",
      icon: isSilenced
        ? <BellIcon width={16} height={16} />
        : <BellOffIcon width={16} height={16} />,
      active: isSilenced,
      onClick: onToggleSilence,
    });
  }
  return items;
}

export default function ChatHeader({
  channelName,
  memberCount,
  isInChannel,
  isDm,
  isPersisted,
  onJoin,
  onChannelInfoToggle,
  onChannelSearch,
  keyTrustLevel,
  onVerifyClick,
  onPollCreate,
  isSilenced,
  onToggleSilence,
  isScreenSharing,
  onToggleScreenShare,
  sfuAvailable,
  broadcastInfo,
  hasNewPins,
  onPinnedMessages,
  hasNewDownloads,
  onDownloads,
}: ChatHeaderProps) {
  const prefix = isDm ? "@" : "#";
  const subtitle = isDm ? "Direct Message" : `${memberCount} members`;

  const privateBadge = isDm;
  const isStreaming = !!broadcastInfo;

  return (
    <div className={`${styles.header} ${isStreaming ? styles.headerStreaming : ""}`}>
      {/* Broadcaster info (replaces channel info when streaming) */}
      {isStreaming ? (
        <div className={styles.headerInfo}>
          <div className={styles.broadcasterRow}>
            <div
              className={styles.broadcasterAvatar}
              style={{
                background: broadcastInfo.avatarUrl
                  ? "transparent"
                  : colorFor(broadcastInfo.broadcasterName),
              }}
            >
              {broadcastInfo.avatarUrl ? (
                <img
                  src={broadcastInfo.avatarUrl}
                  alt={broadcastInfo.broadcasterName}
                  className={styles.broadcasterAvatarImg}
                />
              ) : (
                broadcastInfo.broadcasterName.charAt(0).toUpperCase()
              )}
            </div>
            <div className={styles.broadcasterMeta}>
              <span className={styles.broadcasterName}>
                {broadcastInfo.isOwnBroadcast ? "You" : broadcastInfo.broadcasterName}
                <span className={styles.broadcasterChannel}> - {broadcastInfo.channelName}</span>
              </span>
              <span className={styles.broadcastLabel}>
                <span className={styles.liveDot} />
                Screen sharing
              </span>
            </div>
          </div>
        </div>
      ) : (
        <div className={styles.headerInfo}>
          <h2 className={styles.channelName}>
            {prefix} {channelName}
            {isPersisted && (
              <DatabaseIcon
                className={styles.persistedIcon}
                width={14}
                height={14}
                aria-label="Persistent chat"
              >
                <title>Messages in this channel are stored on the server</title>
              </DatabaseIcon>
            )}
          </h2>
          {!isMobile && (<span className={styles.memberCount}>{subtitle}</span>)}
        </div>
      )}

      <div className={styles.headerActions}>
        {/* Viewer count (when streaming, shown on the right) */}
        {isStreaming && (
          <span className={styles.viewerCount}>
            <UsersGroupIcon width={14} height={14} />
            {broadcastInfo.viewerCount}
          </span>
        )}
        {keyTrustLevel && !privateBadge && (
          <KeyTrustIndicator
            trustLevel={keyTrustLevel}
            onVerifyClick={onVerifyClick}
          />
        )}
        {onChannelSearch && !privateBadge && (
          <button
            className={styles.serverInfoBtn}
            onClick={onChannelSearch}
            aria-label="Search in channel"
            title="Search in channel"
          >
            <SearchIcon width={18} height={18} />
          </button>
        )}
        {onChannelInfoToggle && !privateBadge && (
          <button
            className={styles.serverInfoBtn}
            onClick={onChannelInfoToggle}
            aria-label="Channel info"
            title="Channel info"
          >
            <FolderIcon width={18} height={18} />
          </button>
        )}
        {onToggleScreenShare && !privateBadge && !broadcastInfo?.isOwnBroadcast && (
          <button
            className={`${styles.serverInfoBtn} ${isScreenSharing ? styles.screenShareActive : ""}`}
            onClick={onToggleScreenShare}
            aria-label={isScreenSharing ? "Stop sharing" : "Share screen"}
            title={
              isScreenSharing
                ? "Stop sharing"
                : sfuAvailable
                  ? "Share screen (server-relayed)"
                  : "Share screen (no SFU - peer-to-peer)"
            }
          >
            <ScreenShareIcon width={18} height={18} />
          </button>
        )}
        {/* Stream close button (when streaming, replaces the toggle) */}
        {isStreaming && (
          <button
            className={styles.streamCloseBtn}
            onClick={broadcastInfo.onClose}
            title={broadcastInfo.isOwnBroadcast ? "Stop sharing" : "Close stream"}
            aria-label={broadcastInfo.isOwnBroadcast ? "Stop sharing" : "Close stream"}
          >
            <CloseIcon width={16} height={16} />
          </button>
        )}
        {!privateBadge && (
          <KebabMenu
            items={buildKebabItems({ onPollCreate, isSilenced, onToggleSilence, hasNewPins, onPinnedMessages, hasNewDownloads, onDownloads })}
            ariaLabel="Channel options"
            badge={hasNewPins || hasNewDownloads}
          />
        )}
        {!isInChannel && onJoin && (
          <button className={styles.joinBtn} onClick={onJoin}>
            {isMobile ? "Join" : "Join Channel"}
          </button>
        )}
      </div>
    </div>
  );
}
