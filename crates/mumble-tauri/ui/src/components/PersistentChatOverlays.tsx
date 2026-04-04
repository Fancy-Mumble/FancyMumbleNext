import { useState, useEffect, useCallback, type ReactNode } from "react";
import { useAppStore } from "../store";
import type { KeyTrustLevel, PendingKeyShareRequest, PersistenceMode, UserMode } from "../types";
import { getPreferences } from "../preferencesStorage";
import PersistenceBanner from "./PersistenceBanner";
import { InfoBanner } from "./InfoBanner";
import infoBannerStyles from "./InfoBanner.module.css";
import KeyVerificationDialog from "./KeyVerificationDialog";
import CustodianPrompt from "./CustodianPrompt";
import KeyShareWarningDialog from "./KeyShareWarningDialog";
import KeyIcon from "../assets/icons/status/key.svg?react";
import WarningIcon from "../assets/icons/status/warning.svg?react";

interface PersistentChatResult {
  trustLevel: KeyTrustLevel | undefined;
  onVerifyClick: (() => void) | undefined;
  isPersisted: boolean;
  banner: ReactNode;
  signalBridgeErrorBanner: ReactNode;
  disputeBanner: ReactNode;
  keyShareBanner: ReactNode;
  revokedBanner: ReactNode;
  keyRevoked: boolean;
  dialogs: ReactNode;
}

const keyIcon = <KeyIcon aria-hidden="true" />;

const warningIcon = <WarningIcon aria-hidden="true" />;

function buildKeyShareBanner(
  channelId: number | null,
  requests: PendingKeyShareRequest[],
  onShareClick: (peerCertHash: string, peerName: string) => void,
  onDismiss: (channelId: number, hash: string) => void,
): ReactNode {
  if (channelId === null || requests.length === 0) return null;
  return (
    <>
      {requests.map((req) => (
        <InfoBanner
          key={req.peer_cert_hash}
          variant="glass"
          icon={keyIcon}
          actions={
            <button
              className={infoBannerStyles.approveButton}
              onClick={() => onShareClick(req.peer_cert_hash, req.peer_name)}
            >
              Share Key
            </button>
          }
          onDismiss={() => onDismiss(channelId, req.peer_cert_hash)}
        >
          <p className={infoBannerStyles.description}>
            <strong>{req.peer_name}</strong> joined and needs the encryption key.
          </p>
        </InfoBanner>
      ))}
    </>
  );
}

function buildDisputeBanner(
  onCompareClick: () => void,
): ReactNode {
  return (
    <InfoBanner
      variant="danger"
      icon={warningIcon}
      actions={
        <button className={infoBannerStyles.dangerAction} onClick={onCompareClick}>
          Compare fingerprints
        </button>
      }
    >
      <p className={infoBannerStyles.description}>
        Conflicting encryption keys detected.
      </p>
    </InfoBanner>
  );
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
  const pendingKeyShares = useAppStore((s) => s.pendingKeyShares);
  const approveKeyShare = useAppStore((s) => s.approveKeyShare);
  const dismissKeyShare = useAppStore((s) => s.dismissKeyShare);
  const pchatKeyRevoked = useAppStore((s) => s.pchatKeyRevoked);

  const [showVerifyDialog, setShowVerifyDialog] = useState(false);
  const [showCustodianPrompt, setShowCustodianPrompt] = useState(false);
  const [keyShareConfirm, setKeyShareConfirm] = useState<{ hash: string; name: string } | null>(null);
  const [userMode, setUserMode] = useState<UserMode>("normal");

  useEffect(() => {
    getPreferences().then((p) => setUserMode(p.userMode));
  }, []);

  const persistence = channelId === null ? undefined : channelPersistence[channelId];
  const isLoading = channelId !== null && pchatHistoryLoading.has(channelId);
  const trust = channelId === null ? undefined : keyTrust[channelId];
  const custodian = channelId === null ? undefined : custodianPins[channelId];
  const dispute = channelId === null ? undefined : pendingDisputes[channelId];
  const keyRevoked = channelId !== null && pchatKeyRevoked.has(channelId);
  const persistenceMode: PersistenceMode = persistence?.mode ?? "NONE";

  const handleShareClick = useCallback((peerCertHash: string, peerName: string) => {
    setKeyShareConfirm({ hash: peerCertHash, name: peerName });
  }, []);

  const handleShareConfirm = useCallback(() => {
    if (channelId !== null && keyShareConfirm) {
      approveKeyShare(channelId, keyShareConfirm.hash);
    }
    setKeyShareConfirm(null);
  }, [channelId, keyShareConfirm, approveKeyShare]);

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

  const keyShareRequests = (channelId !== null && pendingKeyShares[channelId]) || [];

  return {
    trustLevel: trust?.trustLevel,
    onVerifyClick: trust ? () => setShowVerifyDialog(true) : undefined,
    isPersisted: !!persistence && persistence.mode !== "NONE",
    banner: showBanner ? (
      <PersistenceBanner channelId={channelId} />
    ) : null,
    disputeBanner: dispute
      ? buildDisputeBanner(() => setShowVerifyDialog(true))
      : null,
    keyShareBanner: keyRevoked
      ? null
      : buildKeyShareBanner(channelId, keyShareRequests, handleShareClick, dismissKeyShare),
    revokedBanner: keyRevoked ? (
      <InfoBanner variant="danger" icon={warningIcon}>
        {userMode === "normal" ? (
          <p className={infoBannerStyles.description}>
            You can't read or send messages in this channel yet.
            Someone who already has access needs to let you in first.
          </p>
        ) : (
          <>
            <p className={infoBannerStyles.description}>
              <strong>Key challenge failed</strong> — your encryption key was rejected by the server.
            </p>
            <p className={infoBannerStyles.description}>
              All local keying material for this channel has been purged.
              You cannot send or read messages until a verified key holder shares the correct key with you.
            </p>
          </>
        )}
      </InfoBanner>
    ) : null,
    keyRevoked,
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
        <KeyShareWarningDialog
          open={keyShareConfirm !== null}
          peerName={keyShareConfirm?.name ?? ""}
          persistenceMode={persistenceMode}
          totalStored={persistence?.totalStored ?? 0}
          onConfirm={handleShareConfirm}
          onCancel={() => setKeyShareConfirm(null)}
        />
      </>
    ),
  };
}
