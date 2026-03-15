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
  GroupChat,
  PersistenceMode,
  ChannelPersistenceState,
  KeyTrustState,
  CustodianPinState,
  PendingDispute,
  FetchHistoryResponse,
  ChannelPersistConfig,
} from "./types";
import type { PollPayload, PollVotePayload } from "./components/PollCreator";
import { registerPoll, registerVote } from "./components/PollCard";
import { offloadManager } from "./messageOffload";

// --- Store shape --------------------------------------------------

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

  // -- DM state --------------------------------------------------
  /** Session ID of the user whose DM chat is currently viewed. */
  selectedDmUser: number | null;
  /** DM messages for the currently viewed conversation. */
  dmMessages: ChatMessage[];
  /** DM unread counts keyed by user session. */
  dmUnreadCounts: Record<number, number>;

  // -- Group chat state ------------------------------------------
  /** All known group chats. */
  groupChats: GroupChat[];
  /** ID of the group chat currently being viewed (mutually exclusive with channel/DM). */
  selectedGroup: string | null;
  /** Messages for the currently viewed group chat. */
  groupMessages: ChatMessage[];
  /** Group unread counts keyed by group ID. */
  groupUnreadCounts: Record<string, number>;

  // -- Poll state (in-memory, not persisted) ---------------------
  /** All known polls keyed by poll ID. */
  polls: Map<string, PollPayload>;
  /** Synthetic local-only messages for rendering polls in the chat flow. */
  pollMessages: ChatMessage[];

  // -- Persistent chat state -------------------------------------
  /** Persistence metadata per channel (mode, retention, fetch state). */
  channelPersistence: Record<number, ChannelPersistenceState>;
  /** Key trust state per channel (trust level, fingerprints, distributor). */
  keyTrust: Record<number, KeyTrustState>;
  /** Custodian pin state per channel (TOFU pinning). */
  custodianPins: Record<number, CustodianPinState>;
  /** Pending key disputes per channel. */
  pendingDisputes: Record<number, PendingDispute>;

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

  // DM actions
  selectDmUser: (session: number) => Promise<void>;
  sendDm: (targetSession: number, body: string) => Promise<void>;
  refreshDmMessages: (session: number) => Promise<void>;

  // Group chat actions
  createGroup: (name: string, memberSessions: number[]) => Promise<void>;
  selectGroup: (groupId: string) => Promise<void>;
  sendGroupMessage: (groupId: string, body: string) => Promise<void>;
  refreshGroupMessages: (groupId: string) => Promise<void>;

  // Persistent chat actions
  fetchHistory: (channelId: number, beforeId?: string) => Promise<void>;
  getPersistenceMode: (channelId: number) => PersistenceMode;
  verifyKeyFingerprint: (channelId: number) => Promise<void>;
  acceptCustodianChanges: (channelId: number) => Promise<void>;
  confirmCustodians: (channelId: number) => Promise<void>;
  resolveKeyDispute: (channelId: number, trustedSenderHash: string) => Promise<void>;
  updateChannelPersistenceConfig: (channelId: number, config: ChannelPersistConfig) => void;
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
  | "selectedDmUser"
  | "dmMessages"
  | "dmUnreadCounts"
  | "groupChats"
  | "selectedGroup"
  | "groupMessages"
  | "groupUnreadCounts"
  | "polls"
  | "pollMessages"
  | "channelPersistence"
  | "keyTrust"
  | "custodianPins"
  | "pendingDisputes"
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
  selectedDmUser: null,
  dmMessages: [],
  dmUnreadCounts: {},
  groupChats: [],
  selectedGroup: null,
  groupMessages: [],
  groupUnreadCounts: {},
  polls: new Map(),
  pollMessages: [],
  channelPersistence: {},
  keyTrust: {},
  custodianPins: {},
  pendingDisputes: {},
};

// --- Store --------------------------------------------------------

