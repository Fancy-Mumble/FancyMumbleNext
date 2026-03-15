import { isMobilePlatform } from "../utils/platform";
import styles from "./ChatView.module.css";

interface ChatHeaderProps {
  readonly channelName: string;
  readonly memberCount: number;
  readonly isInChannel: boolean;
  readonly isDm?: boolean;
  readonly isGroup?: boolean;
  readonly onJoin?: () => void;
  readonly onChannelInfoToggle?: () => void;
}

export default function ChatHeader({
  channelName,
  memberCount,
  isInChannel,
  isDm,
  isGroup,
  onJoin,
  onChannelInfoToggle,
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
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{ marginRight: 6, verticalAlign: "text-bottom" }}>
              <path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2" />
              <circle cx="9" cy="7" r="4" />
              <path d="M23 21v-2a4 4 0 0 0-3-3.87" />
              <path d="M16 3.13a4 4 0 0 1 0 7.75" />
            </svg>
          )}
          {prefix} {channelName}
        </h2>
        {!isMobilePlatform() && (<span className={styles.memberCount}>{subtitle}</span>)}
      </div>
      <div className={styles.headerActions}>
        {onChannelInfoToggle && !privateBadge && (
          <button
            className={styles.serverInfoBtn}
            onClick={onChannelInfoToggle}
            aria-label="Channel info"
            title="Channel info"
          >
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.93a2 2 0 0 1-1.66-.9l-.82-1.2A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13c0 1.1.9 2 2 2Z" />
            </svg>
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
