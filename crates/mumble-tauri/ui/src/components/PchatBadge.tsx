import type { PchatProtocol } from "../types";
import styles from "./PchatBadge.module.css";

interface PchatBadgeInfo {
  label: string;
  className: string;
  title: string;
}

const BADGE_MAP: Record<string, PchatBadgeInfo> = {
  fancy_v1_post_join: {
    label: "Fancy",
    className: styles.fancy,
    title: "Fancy E2EE (post-join)",
  },
  fancy_v1_full_archive: {
    label: "Fancy",
    className: styles.fancy,
    title: "Fancy E2EE (full archive)",
  },
  signal_v1: {
    label: "Signal",
    className: styles.signal,
    title: "Signal Protocol encryption",
  },
  server_managed: {
    label: "Server",
    className: styles.server,
    title: "Server-managed persistence",
  },
};

interface PchatBadgeProps {
  readonly protocol: PchatProtocol | undefined;
}

export function PchatBadge({ protocol }: PchatBadgeProps) {
  if (!protocol || protocol === "none") return null;
  const info = BADGE_MAP[protocol];
  if (!info) return null;

  return (
    <span className={`${styles.badge} ${info.className}`} title={info.title}>
      {info.label}
    </span>
  );
}
