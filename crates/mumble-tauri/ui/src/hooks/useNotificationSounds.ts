import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import type { NotificationSoundSettings, NotificationEvent, VoiceState, UserEntry } from "../types";
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

export function playSoundForEvent(
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
  const prevChannelRef = useRef<number | null>(null);
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
      listen("webrtc-signal", (event) => {
        const { signal_type } = event.payload as { signal_type: number };
        // signal_type 0 = START
        if (signal_type === 0) {
          playSoundForEvent(settingsRef.current, "streamStart");
        }
      }),
    );

    // Self-mention notification (dispatched from MessageItem when a
    // newly-rendered message contains an @-mention targeting the user).
    const onSelfMention = () => {
      playSoundForEvent(settingsRef.current, "mention");
    };
    globalThis.addEventListener("fancy:self-mention", onSelfMention);

    return () => {
      globalThis.removeEventListener("fancy:self-mention", onSelfMention);
      for (const p of unlisteners) {
        p.then((f) => f());
      }
    };
  }, []);

  // User join/leave (server-wide), channel join/leave, voice activity, self-mute
  useEffect(() => {
    // Cached scalar slices used to fast-bail on unrelated state updates.
    // The Zustand store fires its subscriber on every state mutation
    // (including audio packets and chat messages), so without these checks
    // we would walk `state.users` and rebuild a Set on every UDP frame.
    let lastUsersRef: readonly UserEntry[] | null = null;
    let lastTalkingRef: ReadonlySet<number> | null = null;
    let lastVoiceState: VoiceState | null = null;
    let lastChannel: number | null = null;
    let lastOwnSession: number | null | undefined = undefined;
    let lastActiveServerId: string | null = null;

    const resetPerServerRefs = () => {
      prevUserCountRef.current = null;
      prevChannelUsersRef.current = null;
      prevChannelRef.current = null;
    };

    const unsub = useAppStore.subscribe((state) => {
      // When the active server changes, reset all per-server counters so
      // the first snapshot from the new server does not trigger spurious
      // join/leave/channel sounds.
      if (state.activeServerId !== lastActiveServerId) {
        lastActiveServerId = state.activeServerId;
        resetPerServerRefs();
        // Also reset cached refs so we don't compare stale slices.
        lastUsersRef = null;
        lastTalkingRef = null;
        lastVoiceState = null;
        lastChannel = null;
        lastOwnSession = undefined;
        return;
      }

      const usersChanged = state.users !== lastUsersRef;
      const talkingChanged = state.talkingSessions !== lastTalkingRef;
      const voiceChanged = state.voiceState !== lastVoiceState;
      const channelChanged = state.currentChannel !== lastChannel;
      const ownChanged = state.ownSession !== lastOwnSession;

      if (!usersChanged && !talkingChanged && !voiceChanged && !channelChanged && !ownChanged) {
        return;
      }

      lastUsersRef = state.users;
      lastTalkingRef = state.talkingSessions;
      lastVoiceState = state.voiceState;
      lastChannel = state.currentChannel;
      lastOwnSession = state.ownSession;

      // Server-wide user join/leave (own session excluded so connecting doesn't trigger it)
      if (usersChanged || ownChanged) {
        const userCount = state.users.reduce(
          (acc, u) => (u.session !== state.ownSession ? acc + 1 : acc),
          0,
        );
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
      }

      // Channel-specific user join/leave
      const myChannel = state.currentChannel;
      const mySession = state.ownSession;
      if (myChannel !== null && (usersChanged || channelChanged || ownChanged)) {
        const channelUsers = new Set<number>();
        for (const u of state.users) {
          if (u.channel_id === myChannel && u.session !== mySession) {
            channelUsers.add(u.session);
          }
        }
        const prevSet = prevChannelUsersRef.current;
        const prevChannel = prevChannelRef.current;
        // Only compare against the same channel - skip when we ourselves changed channel
        // to avoid false join/leave sounds for the other users in the old/new channels.
        if (prevSet !== null && prevChannel === myChannel) {
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
        prevChannelRef.current = myChannel;
      }

      // Voice activity
      if (talkingChanged) {
        const talkingCount = state.talkingSessions.size;
        if (talkingCount > prevTalkingCountRef.current) {
          playSoundForEvent(settingsRef.current, "voiceActivity");
        }
        prevTalkingCountRef.current = talkingCount;
      }

      // Self-mute detection
      if (voiceChanged) {
        const vs = state.voiceState;
        if (prevVoiceStateRef.current !== null && vs !== prevVoiceStateRef.current) {
          if (vs === "muted" || (prevVoiceStateRef.current === "muted" && vs === "active")) {
            playSoundForEvent(settingsRef.current, "selfMuted");
          }
        }
        prevVoiceStateRef.current = vs;
      }
    });
    return unsub;
  }, []);
}
