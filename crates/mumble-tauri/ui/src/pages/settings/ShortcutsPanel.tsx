import type { ShortcutBindings } from "./shortcutHelpers";
import { ShortcutRecorder } from "./SharedControls";
import styles from "./SettingsPage.module.css";

export function ShortcutsPanel({
  shortcuts,
  onChangeShortcut,
}: {
  shortcuts: ShortcutBindings;
  onChangeShortcut: (key: keyof ShortcutBindings, value: string) => void;
}) {
  return (
    <>
      <h2 className={styles.panelTitle}>Shortcuts</h2>
      <p className={styles.fieldHint}>
        Global keyboard shortcuts that work even when the app is in the
        background.
      </p>

      <section className={styles.section}>
        <ShortcutRecorder
          label="Toggle Mute"
          value={shortcuts.toggleMute}
          onChange={(v) => onChangeShortcut("toggleMute", v)}
        />
      </section>

      <section className={styles.section}>
        <ShortcutRecorder
          label="Toggle Deafen"
          value={shortcuts.toggleDeafen}
          onChange={(v) => onChangeShortcut("toggleDeafen", v)}
        />
      </section>
    </>
  );
}
