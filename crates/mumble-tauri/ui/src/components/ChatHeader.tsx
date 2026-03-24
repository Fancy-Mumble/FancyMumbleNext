import { isMobilePlatform } from "../utils/platform";
import type { KeyTrustLevel } from "../types";
import KeyTrustIndicator from "./KeyTrustIndicator";
import styles from "./ChatView.module.css";
import UsersGroupIcon from "../assets/icons/user/users-group.svg?react";
import DatabaseIcon from "../assets/icons/general/database.svg?react";
import SearchIcon from "../assets/icons/action/search.svg?react";
import FolderIcon from "../assets/icons/general/folder.svg?react";

interface ChatHeaderProps {
  readonly channelName: string;
  readonly memberCount: number;
  readonly isInChannel: boolean;
  readonly isDm?: boolean;
  readonly isGroup?: boolean;
  readonly isPersisted?: boolean;
  readonly onJoin?: () => void;
  readonly onChannelInfoToggle?: () => void;
  readonly onChannelSearch?: () => void;
  readonly keyTrustLevel?: KeyTrustLevel;
  readonly onVerifyClick?: () => void;
}

export default function ChatHeader({
  channelName,
  memberCount,
  isInChannel,
  isDm,
  isGroup,
  isPersisted,
  onJoin,
  onChannelInfoToggle,
  onChannelSearch,
  keyTrustLevel,
  onVerifyClick,
}: ChatHeaderProps) {
  let prefix: string;
  if (isGroup) prefix = "";
  else if (isDm) prefix = "@";
  else prefix = "#";

  let subtitle: string;
  if (isGroup) subtitle = `${memberCount} ${memberCount === 1 ? "member" : "members"}`;
  else if (isDm) subtitle = "Direct Message";
  else subtitle = `${memberCount} members`;

  const privateBadge = isDm || isGroup;

  return (
    <div className={styles.header}>
      <div className={styles.headerInfo}>
        <h2 className={styles.channelName}>
          {isGroup && (
            <UsersGroupIcon width={18} height={18} style={{ marginRight: 6, verticalAlign: "text-bottom" }} />
          )}
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
        {!isMobilePlatform() && (<span className={styles.memberCount}>{subtitle}</span>)}
      </div>
      <div className={styles.headerActions}>
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
        {!isInChannel && onJoin && (
          <button className={styles.joinBtn} onClick={onJoin}>
            {isMobilePlatform() ? "Join" : "Join Channel"}
          </button>
        )}
      </div>
    </div>
  );
}
