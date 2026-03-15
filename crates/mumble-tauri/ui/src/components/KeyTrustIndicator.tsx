import { useState, useRef, useEffect } from "react";
import type { KeyTrustLevel } from "../types";
import styles from "./KeyTrustIndicator.module.css";

interface KeyTrustIndicatorProps {
  readonly trustLevel: KeyTrustLevel;
  readonly onVerifyClick?: () => void;
}

function trustLabel(level: KeyTrustLevel): string {
  switch (level) {
    case "ManuallyVerified": return "Verified";
    case "Verified": return "Verified";
    case "Unverified": return "Unverified";
    case "Disputed": return "Disputed";
  }
}

function trustDescription(level: KeyTrustLevel): string {
  switch (level) {
    case "ManuallyVerified":
      return "This channel's encryption key has been manually verified via out-of-band comparison.";
    case "Verified":
      return "This channel's encryption key has been verified through multi-confirmation consensus or key custodian endorsement.";
    case "Unverified":
      return "This channel's encryption key has not been verified. You are protected against passive eavesdropping, but an active attacker could intercept your messages.";
    case "Disputed":
      return "Conflicting encryption keys detected. Verify with a trusted member to resolve.";
  }
}

function trustColorClass(level: KeyTrustLevel): string {
  switch (level) {
    case "ManuallyVerified": return styles.manuallyVerified;
    case "Verified": return styles.verified;
    case "Unverified": return styles.unverified;
    case "Disputed": return styles.disputed;
  }
}

function ShieldCheckIcon() {
  return (
    <svg className={styles.icon} viewBox="0 0 24 24" fill="none" stroke="currentColor"
      strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
      <path d="M9 12l2 2 4-4" />
    </svg>
  );
}

function LockIcon() {
  return (
    <svg className={styles.icon} viewBox="0 0 24 24" fill="none" stroke="currentColor"
      strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="3" y="11" width="18" height="11" rx="2" ry="2" />
      <path d="M7 11V7a5 5 0 0 1 10 0v4" />
    </svg>
  );
}

function WarningIcon() {
  return (
    <svg className={styles.icon} viewBox="0 0 24 24" fill="none" stroke="currentColor"
      strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" />
      <line x1="12" y1="9" x2="12" y2="13" />
      <line x1="12" y1="17" x2="12.01" y2="17" />
    </svg>
  );
}

function TrustIcon({ level }: Readonly<{ level: KeyTrustLevel }>) {
  switch (level) {
    case "ManuallyVerified": return <ShieldCheckIcon />;
    case "Verified": return <LockIcon />;
    case "Unverified": return <LockIcon />;
    case "Disputed": return <WarningIcon />;
  }
}

export default function KeyTrustIndicator({ trustLevel, onVerifyClick }: KeyTrustIndicatorProps) {
  const [showTooltip, setShowTooltip] = useState(false);
  const wrapperRef = useRef<HTMLDivElement>(null);

  // Close tooltip on outside click.
  useEffect(() => {
    if (!showTooltip) return;
    const handleClick = (e: MouseEvent) => {
      if (wrapperRef.current && !wrapperRef.current.contains(e.target as Node)) {
        setShowTooltip(false);
      }
    };
    document.addEventListener("click", handleClick, true);
    return () => document.removeEventListener("click", handleClick, true);
  }, [showTooltip]);

  const colorClass = trustColorClass(trustLevel);

  return (
    <div className={styles.wrapper} ref={wrapperRef}>
      <button
        className={`${styles.indicator} ${colorClass}`}
        onClick={() => setShowTooltip((v) => !v)}
        aria-label={`Encryption: ${trustLabel(trustLevel)}`}
        title={`Encryption: ${trustLabel(trustLevel)}`}
      >
        <TrustIcon level={trustLevel} />
        <span className={styles.label}>{trustLabel(trustLevel)}</span>
      </button>

      {showTooltip && (
        <div className={styles.tooltip}>
          <div className={styles.tooltipTitle}>Channel Encryption</div>
          <p>{trustDescription(trustLevel)}</p>
          {(trustLevel === "Unverified" || trustLevel === "Disputed") && onVerifyClick && (
            <p>
              <button className={styles.tooltipAction} onClick={() => { setShowTooltip(false); onVerifyClick(); }}>
                {trustLevel === "Disputed" ? "Compare fingerprints" : "Verify with a key custodian"}
              </button>
              {" "}to turn this indicator green.
            </p>
          )}
        </div>
      )}
    </div>
  );
}
