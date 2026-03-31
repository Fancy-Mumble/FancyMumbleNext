import { isMobile } from "../../utils/platform";
import type { KeyTrustLevel } from "../../types";
import KeyTrustIndicator from "../security/KeyTrustIndicator";
import KebabMenu, { type KebabMenuItem } from "../elements/KebabMenu";
import PollIcon from "../../assets/icons/communication/poll.svg?react";
import BellIcon from "../../assets/icons/status/bell.svg?react";
import BellOffIcon from "../../assets/icons/status/bell-off.svg?react";
import styles from "./ChatView.module.css";
import UsersGroupIcon from "../../assets/icons/user/users-group.svg?react";
import DatabaseIcon from "../../assets/icons/general/database.svg?react";
import SearchIcon from "../../assets/icons/action/search.svg?react";
import FolderIcon from "../../assets/icons/general/folder.svg?react";

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
  readonly onPollCreate?: () => void;
  readonly isSilenced?: boolean;
  readonly onToggleSilence?: () => void;
}

function buildKebabItems({
  onPollCreate,
  isSilenced,
  onToggleSilence,
}: Pick<ChatHeaderProps, "onPollCreate" | "isSilenced" | "onToggleSilence">): KebabMenuItem[] {
  const items: KebabMenuItem[] = [];
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
  isGroup,
  isPersisted,
  onJoin,
  onChannelInfoToggle,
  onChannelSearch,
  keyTrustLevel,
  onVerifyClick,
  onPollCreate,
  isSilenced,
  onToggleSilence,
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
        {!isMobile && (<span className={styles.memberCount}>{subtitle}</span>)}
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
        {!privateBadge && (
          <KebabMenu
            items={buildKebabItems({ onPollCreate, isSilenced, onToggleSilence })}
            ariaLabel="Channel options"
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
