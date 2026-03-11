/**
 * Global Zustand store for the Mumble Tauri client.
 *
 * All complex logic lives in the Rust backend - the frontend only
 * invokes Tauri commands and reacts to events.
 */

import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  ChannelEntry,
  UserEntry,
  ChatMessage,
  ConnectionStatus,
  MumbleServerConfig,
  VoiceState,
} from "./types";
import type { PollPayload, PollVotePayload } from "./components/PollCreator";
import { registerPoll, registerVote } from "./components/PollCard";

// ─── Store shape ──────────────────────────────────────────────────

interface AppState {
  // Reactive state
  status: ConnectionStatus;
  channels: ChannelEntry[];
  users: UserEntry[];
  selectedChannel: number | null;
  /** The channel the user is physically in on the server. */
  currentChannel: number | null;
  /** Session ID of the user whose profile panel is open (right side). */
  selectedUser: number | null;
  /** Our own session ID assigned by the server after connecting. */
  ownSession: number | null;
  messages: ChatMessage[];
  error: string | null;
  listenedChannels: Set<number>;
  unreadCounts: Record<number, number>;
  serverConfig: MumbleServerConfig;
  voiceState: VoiceState;

  // ── Poll state (in-memory, not persisted) ─────────────────────
  /** All known polls keyed by poll ID. */
  polls: Map<string, PollPayload>;
  /** Synthetic local-only messages for rendering polls in the chat flow. */
  pollMessages: ChatMessage[];

  // Actions
  connect: (host: string, port: number, username: string, certLabel?: string | null) => Promise<void>;
  disconnect: () => Promise<void>;
  selectChannel: (id: number) => Promise<void>;
  joinChannel: (id: number) => Promise<void>;
  sendMessage: (channelId: number, body: string) => Promise<void>;
  toggleListen: (channelId: number) => Promise<void>;
  refreshState: () => Promise<void>;
  refreshMessages: (channelId: number) => Promise<void>;
  enableVoice: () => Promise<void>;
  disableVoice: () => Promise<void>;
  toggleMute: () => Promise<void>;
  toggleDeafen: () => Promise<void>;
  selectUser: (session: number | null) => void;
  sendPluginData: (receiverSessions: number[], data: Uint8Array, dataId: string) => Promise<void>;
  /** Add a poll to the store (called locally when creating a poll). */
  addPoll: (poll: PollPayload, isOwn: boolean) => void;
  setError: (error: string | null) => void;
  reset: () => void;
}

const INITIAL: Pick<
  AppState,
  | "status"
  | "channels"
  | "users"
  | "selectedChannel"
  | "currentChannel"
  | "selectedUser"
  | "ownSession"
  | "messages"
  | "error"
  | "listenedChannels"
  | "unreadCounts"
  | "serverConfig"
  | "voiceState"
  | "polls"
  | "pollMessages"
> = {
  status: "disconnected",
  channels: [],
  users: [],
  selectedChannel: null,
  currentChannel: null,
  selectedUser: null,
  ownSession: null,
  messages: [],
  error: null,
  listenedChannels: new Set(),
  unreadCounts: {},
  serverConfig: {
    max_message_length: 5000,
    max_image_message_length: 131072,
    allow_html: true,
  },
  voiceState: "inactive" as VoiceState,
  polls: new Map(),
  pollMessages: [],
};

// ─── Store ────────────────────────────────────────────────────────

