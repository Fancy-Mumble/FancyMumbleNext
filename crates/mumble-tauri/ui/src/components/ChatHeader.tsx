import styles from "./ChatView.module.css";

interface ChatHeaderProps {
  readonly channelName: string;
  readonly memberCount: number;
  readonly isInChannel: boolean;
  readonly onJoin: () => void;
}

export default function ChatHeader({
  channelName,
  memberCount,
  isInChannel,
  onJoin,
}: ChatHeaderProps) {
  return (
    <div className={styles.header}>
      <div className={styles.headerInfo}>
        <h2 className={styles.channelName}># {channelName}</h2>
        <span className={styles.memberCount}>{memberCount} members</span>
      </div>
      {!isInChannel && (
        <button className={styles.joinBtn} onClick={onJoin}>
          Join Channel
        </button>
      )}
    </div>
  );
}
