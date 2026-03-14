import { useEffect, useState } from "react";
import { Routes, Route, useNavigate, Navigate } from "react-router-dom";
import { initEventListeners } from "./store";
import { getPreferences, isFirstRun } from "./preferencesStorage";
import { setKlipyApiKey } from "./components/GifPicker";
import { useVisualViewport } from "./hooks/useVisualViewport";
import TitleBar from "./components/TitleBar";
import ConnectPage from "./pages/ConnectPage";
import ChatPage from "./pages/ChatPage";
import SettingsPage from "./pages/settings";
import WelcomePage from "./pages/WelcomePage";

export default function App() {
  const navigate = useNavigate();
  const [firstRun, setFirstRun] = useState<boolean | null>(null);

  // Track visual viewport height on mobile so the layout shrinks
  // when the on-screen keyboard is active.
  useVisualViewport();

  // Check first-run status on mount and load persisted preferences.
  useEffect(() => {
    isFirstRun().then(setFirstRun);
    getPreferences().then((prefs) => {
      setKlipyApiKey(prefs.klipyApiKey);
    });
  }, []);

  useEffect(() => {
    let unlisteners: (() => void)[] = [];

    initEventListeners(navigate).then((fns) => {
      unlisteners = fns;
    });

    return () => {
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
          </>
        )}
      </Routes>
    </div>
  );
}
