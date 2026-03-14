import styles from "./ChatView.module.css";

interface ChatHeaderProps {
  readonly channelName: string;
  readonly memberCount: number;
  readonly isInChannel: boolean;
  readonly isDm?: boolean;
  readonly isGroup?: boolean;
  readonly onJoin?: () => void;
  readonly onServerInfoToggle?: () => void;
}

export default function ChatHeader({
  channelName,
  memberCount,
  isInChannel,
  isDm,
  isGroup,
  onJoin,
  onServerInfoToggle,
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
        <span className={styles.memberCount}>{subtitle}</span>
      </div>
      <div className={styles.headerActions}>
        {onServerInfoToggle && !privateBadge && (
          <button
            className={styles.serverInfoBtn}
            onClick={onServerInfoToggle}
            aria-label="Server info"
            title="Server info"
          >
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <circle cx="12" cy="12" r="10" />
              <line x1="12" y1="16" x2="12" y2="12" />
              <line x1="12" y1="8" x2="12.01" y2="8" />
            </svg>
          </button>
        )}
        {!isInChannel && onJoin && (
          <button className={styles.joinBtn} onClick={onJoin}>
            Join Channel
          </button>
        )}
      </div>
    </div>
  );
}