export const useAppStore = create<AppState>((set) => ({
  ...INITIAL,

  connect: async (host, port, username, certLabel) => {
    set({ status: "connecting", error: null });
    try {
      await invoke("connect", { host, port, username, certLabel: certLabel ?? null });
    } catch (e) {
      set({ status: "disconnected", error: String(e) });
    }
  },

  disconnect: async () => {
    try {
      await invoke("disconnect");
    } catch (e) {
      console.error("disconnect error:", e);
    }
    set({ ...INITIAL });
  },

  selectChannel: async (id) => {
    set({ selectedChannel: id });
    try {
      // Notify backend - marks channel as read.
      await invoke("select_channel", { channelId: id });
      const messages = await invoke<ChatMessage[]>("get_messages", {
        channelId: id,
      });
      set({ messages });
    } catch (e) {
      console.error("select_channel error:", e);
    }
  },

  joinChannel: async (id) => {
    try {
      await invoke("join_channel", { channelId: id });
      set({ currentChannel: id });
    } catch (e) {
      console.error("join_channel error:", e);
    }
  },

  sendMessage: async (channelId, body) => {
    try {
      await invoke("send_message", { channelId, body });
      const messages = await invoke<ChatMessage[]>("get_messages", {
        channelId,
      });
      set({ messages });
    } catch (e) {
      console.error("send_message error:", e);
    }
  },

  refreshState: async () => {
    try {
      const [channels, users] = await Promise.all([
        invoke<ChannelEntry[]>("get_channels"),
        invoke<UserEntry[]>("get_users"),
      ]);
      set({ channels, users });
    } catch (e) {
      console.error("refresh error:", e);
    }
  },

  refreshMessages: async (channelId) => {
    try {
      const messages = await invoke<ChatMessage[]>("get_messages", {
        channelId,
      });
      set({ messages });
    } catch (e) {
      console.error("refresh messages error:", e);
    }
  },

  toggleListen: async (channelId) => {
    try {
      const isNowListened = await invoke<boolean>("toggle_listen", {
        channelId,
      });
      set((prev) => {
        const next = new Set(prev.listenedChannels);
        if (isNowListened) next.add(channelId);
        else next.delete(channelId);
        return { listenedChannels: next };
      });
    } catch (e) {
      console.error("toggle_listen error:", e);
    }
  },

  enableVoice: async () => {
    try {
      await invoke("enable_voice");
      set({ voiceState: "active" });
    } catch (e) {
      console.error("enable_voice error:", e);
    }
  },

  disableVoice: async () => {
    try {
      await invoke("disable_voice");
      set({ voiceState: "inactive" });
    } catch (e) {
      console.error("disable_voice error:", e);
    }
  },

  toggleMute: async () => {
    try {
      await invoke("toggle_mute");
    } catch (e) {
      console.error("toggle_mute error:", e);
    }
  },

  toggleDeafen: async () => {
    try {
      await invoke("toggle_deafen");
    } catch (e) {
      console.error("toggle_deafen error:", e);
    }
  },

  selectUser: (session) => set({ selectedUser: session }),
  sendPluginData: async (receiverSessions, data, dataId) => {
    try {
      await invoke("send_plugin_data", {
        receiverSessions,
        data: Array.from(data),
        dataId,
      });
    } catch (e) {
      console.error("send_plugin_data error:", e);
    }
  },
  addPoll: (poll, isOwn) => {
    registerPoll(poll);
    set((prev) => {
      const newPolls = new Map(prev.polls).set(poll.id, poll);
      // Avoid duplicate synthetic messages.
      if (prev.pollMessages.some((m) => m.body.includes(poll.id))) {
        return { polls: newPolls };
      }
      return {
        polls: newPolls,
        pollMessages: [
          ...prev.pollMessages,
          {
            sender_session: poll.creator,
            sender_name: poll.creatorName || "Unknown",
            body: `<!-- FANCY_POLL:${poll.id} -->`,
            channel_id: poll.channelId ?? 0,
            is_own: isOwn,
          },
        ],
      };
    });
  },
  setError: (error) => set({ error }),
  reset: () => set({ ...INITIAL }),
}));

// ─── Tauri event bridge ───────────────────────────────────────────

// ─── Plugin data handler registry ─────────────────────────────────

type PluginDataHandler = (dataId: string, data: Uint8Array, senderSession: number | null) => void;
const pluginDataHandlers: PluginDataHandler[] = [];

/** Register a handler for incoming plugin data transmissions. */
export function onPluginData(handler: PluginDataHandler): () => void {
  pluginDataHandlers.push(handler);
  return () => {
    const idx = pluginDataHandlers.indexOf(handler);
    if (idx >= 0) pluginDataHandlers.splice(idx, 1);
  };
}

/**
 * Subscribe to backend events and translate them into store updates.
 * Call once from the root `<App>` component; returns cleanup functions.
 */
