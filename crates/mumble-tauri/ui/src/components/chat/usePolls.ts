import { useState, useCallback, useReducer, useRef } from "react";
import { useAppStore } from "../../store";
import type { PollPayload, PollVotePayload } from "./PollCreator";
import { registerVote, registerLocalVote, getPoll } from "./PollCard";

export function usePolls() {
  const users = useAppStore((s) => s.users);
  const ownSession = useAppStore((s) => s.ownSession);
  const selectedChannel = useAppStore((s) => s.selectedChannel);
  const sendPluginData = useAppStore((s) => s.sendPluginData);
  const addPoll = useAppStore((s) => s.addPoll);
  const polls = useAppStore((s) => s.polls);
  const pollMessages = useAppStore((s) => s.pollMessages);

  const [, forceRender] = useReducer((c: number) => c + 1, 0);
  const [showPollCreator, setShowPollCreator] = useState(false);

  const usersRef = useRef(users);
  usersRef.current = users;

  const handlePollCreate = useCallback(
    async (question: string, options: string[], multiple: boolean) => {
      if (selectedChannel === null) return;

      const currentUsers = usersRef.current;
      const ownUser = currentUsers.find((u) => u.session === ownSession);
      const pollId = crypto.randomUUID();
      const poll: PollPayload = {
        type: "poll",
        id: pollId,
        question,
        options,
        multiple,
        creator: ownSession ?? 0,
        creatorName: ownUser?.name ?? "",
        createdAt: new Date().toISOString(),
        channelId: selectedChannel,
      };

      // Register locally via the Zustand store.
      addPoll(poll, true);

      // The Mumble server only forwards PluginDataTransmission to
      // explicitly listed sessions - an empty list means nobody receives it.
      const targets = currentUsers
        .filter((u) => u.channel_id === selectedChannel && u.session !== ownSession)
        .map((u) => u.session);
      const data = new TextEncoder().encode(JSON.stringify(poll));
      await sendPluginData(targets, data, "fancy-poll");
    },
    [selectedChannel, sendPluginData, ownSession, addPoll],
  );

  const handlePollVote = useCallback(
    async (pollId: string, selected: number[]) => {
      const currentUsers = usersRef.current;
      const ownUser = currentUsers.find((u) => u.session === ownSession);
      const vote: PollVotePayload = {
        type: "poll_vote",
        pollId,
        selected,
        voter: ownSession ?? 0,
        voterName: ownUser?.name ?? "",
      };

      registerVote(vote);
      registerLocalVote(pollId, selected);
      forceRender();

      // Look up the poll to determine its channel for targeting.
      const pollData = getPoll(pollId);
      const targetChannel = pollData?.channelId ?? selectedChannel ?? 0;

      // The Mumble server requires explicit receiver sessions.
      const targets = currentUsers
        .filter((u) => u.channel_id === targetChannel && u.session !== ownSession)
        .map((u) => u.session);
      const data = new TextEncoder().encode(JSON.stringify(vote));
      await sendPluginData(targets, data, "fancy-poll-vote");
    },
    [sendPluginData, ownSession, selectedChannel],
  );

  const openPollCreator = useCallback(() => setShowPollCreator(true), []);
  const closePollCreator = useCallback(() => setShowPollCreator(false), []);

  return {
    polls,
    pollMessages,
    showPollCreator,
    openPollCreator,
    closePollCreator,
    handlePollCreate,
    handlePollVote,
  };
}
