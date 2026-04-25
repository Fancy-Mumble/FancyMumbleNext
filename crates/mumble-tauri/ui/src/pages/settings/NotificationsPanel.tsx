import { PlayIcon } from "../../icons";
import { useCallback, useRef } from "react";
import type {
  NotificationSoundSettings,
  NotificationEvent,
  NotificationEventConfig,
} from "../../types";
import { Toggle } from "./SharedControls";
import styles from "./SettingsPage.module.css";
import ns from "./NotificationsPanel.module.css";

import sndDragon3 from "../../assets/audio/dragon-studio-new-notification-3-398649.mp3";
import sndUniv033 from "../../assets/audio/universfield-new-notification-033-480571.mp3";
import sndUniv036 from "../../assets/audio/universfield-new-notification-036-485897.mp3";
import sndUniv040 from "../../assets/audio/universfield-new-notification-040-493469.mp3";
import sndUniv051 from "../../assets/audio/universfield-new-notification-051-494246.mp3";
import sndUniv057 from "../../assets/audio/universfield-new-notification-057-494255.mp3";
import sndUniv09 from "../../assets/audio/universfield-new-notification-09-352705.mp3";

export interface SoundOption {
  id: string;
  label: string;
  url: string;
}

export const SOUND_OPTIONS: SoundOption[] = [
  { id: "none", label: "None", url: "" },
  { id: "dragon-3", label: "Chime", url: sndDragon3 },
  { id: "univ-033", label: "Bubble", url: sndUniv033 },
  { id: "univ-036", label: "Pop", url: sndUniv036 },
  { id: "univ-040", label: "Ding", url: sndUniv040 },
  { id: "univ-051", label: "Ping", url: sndUniv051 },
  { id: "univ-057", label: "Drop", url: sndUniv057 },
  { id: "univ-09", label: "Bell", url: sndUniv09 },
];

interface EventDef {
  key: NotificationEvent;
  label: string;
  description: string;
}

const EVENT_DEFS: EventDef[] = [
  {
    key: "chatMessage",
    label: "Chat message",
    description: "A new message in a channel you are viewing",
  },
  {
    key: "directMessage",
    label: "Direct message",
    description: "A new private or group message",
  },
  {
    key: "mention",
    label: "Mention",
    description: "Someone mentioned you with @, @everyone, @here, or your role",
  },
  {
    key: "userJoin",
    label: "User joined server",
    description: "Someone connected to the server",
  },
  {
    key: "userLeave",
    label: "User left server",
    description: "Someone disconnected from the server",
  },
  {
    key: "userJoinChannel",
    label: "User joined my channel",
    description: "Someone moved into your current channel",
  },
  {
    key: "userLeaveChannel",
    label: "User left my channel",
    description: "Someone moved out of your current channel",
  },
  {
    key: "streamStart",
    label: "Screen share started",
    description: "A user started sharing their screen",
  },
  {
    key: "voiceActivity",
    label: "Voice activity",
    description: "Someone started speaking in your channel",
  },
  {
    key: "selfMuted",
    label: "Self muted",
    description: "You muted or unmuted your microphone",
  },
];

export const DEFAULT_NOTIFICATION_SOUNDS: NotificationSoundSettings = {
  masterEnabled: true,
  events: {
    chatMessage: { enabled: true, sound: "dragon-3", volume: 0.5 },
    directMessage: { enabled: true, sound: "univ-033", volume: 0.7 },
    mention: { enabled: true, sound: "univ-09", volume: 0.7 },
    userJoin: { enabled: true, sound: "univ-036", volume: 0.4 },
    userLeave: { enabled: true, sound: "univ-040", volume: 0.4 },
    userJoinChannel: { enabled: true, sound: "univ-036", volume: 0.5 },
    userLeaveChannel: { enabled: true, sound: "univ-040", volume: 0.5 },
    streamStart: { enabled: true, sound: "univ-051", volume: 0.5 },
    voiceActivity: { enabled: false, sound: "none", volume: 0.3 },
    selfMuted: { enabled: true, sound: "univ-057", volume: 0.4 },
  },
};

function findSoundUrl(id: string): string {
  return SOUND_OPTIONS.find((s) => s.id === id)?.url ?? "";
}

