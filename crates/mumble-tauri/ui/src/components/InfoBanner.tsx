import type { ReactNode } from "react";
import styles from "./InfoBanner.module.css";

interface InfoBannerProps {
  readonly icon?: ReactNode;
  readonly actions?: ReactNode;
  readonly onDismiss?: () => void;
  readonly children: ReactNode;
  readonly variant?: "default" | "danger";
}

export function InfoBanner({ icon, actions, onDismiss, children, variant = "default" }: InfoBannerProps) {
  const bannerClass = variant === "danger"
    ? `${styles.banner} ${styles.danger}`
    : styles.banner;

  return (
    <div className={bannerClass}>
      {icon && <div className={styles.icon}>{icon}</div>}
      <div className={styles.content}>{children}</div>
      {actions && <div className={styles.actions}>{actions}</div>}
      {onDismiss && (
        <button
          className={styles.closeButton}
          onClick={onDismiss}
          aria-label="Dismiss banner"
        >
          <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor"
            strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <line x1="18" y1="6" x2="6" y2="18" />
            <line x1="6" y1="6" x2="18" y2="18" />
          </svg>
        </button>
      )}
    </div>
  );
}

interface BannerStackProps {
  readonly children: ReactNode;
}

export function BannerStack({ children }: BannerStackProps) {
  return <div className={styles.stack}>{children}</div>;
}
