import { HeadphonesIcon, HeadphonesOffIcon, MicIcon, MicOffIcon } from "../../icons";
import { useAppStore } from "../../store";
import styles from "./MobileCallControls.module.css";

/**
 * Inline mute / deafen controls shown below the chat header on
 * narrow / mobile viewports when the user is in a call.
 * Hidden on desktop via CSS media query.
 * Stays visible until the user explicitly ends the call via the
 * sidebar hang-up button, even if voice is deafened.
 */
export default function MobileCallControls() {
  const voiceState = useAppStore((s) => s.voiceState);
  const inCall = useAppStore((s) => s.inCall);
  const toggleMute = useAppStore((s) => s.toggleMute);
  const toggleDeafen = useAppStore((s) => s.toggleDeafen);

  if (!inCall) return null;

  const isActive = voiceState === "active";
  const isInactive = voiceState === "inactive";
  const muteTitle = isActive ? "Mute" : "Unmute";

  return (
    <div className={styles.bar}>
      <button
        className={`${styles.btn} ${isActive ? styles.btnActive : ""}`}
        onClick={toggleMute}
        title={muteTitle}
      >
        {isActive ? (
          <MicIcon width={18} height={18} />
        ) : (
          <MicOffIcon width={18} height={18} />
        )}
        <span className={styles.label}>{muteTitle}</span>
      </button>
      <button
        className={`${styles.btn} ${isInactive ? "" : styles.btnActive}`}
        onClick={toggleDeafen}
        title={isInactive ? "Undeafen" : "Deafen"}
      >
        {isInactive ? (
          <HeadphonesOffIcon width={18} height={18} />
        ) : (
          <HeadphonesIcon width={18} height={18} />
        )}
        <span className={styles.label}>{isInactive ? "Undeafen" : "Deafen"}</span>
      </button>
    </div>
  );
}
