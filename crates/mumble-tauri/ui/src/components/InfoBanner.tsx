import type { ReactNode } from "react";
import CloseIcon from "../assets/icons/action/close.svg?react";
import styles from "./InfoBanner.module.css";

interface InfoBannerProps {
  readonly icon?: ReactNode;
  readonly actions?: ReactNode;
  readonly onDismiss?: () => void;
  readonly children: ReactNode;
  readonly variant?: "default" | "danger" | "glass";
}

export function InfoBanner({ icon, actions, onDismiss, children, variant = "default" }: InfoBannerProps) {
  let bannerClass = styles.banner;
  if (variant === "danger") bannerClass = `${styles.banner} ${styles.danger}`;
  else if (variant === "glass") bannerClass = `${styles.banner} ${styles.glass}`;

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
          <CloseIcon width={14} height={14} />
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