/** Update the taskbar badge with the total unread count (channels + DMs + groups). */
function updateBadgeCount(): void {
  const { unreadCounts, dmUnreadCounts, groupUnreadCounts } = useAppStore.getState();
  const channelSum = Object.values(unreadCounts).reduce((a, b) => a + b, 0);
  const dmSum = Object.values(dmUnreadCounts).reduce((a, b) => a + b, 0);
  const groupSum = Object.values(groupUnreadCounts).reduce((a, b) => a + b, 0);
  const total = channelSum + dmSum + groupSum;
  invoke("update_badge_count", { count: total > 0 ? total : null }).catch(() => {
    // Badge API may not be available on all platforms.
  });
}

export const useAppStore = create<AppState>((set, get) => ({
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
      // Clean up offloaded temp files before resetting state.
      await offloadManager.dispose();
      await invoke("disconnect");
    } catch (e) {
      console.error("disconnect error:", e);
    }
    set({ ...INITIAL });
    invoke("update_badge_count", { count: null }).catch(() => {});
  },

  selectChannel: async (id) => {
    set({ selectedChannel: id, selectedDmUser: null, dmMessages: [], selectedGroup: null, groupMessages: [] });
    try {
      // Notify backend - marks channel as read and clears DM selection.
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

  selectDmUser: async (session) => {
    set({ selectedDmUser: session, selectedChannel: null, messages: [], selectedUser: session, selectedGroup: null, groupMessages: [] });
    try {
      await invoke("select_dm_user", { session });
      const dmMessages = await invoke<ChatMessage[]>("get_dm_messages", { session });
      set({ dmMessages });
    } catch (e) {
      console.error("select_dm_user error:", e);
    }
  },

  sendDm: async (targetSession, body) => {
    try {
      await invoke("send_dm", { targetSession, body });
      const dmMessages = await invoke<ChatMessage[]>("get_dm_messages", { session: targetSession });
      set({ dmMessages });
    } catch (e) {
      console.error("send_dm error:", e);
    }
  },

  refreshDmMessages: async (session) => {
    try {
      const dmMessages = await invoke<ChatMessage[]>("get_dm_messages", { session });
      set({ dmMessages });
    } catch (e) {
      console.error("refresh dm messages error:", e);
    }
  },

  // -- Group chat actions -----------------------------------------

  createGroup: async (name, memberSessions) => {
    try {
      // The backend emits a "group-created" event that the listener below
      // will pick up, so we do not append here to avoid duplicates.
      await invoke<GroupChat>("create_group", { name, memberSessions });
    } catch (e) {
      console.error("create_group error:", e);
    }
  },

  selectGroup: async (groupId) => {
    set({ selectedGroup: groupId, selectedChannel: null, messages: [], selectedDmUser: null, dmMessages: [] });
    try {
      await invoke("select_group", { groupId });
      const groupMessages = await invoke<ChatMessage[]>("get_group_messages", { groupId });
      set({ groupMessages });
    } catch (e) {
      console.error("select_group error:", e);
    }
  },

  sendGroupMessage: async (groupId, body) => {
    try {
      await invoke("send_group_message", { groupId, body });
      const groupMessages = await invoke<ChatMessage[]>("get_group_messages", { groupId });
      set({ groupMessages });
    } catch (e) {
      console.error("send_group_message error:", e);
    }
  },

  refreshGroupMessages: async (groupId) => {
    try {
      const groupMessages = await invoke<ChatMessage[]>("get_group_messages", { groupId });
      set({ groupMessages });
    } catch (e) {
      console.error("refresh group messages error:", e);
    }
  },

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

  // -- Persistent chat actions ------------------------------------

  fetchHistory: async (channelId, beforeId) => {
    set((prev) => ({
      channelPersistence: {
        ...prev.channelPersistence,
        [channelId]: {
          ...prev.channelPersistence[channelId],
          isFetching: true,
        },
      },
    }));
    try {
      const response = await invoke<FetchHistoryResponse>("fetch_persistent_messages", {
        channelId,
        beforeId: beforeId ?? null,
        limit: 50,
      });
      // Convert stored messages into ChatMessage format and prepend to existing messages.
      const historicMessages: ChatMessage[] = response.messages.map((m) => ({
        sender_session: null,
        sender_name: m.senderName,
        body: m.body,
        channel_id: m.channelId,
        is_own: false,
        message_id: m.messageId,
        timestamp: m.timestamp,
      }));
      set((prev) => ({
        messages: [...historicMessages, ...prev.messages],
        channelPersistence: {
          ...prev.channelPersistence,
          [channelId]: {
            ...prev.channelPersistence[channelId],
            hasMore: response.hasMore,
            isFetching: false,
            totalStored: response.totalStored,
          },
        },
      }));
    } catch (e) {
      console.error("fetch_persistent_messages error:", e);
      set((prev) => ({
        channelPersistence: {
          ...prev.channelPersistence,
          [channelId]: {
            ...prev.channelPersistence[channelId],
            isFetching: false,
          },
        },
      }));
    }
  },

  getPersistenceMode: (channelId) => {
    return get().channelPersistence[channelId]?.mode ?? "NONE";
  },

  verifyKeyFingerprint: async (channelId) => {
    try {
      await invoke("verify_channel_key_manual", { channelId });
      set((prev) => ({
        keyTrust: {
          ...prev.keyTrust,
          [channelId]: {
            ...prev.keyTrust[channelId],
            trustLevel: "ManuallyVerified",
          },
        },
      }));
    } catch (e) {
      console.error("verify_channel_key_manual error:", e);
    }
  },

  acceptCustodianChanges: async (channelId) => {
    try {
      await invoke("accept_custodian_changes", { channelId });
      set((prev) => {
        const pin = prev.custodianPins[channelId];
        if (!pin?.pendingUpdate) return {};
        return {
          custodianPins: {
            ...prev.custodianPins,
            [channelId]: {
              pinned: pin.pendingUpdate,
              confirmed: true,
              pendingUpdate: null,
            },
          },
        };
      });
    } catch (e) {
      console.error("accept_custodian_changes error:", e);
    }
  },

  confirmCustodians: async (channelId) => {
    try {
      const { custodianPins } = get();
      const pin = custodianPins[channelId];
      if (!pin) return;
      await invoke("confirm_custodians", {
        channelId,
        custodianHashes: pin.pinned,
      });
      set((prev) => ({
        custodianPins: {
          ...prev.custodianPins,
          [channelId]: { ...prev.custodianPins[channelId], confirmed: true },
        },
      }));
    } catch (e) {
      console.error("confirm_custodians error:", e);
    }
  },

  resolveKeyDispute: async (channelId, trustedSenderHash) => {
    try {
      await invoke("resolve_key_dispute", { channelId, trustedSenderHash });
      set((prev) => {
        const { [channelId]: _removed, ...rest } = prev.pendingDisputes;
        return {
          pendingDisputes: rest,
          keyTrust: {
            ...prev.keyTrust,
            [channelId]: {
              ...prev.keyTrust[channelId],
              trustLevel: "ManuallyVerified",
            },
          },
        };
      });
    } catch (e) {
      console.error("resolve_key_dispute error:", e);
    }
  },

  updateChannelPersistenceConfig: (channelId, config) => {
    set((prev) => ({
      channelPersistence: {
        ...prev.channelPersistence,
        [channelId]: {
          mode: config.mode,
          maxHistory: config.maxHistory,
          retentionDays: config.retentionDays,
          hasMore: false,
          isFetching: false,
          totalStored: prev.channelPersistence[channelId]?.totalStored ?? 0,
        },
      },
    }));
  },
}));

