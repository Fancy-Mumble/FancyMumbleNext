import { useState, useRef, useEffect } from "react";
import type { KeyTrustLevel } from "../../types";
import ShieldCheckSvg from "../../assets/icons/status/shield-check.svg?react";
import LockSvg from "../../assets/icons/status/lock.svg?react";
import WarningSvg from "../../assets/icons/status/warning.svg?react";
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
  return <ShieldCheckSvg className={styles.icon} aria-hidden="true" />;
}

function LockIcon() {
  return <LockSvg className={styles.icon} aria-hidden="true" />;
}

function WarningIcon() {
  return <WarningSvg className={styles.icon} aria-hidden="true" />;
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
