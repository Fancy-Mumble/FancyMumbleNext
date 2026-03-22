import type { ReactNode } from "react";
import styles from "./TabbedPage.module.css";

export interface TabDef<T extends string> {
  id: T;
  label: string;
  icon: string;
}

interface TabbedPageProps<T extends string> {
  heading: string;
  tabs: readonly TabDef<T>[];
  activeTab: T;
  onTabChange: (tab: T) => void;
  onBack: () => void;
  /** Extra CSS class applied to `.mainArea` (e.g. grid layout for preview pane). */
  mainAreaClassName?: string;
  children: ReactNode;
}

const BackIcon = (
  <svg
    width="18"
    height="18"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    <polyline points="15 18 9 12 15 6" />
  </svg>
);

export function TabbedPage<T extends string>({
  heading,
  tabs,
  activeTab,
  onTabChange,
  onBack,
  mainAreaClassName,
  children,
}: Readonly<TabbedPageProps<T>>) {
  const mainCls = mainAreaClassName
    ? `${styles.mainArea} ${mainAreaClassName}`
    : styles.mainArea;

  return (
    <div className={styles.page}>
      <nav className={styles.sidebar}>
        <button
          className={styles.backBtn}
          onClick={onBack}
          aria-label="Go back"
        >
          {BackIcon}
          <span>Back</span>
        </button>

        <h2 className={styles.sidebarHeading}>{heading}</h2>

        <ul className={styles.tabList}>
          {tabs.map((t) => (
            <li key={t.id}>
              <button
                className={`${styles.tabBtn} ${activeTab === t.id ? styles.tabBtnActive : ""}`}
                onClick={() => onTabChange(t.id)}
              >
                <span className={styles.tabIcon}>{t.icon}</span>
                {t.label}
              </button>
            </li>
          ))}
        </ul>
      </nav>

      <div className={mainCls}>
        {children}
      </div>
    </div>
  );
}