// --- Tauri event bridge -------------------------------------------

// --- Plugin data handler registry ---------------------------------

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
            // Default to the channel the user is in, falling back to the first channel.
            const defaultCh = currentCh ?? channels[0].id;
            useAppStore.getState().selectChannel(defaultCh);
          }
        });
    }),
  );

  // Connection dropped.
  unlisteners.push(
    await listen("server-disconnected", () => {
      // Clean up offloaded temp files.
      offloadManager.dispose().catch(() => {});
      // Preserve any error that was set by connection-rejected.
      const currentError = useAppStore.getState().error;
      useAppStore.setState({ ...INITIAL, error: currentError });
      invoke("update_badge_count", { count: null }).catch(() => {});
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

  // New direct message arrived.
  unlisteners.push(
    await listen<{ session: number }>("new-dm", async (event) => {
      const { selectedDmUser } = useAppStore.getState();
      if (selectedDmUser === event.payload.session) {
        await useAppStore
          .getState()
          .refreshDmMessages(event.payload.session);
      }
    }),
  );

  // Unread counts changed.
  unlisteners.push(
    await listen<{ unreads: Record<number, number> }>(
      "unread-changed",
      (event) => {
        useAppStore.setState({ unreadCounts: event.payload.unreads });
        updateBadgeCount();
      },
    ),
  );

  // DM unread counts changed.
  unlisteners.push(
    await listen<{ unreads: Record<number, number> }>(
      "dm-unread-changed",
      (event) => {
        useAppStore.setState({ dmUnreadCounts: event.payload.unreads });
        updateBadgeCount();
      },
    ),
  );

  // -- Group chat events ------------------------------------------

  // A new group chat was created (locally or by another member).
  unlisteners.push(
    await listen<{ group: GroupChat }>("group-created", (event) => {
      const group = event.payload.group;
      useAppStore.setState((prev) => {
        // Avoid duplicates.
        if (prev.groupChats.some((g) => g.id === group.id)) return {};
        return { groupChats: [...prev.groupChats, group] };
      });
    }),
  );

  // New group message arrived.
  unlisteners.push(
    await listen<{ group_id: string }>("new-group-message", async (event) => {
      const { selectedGroup } = useAppStore.getState();
      if (selectedGroup === event.payload.group_id) {
        await useAppStore
          .getState()
          .refreshGroupMessages(event.payload.group_id);
      }
    }),
  );

  // Group unread counts changed.
  unlisteners.push(
    await listen<{ unreads: Record<string, number> }>(
      "group-unread-changed",
      (event) => {
        useAppStore.setState({ groupUnreadCounts: event.payload.unreads });
        updateBadgeCount();
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

  // -- Persistent chat events -------------------------------------

  unlisteners.push(
    // Channel persistence config changed (from ChannelState updates).
    await listen<{ channel_id: number; config: ChannelPersistConfig }>(
      "persistence-config-changed",
      (event) => {
        const { channel_id, config } = event.payload;
        useAppStore.getState().updateChannelPersistenceConfig(channel_id, config);
      },
    ),

    // Key trust level changed for a channel.
    await listen<{ channel_id: number; trust: KeyTrustState }>(
      "key-trust-changed",
      (event) => {
        const { channel_id, trust } = event.payload;
        useAppStore.setState((prev) => ({
          keyTrust: { ...prev.keyTrust, [channel_id]: trust },
        }));
      },
    ),

    // Custodian list changed (TOFU change detection).
    await listen<{ channel_id: number; pin: CustodianPinState }>(
      "custodian-pin-changed",
      (event) => {
        const { channel_id, pin } = event.payload;
        useAppStore.setState((prev) => ({
          custodianPins: { ...prev.custodianPins, [channel_id]: pin },
        }));
      },
    ),

    // Key dispute detected.
    await listen<{ channel_id: number; dispute: PendingDispute }>(
      "key-dispute-detected",
      (event) => {
        const { channel_id, dispute } = event.payload;
        useAppStore.setState((prev) => ({
          pendingDisputes: { ...prev.pendingDisputes, [channel_id]: dispute },
        }));
      },
    ),

    // Key dispute resolved (by custodian shortcut or timeout).
    await listen<{ channel_id: number }>(
      "key-dispute-resolved",
      (event) => {
        const { channel_id } = event.payload;
        useAppStore.setState((prev) => {
          const { [channel_id]: _removed, ...rest } = prev.pendingDisputes;
          return { pendingDisputes: rest };
        });
      },
    ),
  );

  return unlisteners;
}
