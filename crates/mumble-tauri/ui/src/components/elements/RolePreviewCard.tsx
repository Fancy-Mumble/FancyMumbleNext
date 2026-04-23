import { RoleChip } from "./RoleChip";
import styles from "./RolePreviewCard.module.css";

export interface RolePreviewCardProps {
  readonly name: string;
  readonly color?: string | null;
  readonly icon?: number[] | null;
  /** Sample username used in the rendered preview. */
  readonly sampleUsername?: string;
}

/**
 * Displays a small live preview of how a role's customization looks in chat:
 * the chip itself, a sample username with the role color applied and a
 * sample mention bubble.
 */
export function RolePreviewCard({ name, color, icon, sampleUsername = "Sample User" }: RolePreviewCardProps) {
  const style = color ? ({ "--role-color": color } as React.CSSProperties) : undefined;
  return (
    <div className={styles.card} style={style}>
      <span className={styles.title}>Preview</span>
      <div className={styles.row}>
        <RoleChip name={name || "Role"} color={color} icon={icon} size="large" />
      </div>
      <div className={styles.row}>
        <span className={styles.usernameSample} data-has-color={Boolean(color)}>
          {sampleUsername}
          <span className={styles.handle}>online</span>
        </span>
      </div>
      <div className={styles.bubble}>
        Hey <span className={styles.mention}>@{name || "role"}</span>, can you take a look?
      </div>
    </div>
  );
}
