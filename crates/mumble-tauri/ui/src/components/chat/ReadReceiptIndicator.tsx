import { useMemo } from "react";
import { useAppStore } from "../../store";
import { allActiveUsersRead, getReadersForMessage } from "./readReceiptStore";
import styles from "./ReadReceiptIndicator.module.css";

interface ReadReceiptIndicatorProps {
  readonly messageId: string;
  readonly channelId: number;
  readonly allMessageIds: string[];
}

export default function ReadReceiptIndicator({
  messageId,
  channelId,
  allMessageIds,
}: ReadReceiptIndicatorProps) {
  const readReceiptVersion = useAppStore((s) => s.readReceiptVersion);
  const users = useAppStore((s) => s.users);
  const ownSession = useAppStore((s) => s.ownSession);

  const ownHash = useMemo(
    () => users.find((u) => u.session === ownSession)?.hash,
    [users, ownSession],
  );

  const activeHashes = useMemo(
    () =>
      users
        .filter((u) => u.channel_id === channelId && u.hash)
        .map((u) => u.hash!),
    [users, channelId],
  );

  const allRead = useMemo(
    () =>
      allActiveUsersRead(
        channelId,
        messageId,
        allMessageIds,
        activeHashes,
        ownHash,
      ),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [channelId, messageId, allMessageIds, activeHashes, ownHash, readReceiptVersion],
  );

  const readerCount = useMemo(
    () => {
      const readers = getReadersForMessage(channelId, messageId, allMessageIds);
      return ownHash
        ? readers.filter((r) => r.cert_hash !== ownHash).length
        : readers.length;
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [channelId, messageId, allMessageIds, ownHash, readReceiptVersion],
  );

  const singleCheck = (
    <path
      d="M11.071.653a.457.457 0 0 0-.304-.102.493.493 0 0 0-.381.178l-6.19 7.636-2.405-2.272a.463.463 0 0 0-.336-.146.47.47 0 0 0-.343.146l-.311.31a.445.445 0 0 0-.14.337c0 .136.046.249.14.337l2.995 2.83a.63.63 0 0 0 .448.186h.065a.63.63 0 0 0 .416-.186l6.646-8.09a.42.42 0 0 0 .108-.299.453.453 0 0 0-.108-.298l-.3-.267z"
      fill="currentColor"
    />
  );

  if (readerCount === 0) {
    return (
      <span className={styles.indicator} title="Sent">
        <svg width="16" height="11" viewBox="0 0 16 11" aria-label="Sent">
          {singleCheck}
        </svg>
      </span>
    );
  }

  if (!allRead) {
    return (
      <span className={`${styles.indicator} ${styles.read}`} title={`Read by ${readerCount}`}>
        <svg width="16" height="11" viewBox="0 0 16 11" aria-label="Read">
          {singleCheck}
        </svg>
      </span>
    );
  }

  return (
    <span className={`${styles.indicator} ${styles.read}`} title="Read by everyone">
      <svg width="16" height="11" viewBox="0 0 16 11" aria-label="Read by everyone">
        {singleCheck}
        <path
          d="M15.071.653a.457.457 0 0 0-.304-.102.493.493 0 0 0-.381.178l-6.19 7.636-1.143-1.08-.255.312.695.657a.63.63 0 0 0 .448.186h.065a.63.63 0 0 0 .416-.186l6.646-8.09a.42.42 0 0 0 .108-.299.453.453 0 0 0-.108-.298l-.3-.267z"
          fill="currentColor"
          transform="translate(-1.5, 0)"
        />
      </svg>
    </span>
  );
}
