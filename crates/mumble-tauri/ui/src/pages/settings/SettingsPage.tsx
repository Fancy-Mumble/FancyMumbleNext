import { useState, useEffect, useCallback, useRef } from "react";
import { useNavigate } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { load } from "@tauri-apps/plugin-store";
import type { AudioDevice, AudioSettings, FancyProfile, UserMode, TimeFormat } from "../../types";
import { getPreferences, updatePreferences, getSavedAudioSettings, saveAudioSettings } from "../../preferencesStorage";
import { serializeProfile, dataUrlToBytes } from "../../profileFormat";
import { setKlipyApiKey } from "../../components/chat/GifPicker";
import { useAppStore } from "../../store";
import {
  type ShortcutBindings,
  loadShortcuts,
  saveShortcuts,
  applyGlobalShortcut,
  clearGlobalShortcut,
} from "./shortcutHelpers";
import { loadProfileData, saveProfileData, migrateProfilesToIdentities } from "./profileData";
import { ProfilePanel } from "./ProfilePanel";
import { AudioPanel } from "./AudioPanel";
import { ShortcutsPanel } from "./ShortcutsPanel";
import { AdvancedPanel } from "./AdvancedPanel";
import { IdentitiesPanel } from "./IdentitiesPanel";
import { PersonalizationPanel } from "./PersonalizationPanel";
import { NotificationsPanel, DEFAULT_NOTIFICATION_SOUNDS } from "./NotificationsPanel";
import { getNotificationSounds, saveNotificationSounds } from "../../preferencesStorage";
import { ProfilePreviewCard } from "./ProfilePreviewCard";
import { loadPersonalization, savePersonalization, type PersonalizationData } from "../../personalizationStorage";
import { TabbedPage, type TabDef } from "../../components/elements/TabbedPage";
import styles from "./SettingsPage.module.css";

// -- Types & constants ----------------------------------------------

type Tab = "profile" | "voice" | "shortcuts" | "identities" | "advanced" | "personalize" | "notifications";

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
  force_tcp_audio: false,
};

const PERSONALIZATION_DEFAULTS: PersonalizationData = {
  chatBgOriginal: null,
  chatBgBlurred: null,
  chatBgBlurSigma: 0,
  chatBgOpacity: 0.25,
  chatBgDim: 0.5,
  chatBgFit: "cover",
  bubbleStyle: "bubbles",
  fontSize: "medium",
  fontSizeCustomPx: 14,
  fontFamily: "system",
  compactMode: false,
  channelViewerStyle: "flat",
};

const TABS: TabDef<Tab>[] = [
  { id: "profile", label: "Profile", icon: "👤" },
  { id: "voice", label: "Voice", icon: "🎙️" },
  { id: "shortcuts", label: "Shortcuts", icon: "⌨️" },
  { id: "identities", label: "Identities", icon: "🔑" },
  { id: "notifications", label: "Notifications", icon: "🔔" },
  { id: "personalize", label: "Personalize", icon: "🎨" },
  { id: "advanced", label: "Advanced", icon: "⚙️" },
];

// -- Main component -------------------------------------------------

