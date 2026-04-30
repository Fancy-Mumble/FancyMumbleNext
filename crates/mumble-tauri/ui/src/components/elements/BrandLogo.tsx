/**
 * Fancy Mumble brand mark - the rounded gradient square with the
 * letter "M". Used by the welcome wizard, the connect page, and the
 * branded updater bootstrapper window.
 *
 * Self-contained: declares its own gradient and shadow rather than
 * relying on theme variables, so it renders identically in the
 * isolated updater window which loads no global theme.
 */
import type { CSSProperties } from "react";
import styles from "./BrandLogo.module.css";

type BrandLogoProps = Readonly<{
  /** Edge length in pixels. Defaults to 64. */
  size?: number;
  /** Optional className passed through to the root element. */
  className?: string;
}>;

export default function BrandLogo({ size = 64, className }: BrandLogoProps) {
  const style: CSSProperties = {
    width: `${size}px`,
    height: `${size}px`,
    minWidth: `${size}px`,
    minHeight: `${size}px`,
    maxWidth: `${size}px`,
    maxHeight: `${size}px`,
    fontSize: `${Math.round(size * 0.5)}px`,
  };
  const rootClass = className
    ? `${styles.logo} ${className}`
    : styles.logo;
  return (
    <div className={rootClass} style={style} aria-hidden="true">
      <span className={styles.glyph}>M</span>
    </div>
  );
}
