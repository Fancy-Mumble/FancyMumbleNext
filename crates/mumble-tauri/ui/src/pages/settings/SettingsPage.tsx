import { useState, useEffect, useCallback, useRef } from "react";
import { useNavigate } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { load } from "@tauri-apps/plugin-store";
import type { AudioDevice, AudioSettings, FancyProfile, UserMode, TimeFormat } from "../../types";
import { getPreferences, updatePreferences, getSavedAudioSettings, saveAudioSettings } from "../../preferencesStorage";
import { serializeProfile, dataUrlToBytes } from "../../profileFormat";
import { setKlipyApiKey } from "../../components/GifPicker";
import { useAppStore } from "../../store";
import {
  type ShortcutBindings,
  loadShortcuts,
  saveShortcuts,
  applyGlobalShortcut,
  clearGlobalShortcut,
} from "./shortcutHelpers";
import { loadProfileData, saveProfileData } from "./profileData";
import { ProfilePanel } from "./ProfilePanel";
import { AudioPanel } from "./AudioPanel";
import { ShortcutsPanel } from "./ShortcutsPanel";
import { AdvancedPanel } from "./AdvancedPanel";
import { ProfilePreviewCard } from "./ProfilePreviewCard";
import styles from "./SettingsPage.module.css";

// ── Types & constants ──────────────────────────────────────────────

type Tab = "profile" | "voice" | "shortcuts" | "advanced";

const DEFAULT_AUDIO: AudioSettings = {
  selected_device: null,
  auto_gain: true,
  vad_threshold: 0.3,
  max_gain_db: 15,
  noise_gate_close_ratio: 0.8,
  hold_frames: 15,
  push_to_talk: false,
  push_to_talk_key: null,
  bitrate_bps: 72000,
  frame_size_ms: 20,
  noise_suppression: true,
  selected_output_device: null,
  input_volume: 1,
  output_volume: 1,
  auto_input_sensitivity: false,
};

const TABS: { id: Tab; label: string; icon: string }[] = [
  { id: "profile", label: "Profile", icon: "👤" },
  { id: "voice", label: "Voice", icon: "🎙️" },
  { id: "shortcuts", label: "Shortcuts", icon: "⌨️" },
  { id: "advanced", label: "Advanced", icon: "⚙️" },
];

// ── Main component ─────────────────────────────────────────────────