export default function SettingsPage() {
  const navigate = useNavigate();
  const [tab, setTab] = useState<Tab>("profile");
  const isConnected = useAppStore((s) => s.status) === "connected";
  const connectedCertLabel = useAppStore((s) => s.connectedCertLabel);

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
  const [enableNotifications, setEnableNotifications] = useState(true);
  const [disableDualPath, setDisableDualPath] = useState(false);
  const [disableReadReceipts, setDisableReadReceipts] = useState(false);
  const [debugLogging, setDebugLogging] = useState(false);
  const [timeFormat, setTimeFormat] = useState<TimeFormat>("auto");
  const [convertToLocalTime, setConvertToLocalTime] = useState(true);

  // Shortcuts
  const [shortcuts, setShortcuts] = useState<ShortcutBindings>({
    toggleMute: "",
    toggleDeafen: "",
  });

  // Profile (per-identity)
  const [profile, setProfile] = useState<FancyProfile>({});
  const [bio, setBio] = useState("");
  const [avatarDataUrl, setAvatarDataUrl] = useState<string | null>(null);
  const [activeIdentity, setActiveIdentity] = useState<string | null>(null);

  // Identities
  const [identities, setIdentities] = useState<string[]>([]);

  // Personalization
  const [personalization, setPersonalization] = useState<PersonalizationData>(PERSONALIZATION_DEFAULTS);

  // Notification sounds
  const [notificationSounds, setNotificationSounds] = useState(DEFAULT_NOTIFICATION_SOUNDS);

  const [loadError, setLoadError] = useState<string | null>(null);
  const [profileError, setProfileError] = useState<string | null>(null);

  // -- Load everything on mount ------------------------------------

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
        setEnableNotifications(prefs.enableNotifications ?? true);
        setDisableDualPath(prefs.disableDualPath ?? false);
        setDisableReadReceipts(prefs.disableReadReceipts ?? false);
        setDebugLogging(prefs.debugLogging ?? false);
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

      let certs: string[] = [];
      try {
        certs = await invoke<string[]>("list_certificates");
        setIdentities(certs);
      } catch {
        /* keep defaults */
      }

      // Migrate global profile to per-identity storage (one-time).
      await migrateProfilesToIdentities(certs);

      // Prefer the identity used for the active connection; fall back to first cert.
      const { connectedCertLabel } = useAppStore.getState();
      const initialIdentity = connectedCertLabel ?? certs[0] ?? null;
      setActiveIdentity(initialIdentity);

      try {
        const pd = await loadProfileData(initialIdentity);
        setProfile(pd.profile);
        setBio(pd.bio);
        setAvatarDataUrl(pd.avatarDataUrl);
      } catch {
        /* keep defaults */
      }

      try {
        const pz = await loadPersonalization();
        setPersonalization(pz);
      } catch {
        /* keep defaults */
      }

      try {
        const ns = await getNotificationSounds();
        if (ns) setNotificationSounds(ns);
      } catch {
        /* keep defaults */
      }

      // Mark initial load as done *after* state has settled.
      requestAnimationFrame(() => {
        initialLoadDone.current = true;
      });
    })();
  }, []);

  // -- Listen for permission-denied events from the backend -----

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

  // -- Auto-save audio settings (debounced) ------------------------

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

  // -- Auto-save personalization (debounced) -----------------------

  useEffect(() => {
    if (!initialLoadDone.current) return;
    const timer = setTimeout(async () => {
      try {
        await savePersonalization(personalization);
      } catch (e) {
        console.error("Auto-save personalization error:", e);
      }
    }, 400);
    return () => clearTimeout(timer);
  }, [personalization]);

  // -- Auto-save notification sounds (debounced) -------------------

  useEffect(() => {
    if (!initialLoadDone.current) return;
    const timer = setTimeout(async () => {
      try {
        await saveNotificationSounds(notificationSounds);
      } catch (e) {
        console.error("Auto-save notification sounds error:", e);
      }
    }, 400);
    return () => clearTimeout(timer);
  }, [notificationSounds]);

  // -- Auto-save profile data locally (debounced) ------------------

  useEffect(() => {
    if (!initialLoadDone.current) return;
    const timer = setTimeout(async () => {
      try {
        await saveProfileData({
          profile,
          bio,
          avatarDataUrl,
        }, activeIdentity);
      } catch (e) {
        console.error("Auto-save profile error:", e);
      }
    }, 400);
    return () => clearTimeout(timer);
  }, [profile, bio, avatarDataUrl, activeIdentity]);

  // -- Auto-apply profile to server (debounced) --------------------
  // Only sync when viewing the identity that is actually connected.

  useEffect(() => {
    if (!initialLoadDone.current || !isConnected) return;
    if (connectedCertLabel !== activeIdentity) return;
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
  }, [profile, bio, avatarDataUrl, isConnected, connectedCertLabel, activeIdentity]);

  // -- Handlers ----------------------------------------------------

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

  const handleToggleNotifications = useCallback(async () => {
    setEnableNotifications((prev) => {
      const next = !prev;
      updatePreferences({ enableNotifications: next });
      invoke("set_notifications_enabled", { enabled: next }).catch((e) =>
        console.error("set_notifications_enabled error:", e),
      );
      return next;
    });
  }, []);

  const handleNotificationSoundsChange = useCallback(
    (patch: Partial<typeof notificationSounds>) => {
      setNotificationSounds((prev) => ({ ...prev, ...patch }));
    },
    [],
  );

  const handleToggleDualPath = useCallback(async () => {
    setDisableDualPath((prev) => {
      const next = !prev;
      updatePreferences({ disableDualPath: next });
      invoke("set_disable_dual_path", { disabled: next }).catch((e) =>
        console.error("set_disable_dual_path error:", e),
      );
      return next;
    });
  }, []);

  const handleToggleReadReceipts = useCallback(() => {
    setDisableReadReceipts((prev) => {
      const next = !prev;
      updatePreferences({ disableReadReceipts: next });
      return next;
    });
  }, []);

  const handleToggleDeveloperMode = useCallback(async () => {
    const next: UserMode = userMode === "developer" ? "expert" : "developer";
    setUserMode(next);
    await updatePreferences({ userMode: next });
  }, [userMode]);

  const handleToggleDebugLogging = useCallback(async () => {
    const next = !debugLogging;
    const filter = next ? "debug" : "info";
    try {
      await invoke("set_log_level", { filter });
      setDebugLogging(next);
      await updatePreferences({ debugLogging: next });
    } catch (e) {
      console.error("Failed to set log level:", e);
    }
  }, [debugLogging]);

  const refreshIdentities = useCallback(async () => {
    try {
      const certs = await invoke<string[]>("list_certificates");
      setIdentities(certs);
    } catch (e) {
      console.error("Failed to refresh identities:", e);
    }
  }, []);

  const switchIdentity = useCallback(async (label: string | null) => {
    setActiveIdentity(label);
    try {
      const pd = await loadProfileData(label);
      setProfile(pd.profile);
      setBio(pd.bio);
      setAvatarDataUrl(pd.avatarDataUrl);
    } catch {
      setProfile({});
      setBio("");
      setAvatarDataUrl(null);
    }
  }, []);

  const handleEditIdentityProfile = useCallback(
    (label: string) => {
      switchIdentity(label);
      setTab("profile");
    },
    [switchIdentity],
  );

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

  // -- Render ------------------------------------------------------

  return (
    <TabbedPage
      heading="Settings"
      tabs={TABS}
      activeTab={tab}
      onTabChange={setTab}
      onBack={handleBack}
      mainAreaClassName={tab === "profile" ? styles.mainAreaWithPreview : undefined}
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
              activeIdentity={activeIdentity}
              identities={identities}
              connectedCertLabel={connectedCertLabel}
              onSwitchIdentity={switchIdentity}
              onGoToIdentities={() => setTab("identities")}
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

          {tab === "identities" && (
            <IdentitiesPanel
              identities={identities}
              connectedCertLabel={connectedCertLabel}
              onRefresh={refreshIdentities}
              onEditProfile={handleEditIdentityProfile}
              isExpert={userMode !== "normal"}
            />
          )}

          {tab === "personalize" && (
            <PersonalizationPanel
              data={personalization}
              onChange={(patch) => setPersonalization((prev) => ({ ...prev, ...patch }))}
              isExpert={userMode !== "normal"}
            />
          )}

          {tab === "notifications" && (
            <NotificationsPanel
              settings={notificationSounds}
              onChange={handleNotificationSoundsChange}
              enableNativeNotifications={enableNotifications}
              onToggleNativeNotifications={handleToggleNotifications}
              isExpert={userMode !== "normal"}
            />
          )}

          {tab === "advanced" && (
            <AdvancedPanel
              userMode={userMode}
              klipyApiKey={klipyApiKey}
              disableDualPath={disableDualPath}
              disableReadReceipts={disableReadReceipts}
              debugLogging={debugLogging}
              timeFormat={timeFormat}
              convertToLocalTime={convertToLocalTime}
              onToggleMode={handleToggleMode}
              onKlipyApiKeyChange={handleKlipyApiKeyChange}
              onToggleDualPath={handleToggleDualPath}
              onToggleReadReceipts={handleToggleReadReceipts}
              onToggleDebugLogging={handleToggleDebugLogging}
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
    </TabbedPage>
  );
}
