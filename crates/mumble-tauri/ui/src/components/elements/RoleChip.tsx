import { useMemo } from "react";
import styles from "./RoleChip.module.css";

export interface RoleChipProps {
  /** Display name of the role. */
  readonly name: string;
  /** Optional CSS color string used for the accent background and border. */
  readonly color?: string | null;
  /** Optional raw icon bytes (PNG/JPEG). Rendered as a small avatar. */
  readonly icon?: number[] | null;
  /** Optional size variant. Defaults to `medium`. */
  readonly size?: "small" | "medium" | "large";
  readonly className?: string;
  readonly title?: string;
  readonly onClick?: () => void;
}

function bytesToDataUrl(bytes: number[]): string {
  let binary = "";
  for (const b of bytes) binary += String.fromCharCode(b);
  return `data:image/*;base64,${btoa(binary)}`;
}

/**
 * Reusable colored chip representing a role / channel group. Used in role
 * lists, user member rows, mention previews and similar surfaces.
 */
export function RoleChip({ name, color, icon, size = "medium", className, title, onClick }: RoleChipProps) {
  const iconSrc = useMemo(() => (icon && icon.length > 0 ? bytesToDataUrl(icon) : null), [icon]);
  const sizeCls = size === "small" ? styles.small : size === "large" ? styles.large : "";
  const classes = [
    styles.chip,
    color ? styles.colored : "",
    sizeCls,
    className ?? "",
  ]
    .filter(Boolean)
    .join(" ");

  const style = color ? ({ "--role-color": color } as React.CSSProperties) : undefined;
  const Tag = onClick ? "button" : "span";

  return (
    <Tag
      type={onClick ? "button" : undefined}
      className={classes}
      style={style}
      title={title ?? name}
      onClick={onClick}
    >
      {iconSrc ? (
        <img className={styles.icon} src={iconSrc} alt="" />
      ) : (
        <span className={styles.dot} aria-hidden="true" />
      )}
      <span>{name}</span>
    </Tag>
  );
}
