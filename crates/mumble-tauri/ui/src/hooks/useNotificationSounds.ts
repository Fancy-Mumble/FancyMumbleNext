import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import type { NotificationSoundSettings, NotificationEvent, VoiceState } from "../types";
import { SOUND_OPTIONS } from "../pages/settings/NotificationsPanel";
import { useAppStore } from "../store";

function findSoundUrl(id: string): string {
  return SOUND_OPTIONS.find((s) => s.id === id)?.url ?? "";
}

function playSound(url: string, volume: number) {
  if (!url) return;
  const audio = new Audio(url);
  audio.volume = volume;
  audio.play().catch(() => {});
}

function playSoundForEvent(
  settings: NotificationSoundSettings,
  event: NotificationEvent,
) {
  if (!settings.masterEnabled) return;
  const cfg = settings.events[event];
  if (!cfg?.enabled || cfg.sound === "none") return;
  const url = findSoundUrl(cfg.sound);
  playSound(url, cfg.volume);
}

export function useNotificationSounds(
  settings: NotificationSoundSettings,
) {
  const settingsRef = useRef(settings);
  settingsRef.current = settings;

  const prevUserCountRef = useRef<number | null>(null);
  const prevTalkingCountRef = useRef<number>(0);
  const prevChannelUsersRef = useRef<Set<number> | null>(null);
  const prevVoiceStateRef = useRef<VoiceState | null>(null);

  useEffect(() => {
    const unlisteners: Promise<() => void>[] = [];

    unlisteners.push(
      listen("new-message", () => {
        playSoundForEvent(settingsRef.current, "chatMessage");
      }),
    );

    unlisteners.push(
      listen("new-dm", () => {
        playSoundForEvent(settingsRef.current, "directMessage");
      }),
    );

    unlisteners.push(
      listen("new-group-message", () => {
        playSoundForEvent(settingsRef.current, "directMessage");
      }),
    );

    unlisteners.push(
      listen("webrtc-signal", (event) => {
        const { signal_type } = event.payload as { signal_type: number };
        // signal_type 0 = START
        if (signal_type === 0) {
          playSoundForEvent(settingsRef.current, "streamStart");
        }
      }),
    );

    return () => {
      for (const p of unlisteners) {
        p.then((f) => f());
      }
    };
  }, []);

  // User join/leave (server-wide), channel join/leave, voice activity, self-mute
  useEffect(() => {
    const unsub = useAppStore.subscribe((state) => {
      // Server-wide user join/leave
      const userCount = state.users.length;
      const prev = prevUserCountRef.current;
      if (prev === null) {
        prevUserCountRef.current = userCount;
      } else if (userCount > prev) {
        playSoundForEvent(settingsRef.current, "userJoin");
        prevUserCountRef.current = userCount;
      } else if (userCount < prev) {
        playSoundForEvent(settingsRef.current, "userLeave");
        prevUserCountRef.current = userCount;
      }

      // Channel-specific user join/leave
      const myChannel = state.currentChannel;
      const mySession = state.ownSession;
      if (myChannel !== null) {
        const channelUsers = new Set(
          state.users
            .filter((u) => u.channel_id === myChannel && u.session !== mySession)
            .map((u) => u.session),
        );
        const prevSet = prevChannelUsersRef.current;
        if (prevSet !== null) {
          for (const session of channelUsers) {
            if (!prevSet.has(session)) {
              playSoundForEvent(settingsRef.current, "userJoinChannel");
              break;
            }
          }
          for (const session of prevSet) {
            if (!channelUsers.has(session)) {
              playSoundForEvent(settingsRef.current, "userLeaveChannel");
              break;
            }
          }
        }
        prevChannelUsersRef.current = channelUsers;
      }

      // Voice activity
      const talkingCount = state.talkingSessions.size;
      if (talkingCount > prevTalkingCountRef.current) {
        playSoundForEvent(settingsRef.current, "voiceActivity");
      }
      prevTalkingCountRef.current = talkingCount;

      // Self-mute detection
      const vs = state.voiceState;
      if (prevVoiceStateRef.current !== null && vs !== prevVoiceStateRef.current) {
        if (vs === "muted" || (prevVoiceStateRef.current === "muted" && vs === "active")) {
          playSoundForEvent(settingsRef.current, "selfMuted");
        }
      }
      prevVoiceStateRef.current = vs;
    });
    return unsub;
  }, []);
}
