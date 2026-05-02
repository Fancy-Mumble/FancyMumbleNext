import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppStore } from "../store";
import {
  getSavedServers,
  setServerPassword,
  updateServer,
} from "../serverStorage";
import type { SavedServer } from "../types";

/** Password dialog submit/change-username handlers, shared by ConnectPage and ChatPage. */
export interface PasswordPromptHandlers {
  readonly handleSubmit: (password: string, save: boolean) => Promise<void>;
  readonly handleChangeUsername: (newUsername: string) => Promise<void>;
  readonly showSaveOption: boolean;
}


export function usePasswordPrompt(
  connectingServerId?: string | null,
  onSavedServersChanged?: (servers: SavedServer[]) => void,
): PasswordPromptHandlers {
  const pendingConnect = useAppStore((s) => s.pendingConnect);
  const retryWithPassword = useAppStore((s) => s.retryWithPassword);
  const dismissPasswordPrompt = useAppStore((s) => s.dismissPasswordPrompt);
  const connect = useAppStore((s) => s.connect);

  const [savedServers, setSavedServers] = useState<SavedServer[]>([]);

  useEffect(() => {
    let cancelled = false;
    void getSavedServers().then((list) => {
      if (!cancelled) setSavedServers(list);
    });
    return () => { cancelled = true; };
  }, []);

  const matchingServerId = pendingConnect
    ? (savedServers.find(
        (s) =>
          s.host === pendingConnect.host &&
          s.port === pendingConnect.port &&
          s.username === pendingConnect.username,
      )?.id ?? null)
    : null;

  const handleSubmit = useCallback(
    async (password: string, save: boolean) => {
      const targetId = connectingServerId ?? matchingServerId;
      if (save && targetId) {
        await setServerPassword(targetId, password);
      }
      retryWithPassword(password);
    },
    [connectingServerId, matchingServerId, retryWithPassword],
  );

  const handleChangeUsername = useCallback(
    async (newUsername: string) => {
      if (!pendingConnect) return;
      const targetId = connectingServerId ?? matchingServerId;
      if (targetId) {
        await updateServer(targetId, { username: newUsername });
        const updated = savedServers.map((s) =>
          s.id === targetId ? { ...s, username: newUsername } : s,
        );
        setSavedServers(updated);
        onSavedServersChanged?.(updated);
      }
      // Tear down the failed session so the fresh connect reuses its tab.
      const failedSessionId = useAppStore.getState().activeServerId;
      dismissPasswordPrompt();
      if (failedSessionId) {
        try {
          await invoke("disconnect_server", { serverId: failedSessionId });
        } catch (_) { /* already torn down */ }
        await useAppStore.getState().refreshSessions().catch(() => {});
      }
      await connect(
        pendingConnect.host,
        pendingConnect.port,
        newUsername,
        pendingConnect.certLabel ?? null,
      );
    },
    [
      pendingConnect,
      connectingServerId,
      matchingServerId,
      savedServers,
      onSavedServersChanged,
      dismissPasswordPrompt,
      connect,
    ],
  );

  return {
    handleSubmit,
    handleChangeUsername,
    showSaveOption: matchingServerId !== null,
  };
}