export async function initEventListeners(
  navigate: (path: string) => void,
): Promise<UnlistenFn[]> {
  const unlisteners: UnlistenFn[] = [];

  // Server fully connected (ServerSync received).
  unlisteners.push(
    await listen("server-connected", () => {
      // Navigate immediately - don't block on data fetching.
      useAppStore.setState({ status: "connected" });
      navigate("/chat");

      // Load channels/users/messages lazily in the background.
      useAppStore
        .getState()
        .refreshState()
        .then(async () => {
          // Fetch the channel the user is currently in.
          const currentCh = await invoke<number | null>("get_current_channel");
          if (currentCh !== null) {
            useAppStore.setState({ currentChannel: currentCh });
          }

          // Fetch our own session ID.
          const ownSession = await invoke<number | null>("get_own_session");
          useAppStore.setState({ ownSession });

          const { channels, selectedChannel } = useAppStore.getState();
          if (selectedChannel === null && channels.length > 0) {
            useAppStore.getState().selectChannel(channels[0].id);
          }
        });
    }),
  );

  // Connection dropped.
  unlisteners.push(
    await listen("server-disconnected", () => {
      // Preserve any error that was set by connection-rejected.
      const currentError = useAppStore.getState().error;
      useAppStore.setState({ ...INITIAL, error: currentError });
      navigate("/");
    }),
  );

  // Channel / user list changed - debounce rapid-fire updates.
  let stateChangeTimer: ReturnType<typeof setTimeout> | undefined;
  unlisteners.push(
    await listen("state-changed", () => {
      clearTimeout(stateChangeTimer);
      stateChangeTimer = setTimeout(() => {
        useAppStore.getState().refreshState();
      }, 100);
    }),
  );

  // New text message arrived.
  unlisteners.push(
    await listen<{ channel_id: number }>("new-message", async (event) => {
      const { selectedChannel } = useAppStore.getState();
      if (selectedChannel === event.payload.channel_id) {
        await useAppStore
          .getState()
          .refreshMessages(event.payload.channel_id);
      }
    }),
  );

  // Unread counts changed.
  unlisteners.push(
    await listen<{ unreads: Record<number, number> }>(
      "unread-changed",
      (event) => {
        useAppStore.setState({ unreadCounts: event.payload.unreads });
      },
    ),
  );

  // Server rejected the connection.
  unlisteners.push(
    await listen<{ reason: string }>("connection-rejected", (event) => {
      useAppStore.setState({
        status: "disconnected",
        error: event.payload.reason,
      });
      navigate("/");
    }),
  );

  // Listen request was denied by the server - revert the UI.
  unlisteners.push(
    await listen<{ channel_id: number }>("listen-denied", (event) => {
      useAppStore.setState((prev) => {
        const next = new Set(prev.listenedChannels);
        next.delete(event.payload.channel_id);
        return { listenedChannels: next };
      });
    }),
  );

  // Our own user moved to a different channel.
  unlisteners.push(
    await listen<{ channel_id: number }>("current-channel-changed", (event) => {
      useAppStore.setState({ currentChannel: event.payload.channel_id });
    }),
  );

  // Voice state changed (enable/disable voice calling).
  unlisteners.push(
    await listen<VoiceState>("voice-state-changed", (event) => {
      useAppStore.setState({ voiceState: event.payload });
    }),
  );

  // Server config received (limits, allow_html, etc.).
  unlisteners.push(
    await listen("server-config", async () => {
      try {
        const cfg = await invoke<MumbleServerConfig>("get_server_config");
        useAppStore.setState({ serverConfig: cfg });
      } catch (e) {
        console.error("get_server_config error:", e);
      }
    }),
  );

  // Plugin data received (polls, etc.).
  // Process polls and votes directly here so the data reaches the
  // Zustand store even across Vite HMR reloads and React StrictMode
  // double-mounts where the old handler-array dispatch could fail.
  unlisteners.push(
    await listen<{ sender_session: number | null; data: number[]; data_id: string }>(
      "plugin-data",
      (event) => {
        const { data_id, data, sender_session } = event.payload;
        const bytes = new Uint8Array(data);

        if (data_id === "fancy-poll" || data_id === "fancy-poll-vote") {
          try {
            const json = new TextDecoder().decode(bytes);
            const payload = JSON.parse(json);

            if (data_id === "fancy-poll" && payload.type === "poll") {
              const poll = payload as PollPayload;
              poll.creator = sender_session ?? poll.creator;
              // Resolve creator name from current users.
              const users = useAppStore.getState().users;
              const user = users.find((u) => u.session === poll.creator);
              if (user) poll.creatorName = user.name;
              useAppStore.getState().addPoll(poll, false);
            } else if (data_id === "fancy-poll-vote" && payload.type === "poll_vote") {
              const vote = payload as PollVotePayload;
              vote.voter = sender_session ?? vote.voter;
              const users = useAppStore.getState().users;
              const user = users.find((u) => u.session === vote.voter);
              if (user) vote.voterName = user.name;
              registerVote(vote);
              // Trigger re-render for any component reading polls.
              useAppStore.setState({});
            }
          } catch (e) {
            console.error("plugin-data poll processing error:", e);
          }
        }

        // Also dispatch to legacy registered handlers for extensibility.
        for (const handler of pluginDataHandlers) {
          handler(data_id, bytes, sender_session);
        }
      },
    ),
  );

  return unlisteners;
}
