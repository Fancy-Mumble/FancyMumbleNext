import React from "react";
import type { ChatMessage, TimeFormat, UserEntry } from "../../types";
import MessageItem, { MessageAvatar } from "./MessageItem";
import MessageActionBar from "../elements/MessageActionBar";
import CheckIcon from "../../assets/icons/status/check.svg?react";
import { dateKey, formatDateChip } from "../../utils/format";
import { isHeavyContent } from "../../messageOffload";
import type { PollPayload } from "./PollCreator";
import { isMobile } from "../../utils/platform";
import styles from "./ChatView.module.css";

interface ChatMessageListProps {
  readonly allMessages: ChatMessage[];
  readonly userBySession: Map<number, UserEntry>;
  readonly avatarBySession: Map<number, string>;
  readonly convertToLocalTime: boolean;
  readonly bubbleStyle: string;
  readonly lastReadIdx: number | null;
  readonly selectionMode: boolean;
  readonly canDelete: boolean;
  readonly selectedMsgIds: Set<string>;
  readonly restoringKeys: Set<string>;
  readonly polls: Map<string, PollPayload>;
  readonly ownSession: number | null;
  readonly timeFormat: TimeFormat;
  readonly systemUses24h: boolean | undefined;
  readonly selectUser: (session: number) => void;
  readonly handleMessageContextMenu: (e: React.MouseEvent, msg: ChatMessage) => void;
  readonly toggleMsgSelection: (msgId: string) => void;
  readonly handleCite: (msg: ChatMessage) => void;
  readonly handleTouchStart: (msg: ChatMessage) => void;
  readonly cancelLongPress: () => void;
  readonly handleReaction: (msg: ChatMessage, emoji: string) => void;
  readonly handleMoreReactions: (msg: ChatMessage) => void;
  readonly handleCopyText: (msg: ChatMessage) => void;
  readonly handleSingleDelete: (msg: ChatMessage) => void;
  readonly handlePollVote: (pollId: string, selected: number[]) => Promise<void>;
  readonly handleScrollToMessage: (messageId: string) => void;
  readonly handleOpenLightbox: (src: string) => void;
}

interface MsgGroup {
  senderId: number | null;
  isOwn: boolean;
  startIdx: number;
  messages: ChatMessage[];
  day: string;
}

