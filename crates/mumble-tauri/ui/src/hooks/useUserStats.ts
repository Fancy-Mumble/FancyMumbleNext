import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { UserStats } from "../types";

/**
 * Request and listen for a user's stats while `active` is true.
 * Returns the latest UserStats or null while loading / inactive.
 */
export function useUserStats(
  session: number | null,
  active: boolean,
): UserStats | null {
  const [stats, setStats] = useState<UserStats | null>(null);

  useEffect(() => {
    if (!active || session === null) {
      setStats(null);
      return;
    }

    invoke("request_user_stats", { session }).catch(() => {});

    const unlisten = listen<UserStats>("user-stats", (event) => {
      if (event.payload.session === session) {
        setStats(event.payload);
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [session, active]);

  return stats;
}
