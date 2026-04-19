import { useEffect, useState } from "react";
import { Routes, Route, useNavigate, Navigate } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import { initEventListeners } from "./store";
import { getPreferences, getSavedAudioSettings, isFirstRun, getNotificationSounds } from "./preferencesStorage";
import { setKlipyApiKey } from "./components/chat/GifPicker";
import { setKlipyApiKey as setKlipyApiKeyBanner } from "./pages/settings/KlipyGifBrowser";
import { loadShortcuts, applyGlobalShortcut } from "./pages/settings/shortcutHelpers";
import { useVisualViewport } from "./hooks/useVisualViewport";
import { useNotificationSounds } from "./hooks/useNotificationSounds";
import { DEFAULT_NOTIFICATION_SOUNDS } from "./pages/settings/NotificationsPanel";
import type { NotificationSoundSettings } from "./types";
import TitleBar from "./components/layout/TitleBar";
import ConnectPage from "./pages/ConnectPage";
import ChatPage from "./pages/ChatPage";
import SettingsPage from "./pages/settings";
import AdminPanel from "./pages/admin";
import WelcomePage from "./pages/WelcomePage";

export default function App() {
  const navigate = useNavigate();
  const [firstRun, setFirstRun] = useState<boolean | null>(null);
  const [notifSounds, setNotifSounds] =
    useState<NotificationSoundSettings>(DEFAULT_NOTIFICATION_SOUNDS);

  // Track visual viewport height on mobile so the layout shrinks
  // when the on-screen keyboard is active.
  useVisualViewport();

  // Notification sounds - plays audio for events based on user config.
  useNotificationSounds(notifSounds);

  // Check first-run status on mount and load persisted preferences.
  // Also apply saved audio settings and shortcuts to the backend so
  // they take effect without the user visiting the settings page.
  useEffect(() => {
    isFirstRun().then(setFirstRun);
    getPreferences().then((prefs) => {
      setKlipyApiKey(prefs.klipyApiKey);
      setKlipyApiKeyBanner(prefs.klipyApiKey);
    });
    getNotificationSounds().then((ns) => {
      if (ns) setNotifSounds(ns);
    });
    getSavedAudioSettings().then((saved) => {
      if (saved) {
        invoke("set_audio_settings", { settings: saved }).catch((e) =>
          console.error("Startup audio settings error:", e),
        );
      }
    });
    loadShortcuts().then((sc) => {
      if (sc.toggleMute) {
        applyGlobalShortcut(sc.toggleMute, "toggle_mute").catch(console.error);
      }
      if (sc.toggleDeafen) {
        applyGlobalShortcut(sc.toggleDeafen, "toggle_deafen").catch(console.error);
      }
    });
  }, []);

  // Sync notification sounds when settings page saves changes.
  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent<NotificationSoundSettings>).detail;
      setNotifSounds(detail);
    };
    window.addEventListener("notification-sounds-changed", handler);
    return () => window.removeEventListener("notification-sounds-changed", handler);
  }, []);

  useEffect(() => {
    let cancelled = false;
    let unlisteners: (() => void)[] = [];

    initEventListeners(navigate).then((fns) => {
      if (cancelled) {
        fns.forEach((fn) => fn());
        return;
      }
      unlisteners = fns;
    });

    return () => {
      cancelled = true;
      unlisteners.forEach((fn) => fn());
    };
  }, [navigate]);

  // Wait until we know the first-run status before rendering routes.
  if (firstRun === null) return null;

  return (
    <div className="app">
      <TitleBar />
      <Routes>
        {firstRun ? (
          <>
            <Route path="/welcome" element={<WelcomePage onComplete={() => setFirstRun(false)} />} />
            <Route path="*" element={<Navigate to="/welcome" replace />} />
          </>
        ) : (
          <>
            <Route path="/" element={<ConnectPage />} />
            <Route path="/chat" element={<ChatPage />} />
            <Route path="/settings" element={<SettingsPage />} />
            <Route path="/admin" element={<AdminPanel />} />
          </>
        )}
      </Routes>
    </div>
  );
}
