import { useState, useEffect, type ReactNode } from "react";
import { useAppStore } from "../store";
import type { KeyTrustLevel } from "../types";
import PersistenceBanner from "./PersistenceBanner";
import KeyVerificationDialog from "./KeyVerificationDialog";
import CustodianPrompt from "./CustodianPrompt";

interface PersistentChatResult {
  trustLevel: KeyTrustLevel | undefined;
  onVerifyClick: (() => void) | undefined;
  banner: ReactNode;
  disputeBanner: ReactNode;
  dialogs: ReactNode;
}

/**
 * Hook encapsulating persistent-chat UI state: persistence banner,
 * key verification dialog, and custodian prompt.
 * Extracted from ChatView to reduce component complexity.
 */
export function usePersistentChat(
  channelId: number | null,
  channelName: string,
): PersistentChatResult {
  const channelPersistence = useAppStore((s) => s.channelPersistence);
  const pchatHistoryLoading = useAppStore((s) => s.pchatHistoryLoading);
  const keyTrust = useAppStore((s) => s.keyTrust);
  const custodianPins = useAppStore((s) => s.custodianPins);
  const pendingDisputes = useAppStore((s) => s.pendingDisputes);
  const verifyKeyFingerprint = useAppStore((s) => s.verifyKeyFingerprint);
  const acceptCustodianChanges = useAppStore((s) => s.acceptCustodianChanges);
  const confirmCustodians = useAppStore((s) => s.confirmCustodians);

  const [showVerifyDialog, setShowVerifyDialog] = useState(false);
  const [showCustodianPrompt, setShowCustodianPrompt] = useState(false);

  const persistence = channelId === null ? undefined : channelPersistence[channelId];
  const isLoading = channelId !== null && pchatHistoryLoading.has(channelId);
  const trust = channelId === null ? undefined : keyTrust[channelId];
  const custodian = channelId === null ? undefined : custodianPins[channelId];
  const dispute = channelId === null ? undefined : pendingDisputes[channelId];

  // Auto-show custodian prompt when unconfirmed or pending changes detected.
  useEffect(() => {
    if (!custodian) return;
    if (!custodian.confirmed || custodian.pendingUpdate) {
      setShowCustodianPrompt(true);
    }
  }, [custodian]);

  const showBanner = channelId !== null && (
    (persistence && persistence.mode !== "NONE") || isLoading
  );

  return {
    trustLevel: trust?.trustLevel,
    onVerifyClick: trust ? () => setShowVerifyDialog(true) : undefined,
    banner: showBanner ? (
      <PersistenceBanner channelId={channelId} />
    ) : null,
    disputeBanner: dispute ? (
      <div style={{
        padding: "8px 16px",
        background: "rgba(231, 76, 60, 0.12)",
        borderBottom: "1px solid rgba(231, 76, 60, 0.3)",
        color: "var(--color-text-secondary)",
        fontSize: "0.85rem",
      }}>
        Conflicting encryption keys detected.{" "}
        <button
          onClick={() => setShowVerifyDialog(true)}
          style={{ background: "none", border: "none", color: "var(--color-accent)", textDecoration: "underline", cursor: "pointer", padding: 0, fontSize: "inherit" }}
        >
          Compare fingerprints
        </button>{" "}to resolve.
      </div>
    ) : null,
    dialogs: (
      <>
        {trust && channelId !== null && (
          <KeyVerificationDialog
            channelId={channelId}
            open={showVerifyDialog}
            onClose={() => setShowVerifyDialog(false)}
          onVerify={() => verifyKeyFingerprint(channelId)}
          trustLevel={trust.trustLevel}
          channelName={channelName}
          mode={persistence?.mode ?? "NONE"}
          distributorName={trust.distributorName}
          distributorHash={trust.distributorHash}
        />
        )}
        {custodian && channelId !== null && (
          <CustodianPrompt
            open={showCustodianPrompt}
            onClose={() => setShowCustodianPrompt(false)}
            onConfirm={() =>
              custodian.pendingUpdate
                ? acceptCustodianChanges(channelId)
                : confirmCustodians(channelId)
            }
            custodians={custodian.pinned.map((h) => ({ hash: h }))}
            isFirstJoin={!custodian.confirmed && !custodian.pendingUpdate}
            addedCustodians={
              custodian.pendingUpdate
                ?.filter((h) => !custodian.pinned.includes(h))
                .map((h) => ({ hash: h }))
            }
            removedCustodians={
              custodian.pendingUpdate
                ? custodian.pinned
                    .filter((h) => {
                      const pending = custodian.pendingUpdate;
                      return pending ? !pending.includes(h) : false;
                    })
                    .map((h) => ({ hash: h }))
                : undefined
            }
          />
        )}
      </>
    ),
  };
}