export function NotificationsPanel({
  settings,
  onChange,
  enableNativeNotifications,
  onToggleNativeNotifications,
  isExpert,
}: {
  settings: NotificationSoundSettings;
  onChange: (patch: Partial<NotificationSoundSettings>) => void;
  enableNativeNotifications: boolean;
  onToggleNativeNotifications: () => void;
  isExpert: boolean;
}) {
  const previewAudioRef = useRef<HTMLAudioElement | null>(null);

  const patchEvent = useCallback(
    (key: NotificationEvent, patch: Partial<NotificationEventConfig>) => {
      onChange({
        events: {
          ...settings.events,
          [key]: { ...settings.events[key], ...patch },
        },
      });
    },
    [settings.events, onChange],
  );

  const toggleMaster = useCallback(() => {
    onChange({ masterEnabled: !settings.masterEnabled });
  }, [settings.masterEnabled, onChange]);

  const enableAll = useCallback(() => {
    const updated = { ...settings.events };
    for (const def of EVENT_DEFS) {
      updated[def.key] = { ...updated[def.key], enabled: true };
    }
    onChange({ events: updated });
  }, [settings.events, onChange]);

  const disableAll = useCallback(() => {
    const updated = { ...settings.events };
    for (const def of EVENT_DEFS) {
      updated[def.key] = { ...updated[def.key], enabled: false };
    }
    onChange({ events: updated });
  }, [settings.events, onChange]);

  const preview = useCallback((soundId: string, volume: number) => {
    const url = findSoundUrl(soundId);
    if (!url) return;
    if (previewAudioRef.current) {
      previewAudioRef.current.pause();
    }
    const audio = new Audio(url);
    audio.volume = volume;
    previewAudioRef.current = audio;
    audio.play().catch(() => {});
  }, []);

  const allEnabled = EVENT_DEFS.every((d) => settings.events[d.key]?.enabled ?? DEFAULT_NOTIFICATION_SOUNDS.events[d.key].enabled);
  const allDisabled = EVENT_DEFS.every((d) => !(settings.events[d.key]?.enabled ?? DEFAULT_NOTIFICATION_SOUNDS.events[d.key].enabled));

  return (
    <>
      <h2 className={styles.panelTitle}>Notifications</h2>

      {/* Master sound toggle */}
      <section className={styles.section}>
        <div className={styles.toggleRow}>
          <div className={styles.toggleInfo}>
            <h3 className={styles.sectionTitle}>Notification sounds</h3>
            <p className={styles.fieldHint}>
              Play a sound when events occur. Individual events can be
              configured below.
            </p>
          </div>
          <Toggle checked={settings.masterEnabled} onChange={toggleMaster} />
        </div>
      </section>

      {/* Native OS notifications toggle (moved from Advanced) */}
      <section className={styles.section}>
        <div className={styles.toggleRow}>
          <div className={styles.toggleInfo}>
            <h3 className={styles.sectionTitle}>Native notifications</h3>
            <p className={styles.fieldHint}>
              Show native OS notifications for new messages when the app is
              in the background.
            </p>
          </div>
          <Toggle
            checked={enableNativeNotifications}
            onChange={onToggleNativeNotifications}
          />
        </div>
      </section>

      {/* Bulk actions */}
      {settings.masterEnabled && (
        <section className={styles.section}>
          <div className={ns.bulkActions}>
            <button
              type="button"
              className={ns.bulkBtn}
              onClick={enableAll}
              disabled={allEnabled}
            >
              Enable all
            </button>
            <button
              type="button"
              className={ns.bulkBtn}
              onClick={disableAll}
              disabled={allDisabled}
            >
              Disable all
            </button>
          </div>
        </section>
      )}

      {/* Per-event configuration */}
      {settings.masterEnabled &&
        EVENT_DEFS.map((def) => {
          const cfg = settings.events[def.key] ?? DEFAULT_NOTIFICATION_SOUNDS.events[def.key];
          return (
            <section key={def.key} className={styles.section}>
              <div className={styles.toggleRow}>
                <div className={styles.toggleInfo}>
                  <h3 className={styles.sectionTitle}>{def.label}</h3>
                  <p className={styles.fieldHint}>{def.description}</p>
                </div>
                <Toggle
                  checked={cfg.enabled}
                  onChange={() => patchEvent(def.key, { enabled: !cfg.enabled })}
                />
              </div>

              {cfg.enabled && isExpert && (
                <div className={ns.eventConfig}>
                  <div className={ns.soundRow}>
                    <select
                      className={styles.select}
                      value={cfg.sound}
                      onChange={(e) =>
                        patchEvent(def.key, { sound: e.target.value })
                      }
                    >
                      {SOUND_OPTIONS.map((opt) => (
                        <option key={opt.id} value={opt.id}>
                          {opt.label}
                        </option>
                      ))}
                    </select>
                    <button
                      type="button"
                      className={ns.previewBtn}
                      onClick={() => preview(cfg.sound, cfg.volume)}
                      disabled={cfg.sound === "none"}
                      title="Preview sound"
                    >
                      <PlayIcon width={16} height={16} />
                    </button>
                  </div>

                  <div className={ns.volumeRow}>
                    <span className={ns.volumeLabel}>Volume</span>
                    <input
                      type="range"
                      className={styles.slider}
                      min={0}
                      max={1}
                      step={0.05}
                      value={cfg.volume}
                      onChange={(e) =>
                        patchEvent(def.key, {
                          volume: parseFloat(e.target.value),
                        })
                      }
                    />
                    <span className={ns.volumeValue}>
                      {Math.round(cfg.volume * 100)}%
                    </span>
                  </div>
                </div>
              )}
            </section>
          );
        })}
    </>
  );
}
