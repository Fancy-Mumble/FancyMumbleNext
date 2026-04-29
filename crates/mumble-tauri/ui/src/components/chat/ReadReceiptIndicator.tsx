import { CheckDoubleIcon, CheckSingleIcon } from "../../icons";
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

  if (readerCount === 0) {
    return (
      <span className={styles.indicator} title="Sent">
        <CheckSingleIcon width={16} height={11} aria-label="Sent" />
      </span>
    );
  }

  if (!allRead) {
    return (
      <span className={`${styles.indicator} ${styles.read}`} title={`Read by ${readerCount}`}>
        <CheckSingleIcon width={16} height={11} aria-label="Read" />
      </span>
    );
  }

  return (
    <span className={`${styles.indicator} ${styles.read}`} title="Read by everyone">
      <CheckDoubleIcon width={16} height={11} aria-label="Read by everyone" />
    </span>
  );
}