export default function ChatMessageList({
  allMessages,
  userBySession,
  avatarBySession,
  convertToLocalTime,
  bubbleStyle,
  lastReadIdx,
  selectionMode,
  canDelete,
  selectedMsgIds,
  restoringKeys,
  polls,
  ownSession,
  timeFormat,
  systemUses24h,
  selectUser,
  handleMessageContextMenu,
  toggleMsgSelection,
  handleCite,
  handleTouchStart,
  cancelLongPress,
  handleReaction,
  handleMoreReactions,
  handleCopyText,
  handleSingleDelete,
  handlePollVote,
  handleScrollToMessage,
  handleOpenLightbox,
}: ChatMessageListProps) {
  // Group consecutive messages from the same sender,
  // also breaking on date boundaries so date chips render between groups.
  const groups: MsgGroup[] = [];
  for (const [i, msg] of allMessages.entries()) {
    const msgDay = msg.timestamp ? dateKey(msg.timestamp, convertToLocalTime) : "";
    const prev = groups[groups.length - 1];
    if (prev?.senderId === msg.sender_session && prev.isOwn === msg.is_own && prev.day === msgDay) {
      prev.messages.push(msg);
    } else {
      groups.push({ senderId: msg.sender_session, isOwn: msg.is_own, startIdx: i, messages: [msg], day: msgDay });
    }
  }

  let lastDay = "";
  return (
    <>
      {groups.map((group) => {
        const firstGlobalIdx = group.startIdx;
        const firstMsg = group.messages[0];
        const groupKey = firstMsg.message_id ?? `${firstMsg.channel_id}-${firstMsg.sender_session ?? "s"}-${firstGlobalIdx}`;
        const senderUser = group.senderId === null ? undefined : userBySession.get(group.senderId);
        const senderAvatar = group.senderId === null ? undefined : avatarBySession.get(group.senderId);

        // Show date chip when the day changes.
        let dateChip: React.ReactNode = null;
        if (group.day && group.day !== lastDay) {
          const label = formatDateChip(firstMsg.timestamp!, convertToLocalTime);
          dateChip = (
            <div key={`date-${group.day}`} className={styles.dateDivider} aria-label={label}>
              <span className={styles.dateDividerLabel}>{label}</span>
            </div>
          );
          lastDay = group.day;
        }

        return (
          <React.Fragment key={groupKey}>
            {dateChip}
            <div
              className={`${styles.messageGroup} ${group.isOwn ? styles.messageGroupOwn : ""}`}
            >
              {/* Sticky avatar column: always shown in flat style, others-only otherwise */}
              {(!group.isOwn || bubbleStyle === "flat") && (
                <div className={styles.avatarColumn}>
                  <MessageAvatar
                    senderSession={group.senderId}
                    senderName={firstMsg.sender_name}
                    avatarUrl={senderAvatar}
                    user={senderUser}
                    onAvatarClick={selectUser}
                  />
                </div>
              )}
              {/* Bubble column */}
              <div className={styles.bubbleColumn}>
                {group.messages.map((msg, j) => {
                  const globalIdx = firstGlobalIdx + j;
                  const hasMsgId = !!msg.message_id;
                  const isSelected = hasMsgId && selectedMsgIds.has(msg.message_id!);
                  return (
                    <React.Fragment key={msg.message_id ?? `${msg.channel_id}-${msg.sender_session ?? "s"}-${msg.body.slice(0, 32)}-${globalIdx}`}>
                      {lastReadIdx !== null && globalIdx === lastReadIdx && (
                        <div className={styles.unreadDivider} aria-label="New messages">
                          <span className={styles.unreadDividerLabel}>New messages</span>
                        </div>
                      )}
                      <div
                        className={[
                          styles.actionBarWrapper,
                          selectionMode && canDelete && hasMsgId ? styles.messageRowSelectable : "",
                          selectionMode && canDelete && hasMsgId ? styles.selectableRow : "",
                          isSelected ? styles.selectedRow : "",
                        ].join(" ")}
                        data-msg-id={msg.message_id ?? undefined}
                        data-msg-heavy={msg.message_id && isHeavyContent(msg.body) ? "" : undefined}
                        onContextMenu={hasMsgId && !selectionMode ? (e) => handleMessageContextMenu(e, msg) : undefined}
                        onClick={selectionMode && canDelete && hasMsgId ? () => toggleMsgSelection(msg.message_id!) : undefined}
                        onDoubleClick={hasMsgId && !selectionMode && !isMobile ? () => handleCite(msg) : undefined}
                        onTouchStart={hasMsgId && !selectionMode ? () => handleTouchStart(msg) : undefined}
                        onTouchEnd={selectionMode ? undefined : cancelLongPress}
                        onTouchMove={selectionMode ? undefined : cancelLongPress}
                      >
                        {!selectionMode && !isMobile && (
                          <MessageActionBar
                            message={msg}
                            isOwn={msg.is_own}
                            onReaction={handleReaction}
                            onMoreReactions={handleMoreReactions}
                            onCite={handleCite}
                            onCopyText={handleCopyText}
                            onDelete={canDelete ? handleSingleDelete : undefined}
                            canDelete={canDelete && hasMsgId}
                          />
                        )}
                        <MessageItem
                          msg={msg}
                          index={globalIdx}
                          avatarUrl={senderAvatar}
                          user={senderUser}
                          polls={polls}
                          ownSession={ownSession}
                          onVote={handlePollVote}
                          onAvatarClick={selectUser}
                          timeFormat={timeFormat}
                          convertToLocalTime={convertToLocalTime}
                          systemUses24h={systemUses24h}
                          isRestoring={msg.message_id ? restoringKeys.has(msg.message_id) : false}
                          isFirstInGroup={j === 0}
                          onScrollToMessage={handleScrollToMessage}
                          onOpenLightbox={handleOpenLightbox}
                        />
                        {selectionMode && canDelete && hasMsgId && (
                          <div className={`${styles.selectCheckbox} ${isSelected ? styles.selectCheckboxChecked : ""}`}>
                            {isSelected && (
                              <CheckIcon width={12} height={12} />
                            )}
                          </div>
                        )}
                      </div>
                    </React.Fragment>
                  );
                })}
              </div>
            </div>
          </React.Fragment>
        );
      })}
    </>
  );
}