export default function SettingsPage() {
  const navigate = useNavigate();
  const [tab, setTab] = useState<Tab>("profile");
  const isConnected = useAppStore((s) => s.status) === "connected";

  // Audio
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [outputDevices, setOutputDevices] = useState<AudioDevice[]>([]);
  const [audioSettings, setAudioSettings] =
    useState<AudioSettings>(DEFAULT_AUDIO);
  const initialLoadDone = useRef(false);

  // Preferences
  const [userMode, setUserMode] = useState<UserMode>("normal");
  const [defaultUsername, setDefaultUsername] = useState("");
  const [klipyApiKey, setKlipyApiKeyState] = useState("");
  const [timeFormat, setTimeFormat] = useState<TimeFormat>("auto");
  const [convertToLocalTime, setConvertToLocalTime] = useState(true);

  // Shortcuts
  const [shortcuts, setShortcuts] = useState<ShortcutBindings>({
    toggleMute: "",
    toggleDeafen: "",
  });

  // Profile
  const [profile, setProfile] = useState<FancyProfile>({});
  const [bio, setBio] = useState("");
  const [avatarDataUrl, setAvatarDataUrl] = useState<string | null>(null);

  const [loadError, setLoadError] = useState<string | null>(null);
  const [profileError, setProfileError] = useState<string | null>(null);

  // ── Load everything on mount ────────────────────────────────────

  useEffect(() => {
    (async () => {
      try {
        const [devs, outDevs, cfg, saved] = await Promise.all([
          invoke<AudioDevice[]>("get_audio_devices"),
          invoke<AudioDevice[]>("get_output_devices"),
          invoke<AudioSettings>("get_audio_settings"),
          getSavedAudioSettings(),
        ]);
        setDevices(devs);
        setOutputDevices(outDevs);
        // Merge: persisted settings take precedence over backend defaults.
        const merged = saved ? { ...cfg, ...saved } : cfg;
        setAudioSettings(merged);
        // Push merged settings to the backend so it picks up persisted values.
        if (saved) {
          invoke("set_audio_settings", { settings: merged }).catch((e) =>
            console.error("Restore audio settings error:", e),
          );
        }
      } catch (e) {
        setLoadError(String(e));
      }

      try {
        const prefs = await getPreferences();
        setUserMode(prefs.userMode);
        setDefaultUsername(prefs.defaultUsername);
        setKlipyApiKeyState(prefs.klipyApiKey ?? "");
        setKlipyApiKey(prefs.klipyApiKey);
        setTimeFormat(prefs.timeFormat);
        setConvertToLocalTime(prefs.convertToLocalTime);
      } catch {
        /* keep defaults */
      }

      try {
        const sc = await loadShortcuts();
        setShortcuts(sc);
      } catch {
        /* keep defaults */
      }

      try {
        const pd = await loadProfileData();
        setProfile(pd.profile);
        setBio(pd.bio);
        setAvatarDataUrl(pd.avatarDataUrl);
      } catch {
        /* keep defaults */
      }

      // Mark initial load as done *after* state has settled.
      requestAnimationFrame(() => {
        initialLoadDone.current = true;
      });
    })();
  }, []);

  // ── Listen for permission-denied events from the backend ─────

  useEffect(() => {
    const unlisten = listen<{ deny_type: number | null; reason: string | null }>(
      "permission-denied",
      (event) => {
        const { deny_type, reason } = event.payload;
        let msg = reason || "Permission denied by server.";
        if (deny_type === 4) {
          msg =
            "Your profile is too large for this server. " +
            "Try using a smaller banner image or shorter bio.";
        }
        setProfileError(msg);
      },
    );
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  // ── Auto-save audio settings (debounced) ────────────────────────

  useEffect(() => {
    if (!initialLoadDone.current) return;
    const timer = setTimeout(async () => {
      try {
        await Promise.all([
          invoke("set_audio_settings", { settings: audioSettings }),
          saveAudioSettings(audioSettings),
        ]);
      } catch (e) {
        console.error("Auto-save audio settings error:", e);
      }
    }, 400);
    return () => clearTimeout(timer);
  }, [audioSettings]);

  // ── Auto-save profile data locally (debounced) ──────────────────

  useEffect(() => {
    if (!initialLoadDone.current) return;
    const timer = setTimeout(async () => {
      try {
        await saveProfileData({
          profile,
          bio,
          avatarDataUrl,
        });
      } catch (e) {
        console.error("Auto-save profile error:", e);
      }
    }, 400);
    return () => clearTimeout(timer);
  }, [profile, bio, avatarDataUrl]);

  // ── Auto-apply profile to server (debounced) ────────────────────

  useEffect(() => {
    if (!initialLoadDone.current || !isConnected) return;
    const timer = setTimeout(async () => {
      setProfileError(null);
      try {
        const comment = serializeProfile(profile, bio);
        await invoke("set_user_comment", { comment });

        const texture = avatarDataUrl ? dataUrlToBytes(avatarDataUrl) : [];
        await invoke("set_user_texture", { texture });
      } catch (e) {
        console.error("Auto-apply profile error:", e);
      }
    }, 800);
    return () => clearTimeout(timer);
  }, [profile, bio, avatarDataUrl, isConnected]);

  // ── Handlers ────────────────────────────────────────────────────

  const patchAudio = useCallback((patch: Partial<AudioSettings>) => {
    setAudioSettings((prev) => ({ ...prev, ...patch }));
  }, []);

  const patchProfile = useCallback((patch: Partial<FancyProfile>) => {
    setProfile((prev) => ({ ...prev, ...patch }));
  }, []);

  const handleToggleMode = useCallback(async () => {
    const next: UserMode = userMode === "normal" ? "expert" : "normal";
    setUserMode(next);
    await updatePreferences({ userMode: next });
  }, [userMode]);

  const handleKlipyApiKeyChange = useCallback(async (key: string) => {
    setKlipyApiKeyState(key);
    setKlipyApiKey(key);
    await updatePreferences({ klipyApiKey: key });
  }, []);

  const handleChangeShortcut = useCallback(
    async (key: keyof ShortcutBindings, value: string) => {
      setShortcuts((prev) => {
        const updated = { ...prev, [key]: value };
        // Persist + register in background.
        (async () => {
          await clearGlobalShortcut(prev[key]);
          await saveShortcuts(updated);
          const command =
            key === "toggleMute" ? "toggle_mute" : "toggle_deafen";
          await applyGlobalShortcut(value, command);
        })();
        return updated;
      });
    },
    [],
  );

  const handleTimeFormatChange = useCallback(async (fmt: TimeFormat) => {
    setTimeFormat(fmt);
    await updatePreferences({ timeFormat: fmt });
  }, []);

  const handleConvertToLocalTimeChange = useCallback(async () => {
    setConvertToLocalTime((prev) => {
      const next = !prev;
      updatePreferences({ convertToLocalTime: next });
      return next;
    });
  }, []);

  const handleToggleDeveloperMode = useCallback(async () => {
    const next: UserMode = userMode === "developer" ? "expert" : "developer";
    setUserMode(next);
    await updatePreferences({ userMode: next });
  }, [userMode]);

  const handleReset = useCallback(async () => {
    try {
      // Clear all tauri-plugin-store caches so the in-memory data is gone.
      // (The Rust reset_app_data only deletes files on disk - the plugin
      //  keeps a Rust-side cache that survives a webview reload.)
      for (const file of ["preferences.json", "servers.json", "shortcuts.json", "profile.json"]) {
        try {
          const s = await load(file, { autoSave: false, defaults: {} });
          await s.clear();
          await s.save();
        } catch {
          // Ignore - file may not exist yet.
        }
      }
      await invoke("reset_app_data");
      // Reload the app so isFirstRun() re-evaluates and shows the welcome page.
      window.location.replace("/");
    } catch (e) {
      console.error("reset_app_data error:", e);
    }
  }, []);

  const handleBack = useCallback(() => {
    navigate(-1);
  }, [navigate]);

  // ── Render ──────────────────────────────────────────────────────

  return (
    <div className={styles.page}>
      {/* Sidebar */}
      <nav className={styles.sidebar}>
        <button
          className={styles.backBtn}
          onClick={handleBack}
          aria-label="Go back"
        >
          <svg
            width="18"
            height="18"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <polyline points="15 18 9 12 15 6" />
          </svg>
          <span>Back</span>
        </button>

        <h2 className={styles.sidebarHeading}>Settings</h2>

        <ul className={styles.tabList}>
          {TABS.map((t) => (
            <li key={t.id}>
              <button
                className={`${styles.tabBtn} ${tab === t.id ? styles.tabBtnActive : ""}`}
                onClick={() => setTab(t.id)}
              >
                <span className={styles.tabIcon}>{t.icon}</span>
                {t.label}
              </button>
            </li>
          ))}
        </ul>
      </nav>

      {/* Main area (scrolling container) */}
      <div
        className={`${styles.mainArea} ${
          tab === "profile" ? styles.mainAreaWithPreview : ""
        }`}
      >
        {/* Content */}
        <main className={styles.content}>
          {loadError && <p className={styles.error}>{loadError}</p>}

          {tab === "profile" && (
            <ProfilePanel
              defaultUsername={defaultUsername}
              setDefaultUsername={setDefaultUsername}
              profile={profile}
              onPatchProfile={patchProfile}
              bio={bio}
              onBioChange={setBio}
              avatar={avatarDataUrl}
              onAvatarChange={setAvatarDataUrl}
              profileError={profileError}
              isExpert={userMode !== "normal"}
            />
          )}

          {tab === "voice" && (
            <AudioPanel
              devices={devices}
              outputDevices={outputDevices}
              settings={audioSettings}
              onChange={patchAudio}
              isExpert={userMode !== "normal"}
            />
          )}

          {tab === "shortcuts" && (
            <ShortcutsPanel
              shortcuts={shortcuts}
              onChangeShortcut={handleChangeShortcut}
            />
          )}

          {tab === "advanced" && (
            <AdvancedPanel
              userMode={userMode}
              klipyApiKey={klipyApiKey}
              timeFormat={timeFormat}
              convertToLocalTime={convertToLocalTime}
              onToggleMode={handleToggleMode}
              onKlipyApiKeyChange={handleKlipyApiKeyChange}
              onTimeFormatChange={handleTimeFormatChange}
              onConvertToLocalTimeChange={handleConvertToLocalTimeChange}
              onToggleDeveloperMode={handleToggleDeveloperMode}
              onReset={handleReset}
            />
          )}
        </main>

        {/* Profile preview (sticky right column) */}
        {tab === "profile" && (
          <aside className={styles.previewPane}>
            <div className={styles.previewSticky}>
              <ProfilePreviewCard
                profile={profile}
                bio={bio}
                avatar={avatarDataUrl}
                displayName={defaultUsername}
              />
            </div>
          </aside>
        )}
      </div>
    </div>
  );
}
