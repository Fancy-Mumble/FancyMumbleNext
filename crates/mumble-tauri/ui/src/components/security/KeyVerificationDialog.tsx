import { CloseIcon } from "../../icons";
import { useState, useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { KeyTrustLevel, KeyFingerprints, PersistenceMode } from "../../types";
import styles from "./KeyVerificationDialog.module.css";

type FingerprintTab = "emoji" | "words" | "hex";

interface KeyVerificationDialogProps {
  readonly channelId: number;
  readonly open: boolean;
  readonly onClose: () => void;
  readonly onVerify: () => Promise<void>;
  readonly trustLevel: KeyTrustLevel;
  readonly channelName: string;
  readonly mode: PersistenceMode;
  readonly distributorName: string;
  readonly distributorHash: string;
}

function trustStatusClass(level: KeyTrustLevel): string {
  switch (level) {
    case "ManuallyVerified":
    case "Verified":
      return styles.trustVerified;
    case "Unverified":
      return styles.trustUnverified;
    case "Disputed":
      return styles.trustDisputed;
  }
}

function trustStatusText(level: KeyTrustLevel): string {
  switch (level) {
    case "ManuallyVerified": return "Manually Verified";
    case "Verified": return "Verified (consensus)";
    case "Unverified": return "Unverified (TOFU)";
    case "Disputed": return "Disputed - conflicting keys";
  }
}

const SHORT_COUNT = 8;

function FingerprintDisplay({
  fingerprints,
  tab,
  showFull,
  onShowFull,
}: Readonly<{
  fingerprints: KeyFingerprints | null;
  tab: FingerprintTab;
  showFull: boolean;
  onShowFull: () => void;
}>) {
  if (!fingerprints) {
    return (
      <div className={styles.fingerprint}>
        <span>Loading...</span>
      </div>
    );
  }

  return (
    <div className={styles.fingerprint}>
      {tab === "emoji" && (
        <div className={styles.emojiFingerprint}>
          {(showFull ? fingerprints.emoji : fingerprints.emoji.slice(0, SHORT_COUNT)).join(" ")}
        </div>
      )}
      {tab === "words" && (
        <div className={styles.wordFingerprint}>
          {(showFull ? fingerprints.words : fingerprints.words.slice(0, SHORT_COUNT)).join(" ")}
        </div>
      )}
      {tab === "hex" && (
        <div className={styles.hexFingerprint}>
          {fingerprints.hex}
        </div>
      )}
      {!showFull && tab !== "hex" && (
        <button className={styles.showFullBtn} onClick={onShowFull}>
          Show full fingerprint
        </button>
      )}
    </div>
  );
}

export default function KeyVerificationDialog({
  channelId,
  open,
  onClose,
  onVerify,
  trustLevel,
  channelName,
  mode,
  distributorName,
  distributorHash,
}: KeyVerificationDialogProps) {
  const [tab, setTab] = useState<FingerprintTab>("emoji");
  const [showFull, setShowFull] = useState(false);
  const [confirmed, setConfirmed] = useState(false);
  const [verifying, setVerifying] = useState(false);
  const [fingerprints, setFingerprints] = useState<KeyFingerprints | null>(null);

  // Fetch fingerprints when dialog opens.
  useEffect(() => {
    if (!open) {
      setFingerprints(null);
      setShowFull(false);
      setConfirmed(false);
      return;
    }
    invoke<KeyFingerprints>("get_key_fingerprints", { channelId, full: false })
      .then(setFingerprints)
      .catch((e) => console.error("get_key_fingerprints error:", e));
  }, [open, channelId]);

  const handleShowFull = useCallback(() => {
    invoke<KeyFingerprints>("get_key_fingerprints", { channelId, full: true })
      .then((fp) => {
        setFingerprints(fp);
        setShowFull(true);
      })
      .catch((e) => console.error("get_key_fingerprints full error:", e));
  }, [channelId]);

  const handleVerify = useCallback(async () => {
    setVerifying(true);
    try {
      await onVerify();
      onClose();
    } finally {
      setVerifying(false);
    }
  }, [onVerify, onClose]);

  // Close on Escape.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;

  const needsVerification = trustLevel === "Unverified" || trustLevel === "Disputed";

  return (
    <dialog className={styles.overlay} open aria-label="Channel Encryption Verification">
      <div className={styles.dialog}>
        <div className={styles.header}>
          <h3 className={styles.title}>Channel Encryption Verification</h3>
          <button className={styles.closeBtn} onClick={onClose} aria-label="Close">
            <CloseIcon width={16} height={16} />
          </button>
        </div>

        <div className={styles.body}>
          <div className={styles.infoRow}>
            <span className={styles.infoLabel}>Channel:</span>
            <span>#{channelName}</span>
          </div>
          <div className={styles.infoRow}>
            <span className={styles.infoLabel}>Mode:</span>
            <span>{mode}</span>
          </div>
          <div className={styles.infoRow}>
            <span className={styles.infoLabel}>Key distributed by:</span>
            <span>{distributorName} ({distributorHash.slice(0, 8)}...)</span>
          </div>

          {/* Fingerprint tabs */}
          <div className={styles.tabs}>
            <button
              className={`${styles.tab} ${tab === "emoji" ? styles.tabActive : ""}`}
              onClick={() => setTab("emoji")}
            >Emoji</button>
            <button
              className={`${styles.tab} ${tab === "words" ? styles.tabActive : ""}`}
              onClick={() => setTab("words")}
            >Words</button>
            <button
              className={`${styles.tab} ${tab === "hex" ? styles.tabActive : ""}`}
              onClick={() => setTab("hex")}
            >Hex</button>
          </div>

          <FingerprintDisplay
            fingerprints={fingerprints}
            tab={tab}
            showFull={showFull}
            onShowFull={handleShowFull}
          />

          <p className={styles.instructions}>
            Compare this fingerprint with a trusted channel member using voice
            chat, in person, or another secure channel.
          </p>

          {/* Current trust status */}
          <div className={`${styles.trustStatus} ${trustStatusClass(trustLevel)}`}>
            Current trust: {trustStatusText(trustLevel)}
          </div>
        </div>

        {/* Verify footer */}
        <div className={styles.footer}>
          <input
            type="checkbox"
            id="verify-confirm"
            className={styles.checkbox}
            checked={confirmed}
            onChange={(e) => setConfirmed(e.target.checked)}
          />
          <label htmlFor="verify-confirm" className={styles.checkboxLabel}>
            I have verified this fingerprint matches
          </label>
          <button
            className={styles.verifyBtn}
            disabled={!confirmed || verifying || !needsVerification}
            onClick={handleVerify}
          >
            {verifying ? "Verifying..." : "Mark as Verified"}
          </button>
        </div>
      </div>
    </dialog>
  );
}
