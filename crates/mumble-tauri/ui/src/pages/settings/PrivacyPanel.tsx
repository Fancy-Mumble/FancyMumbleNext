import { Toggle } from "./SharedControls";
import styles from "./SettingsPage.module.css";

export function PrivacyPanel({
  enableDualPath,
  disableReadReceipts,
  disableTypingIndicators,
  disableOsmMaps,
  disableLinkPreviews,
  enableExternalEmbeds,
  streamerMode,
  onToggleDualPath,
  onToggleReadReceipts,
  onToggleTypingIndicators,
  onToggleOsmMaps,
  onToggleLinkPreviews,
  onToggleExternalEmbeds,
  onToggleStreamerMode,
}: {
  enableDualPath: boolean;
  disableReadReceipts: boolean;
  disableTypingIndicators: boolean;
  disableOsmMaps: boolean;
  disableLinkPreviews: boolean;
  enableExternalEmbeds: boolean;
  streamerMode: boolean;
  onToggleDualPath: () => void;
  onToggleReadReceipts: () => void;
  onToggleTypingIndicators: () => void;
  onToggleOsmMaps: () => void;
  onToggleLinkPreviews: () => void;
  onToggleExternalEmbeds: () => void;
  onToggleStreamerMode: () => void;
}) {
  return (
    <>
      <h2 className={styles.panelTitle}>Privacy</h2>

      <section className={styles.section}>
        <div className={styles.toggleRow}>
          <div className={styles.toggleInfo}>
            <h3 className={styles.sectionTitle}>
              Enable dual-path sending
            </h3>
            <p className={styles.fieldHint}>
              When enabled, encrypted channels also send a plain-text
              placeholder over the normal message path so legacy clients
              without E2EE support see &quot;[Encrypted message]&quot; instead
              of nothing. Disable this to keep the ciphertext off the
              unencrypted path entirely.
            </p>
          </div>
          <Toggle checked={enableDualPath} onChange={onToggleDualPath} />
        </div>
      </section>

      <section className={styles.section}>
        <div className={styles.toggleRow}>
          <div className={styles.toggleInfo}>
            <h3 className={styles.sectionTitle}>
              Disable read receipts
            </h3>
            <p className={styles.fieldHint}>
              When enabled, other users will not see that you have read their
              messages. You will also not see read receipts from others.
            </p>
          </div>
          <Toggle checked={disableReadReceipts} onChange={onToggleReadReceipts} />
        </div>
      </section>

      <section className={styles.section}>
        <div className={styles.toggleRow}>
          <div className={styles.toggleInfo}>
            <h3 className={styles.sectionTitle}>
              Disable typing indicators
            </h3>
            <p className={styles.fieldHint}>
              When enabled, you will not send typing indicators to others
              and you will not see when others are typing.
            </p>
          </div>
          <Toggle checked={disableTypingIndicators} onChange={onToggleTypingIndicators} />
        </div>
      </section>

      <section className={styles.section}>
        <div className={styles.toggleRow}>
          <div className={styles.toggleInfo}>
            <h3 className={styles.sectionTitle}>
              Disable OpenStreetMap maps
            </h3>
            <p className={styles.fieldHint}>
              When enabled, no map tiles are loaded and no IP geolocation
              requests are sent to external services.
            </p>
          </div>
          <Toggle checked={disableOsmMaps} onChange={onToggleOsmMaps} />
        </div>
      </section>

      <section className={styles.section}>
        <div className={styles.toggleRow}>
          <div className={styles.toggleInfo}>
            <h3 className={styles.sectionTitle}>
              Disable link previews
            </h3>
            <p className={styles.fieldHint}>
              When enabled, the app will not request link metadata from the
              server. This prevents the server from learning which URLs you
              share in chat.
            </p>
          </div>
          <Toggle checked={disableLinkPreviews} onChange={onToggleLinkPreviews} />
        </div>
      </section>

      <section className={styles.section}>
        <div className={styles.toggleRow}>
          <div className={styles.toggleInfo}>
            <h3 className={styles.sectionTitle}>
              Allow external embeds
            </h3>
            <p className={styles.fieldHint}>
              Required for the YouTube watch-together adapter. When
              enabled, the YouTube IFrame API is loaded from
              youtube.com on demand. Disable to keep all watch-together
              sessions on direct media URLs only.
            </p>
          </div>
          <Toggle checked={enableExternalEmbeds} onChange={onToggleExternalEmbeds} />
        </div>
      </section>

      <section className={styles.section}>
        <div className={styles.toggleRow}>
          <div className={styles.toggleInfo}>
            <h3 className={styles.sectionTitle}>
              Streamer mode
            </h3>
            <p className={styles.fieldHint}>
              Hides identifying information (server host, ports, IP
              addresses, geolocation) and suppresses native notifications
              so they cannot leak personal data into a screen recording.
            </p>
          </div>
          <Toggle checked={streamerMode} onChange={onToggleStreamerMode} />
        </div>
      </section>
    </>
  );
}
