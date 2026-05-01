import { lazy, Suspense, useEffect, useState } from "react";
import { Routes, Route, useNavigate, Navigate } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
import { initEventListeners, useAppStore } from "./store";
import { getPreferences, getSavedAudioSettings, isFirstRun, getNotificationSounds } from "./preferencesStorage";
import { setKlipyApiKey } from "./components/chat/GifPicker";
import { setKlipyApiKey as setKlipyApiKeyBanner } from "./pages/settings/KlipyGifBrowser";
import { loadShortcuts, applyGlobalShortcut } from "./pages/settings/shortcutHelpers";
import { useVisualViewport } from "./hooks/useVisualViewport";
import { useNotificationSounds } from "./hooks/useNotificationSounds";
import { useSpoilerReveal } from "./hooks/useSpoilerReveal";
import { useCodeHighlight } from "./hooks/useCodeHighlight";
import { DEFAULT_NOTIFICATION_SOUNDS } from "./pages/settings/NotificationsPanel";
import type { NotificationSoundSettings } from "./types";
import TitleBar from "./components/layout/TitleBar";
import ConnectPage from "./pages/ConnectPage";
import LoadingSplash from "./components/elements/LoadingSplash";
import { isUpdaterWindow } from "./updater";
import UpdaterWindow from "./updater/UpdaterWindow";
import PopoutPage from "./pages/PopoutPage";

const ChatPage = lazy(() => import("./pages/ChatPage"));
const SettingsPage = lazy(() => import("./pages/settings"));
const AdminPanel = lazy(() => import("./pages/admin"));
const RoleEditorPage = lazy(() => import("./pages/admin/RoleEditorPage"));
const WelcomePage = lazy(() => import("./pages/WelcomePage"));

/**
 * Returns true when this webview window is an image popout window.
 * Popout windows are spawned by `open_image_popout` and use a window
 * label of the form `popout-<id>`.
 */
function isPopoutWindow(): boolean {
  // Tauri exposes the window label via the `__TAURI_METADATA__` global, but
  // checking the `?popout=` query string set by the popout URL is simpler
  // and works in browser dev as well.
  if (new URLSearchParams(window.location.search).has("popout")) return true;
  // Fallback: detect via the Tauri window label using the IPC global.
  // We run this synchronously by reading the document title fallback.
  const tauriInternals = (window as unknown as { __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: string } } } }).__TAURI_INTERNALS__;
  const label = tauriInternals?.metadata?.currentWindow?.label;
  return !!label && label.startsWith("popout-");
}

const enum WindowKind { Main, Popout, Updater }

function getWindowKind(): WindowKind {
  if (isUpdaterWindow()) return WindowKind.Updater;
  if (isPopoutWindow()) return WindowKind.Popout;
  return WindowKind.Main;
}

export default function App() {
  switch (getWindowKind()) {
    case WindowKind.Updater: return <UpdaterWindow />;
    case WindowKind.Popout:  return <PopoutPage />;
    default:                 return <MainApp />;
  }
}

function MainApp() {
  const navigate = useNavigate();
  const [firstRun, setFirstRun] = useState<boolean | null>(null);
  const [notifSounds, setNotifSounds] =
    useState<NotificationSoundSettings>(DEFAULT_NOTIFICATION_SOUNDS);

  // Track visual viewport height on mobile so the layout shrinks
  // when the on-screen keyboard is active.
  useVisualViewport();

  // Notification sounds - plays audio for events based on user config.
  useNotificationSounds(notifSounds);

  // Click-to-reveal for spoiler tags rendered anywhere in the app.
  useSpoilerReveal();

  // Syntax-highlight any <pre><code> block rendered anywhere in the app.
  useCodeHighlight();

  // Check first-run status on mount and load persisted preferences.
  // Also apply saved audio settings and shortcuts to the backend so
  // they take effect without the user visiting the settings page.
  useEffect(() => {
    isFirstRun().then(setFirstRun);
    getPreferences().then((prefs) => {
      setKlipyApiKey(prefs.klipyApiKey);
      setKlipyApiKeyBanner(prefs.klipyApiKey);
      useAppStore.setState({ disableLinkPreviews: prefs.disableLinkPreviews ?? false });
      useAppStore.setState({ streamerMode: prefs.streamerMode ?? false });
      // When streamer mode is enabled at startup, suppress native notifications
      // so they cannot leak personal data into a screen recording.
      if (prefs.streamerMode) {
        invoke("set_notifications_enabled", { enabled: false }).catch(() => undefined);
      }
      // Inform the Rust updater whether to auto-install on startup.
      invoke("updater_set_auto_install", { enabled: prefs.autoUpdateOnStartup ?? false })
        .catch(() => undefined);
      // Inform the Rust updater of the version (if any) the user chose to skip.
      invoke("updater_set_skipped_version", { version: prefs.skippedUpdateVersion ?? null })
        .catch(() => undefined);
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
  if (firstRun === null) return <LoadingSplash />;

  return (
    <div className="app">
      <TitleBar />
      <Suspense fallback={<LoadingSplash />}>
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
              <Route path="/admin/role/:groupName" element={<RoleEditorPage />} />
            </>
          )}
        </Routes>
      </Suspense>
    </div>
  );
}
