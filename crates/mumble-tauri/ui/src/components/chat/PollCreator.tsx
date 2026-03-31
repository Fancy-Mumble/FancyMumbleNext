/**
 * PollCreator - modal dialog for creating a poll.
 *
 * Fields:
 *   - Question (free text)
 *   - Answers (dynamic list, 2-10 options)
 *   - Multiple choice toggle (checkbox vs radio)
 *
 * Produces a JSON payload to send via Mumble plugin messages.
 */

import { useState, useCallback } from "react";
import styles from "./PollCreator.module.css";

// --- Poll data format ---------------------------------------------

/** Poll message payload sent over PluginDataTransmission. */
export interface PollPayload {
  type: "poll";
  /** Unique poll id. */
  id: string;
  question: string;
  options: string[];
  /** true = multiple selection allowed; false = single choice. */
  multiple: boolean;
  /** Session of the poll creator. */
  creator: number;
  /** Creator's display name. */
  creatorName: string;
  /** Timestamp ISO string. */
  createdAt: string;
  /** Channel ID where the poll was created. */
  channelId: number;
}

/** A vote cast on a poll. */
export interface PollVotePayload {
  type: "poll_vote";
  /** ID of the poll being voted on. */
  pollId: string;
  /** Indices of selected options. */
  selected: number[];
  /** Session of the voter. */
  voter: number;
  voterName: string;
}

// --- Component ----------------------------------------------------

interface PollCreatorProps {
  /** Called when user submits the poll. */
  onSubmit: (question: string, options: string[], multiple: boolean) => void;
  /** Close modal. */
  onClose: () => void;
}

export default function PollCreator({ onSubmit, onClose }: Readonly<PollCreatorProps>) {
  const [question, setQuestion] = useState("");
  const [options, setOptions] = useState(["", ""]);
  const [multiple, setMultiple] = useState(false);

  const addOption = useCallback(() => {
    if (options.length < 10) setOptions((o) => [...o, ""]);
  }, [options.length]);

  const removeOption = useCallback(
    (idx: number) => {
      if (options.length > 2) {
        setOptions((o) => o.filter((_, i) => i !== idx));
      }
    },
    [options.length],
  );

  const updateOption = useCallback((idx: number, value: string) => {
    setOptions((o) => o.map((v, i) => (i === idx ? value : v)));
  }, []);

  const canSubmit =
    question.trim() && options.filter((o) => o.trim()).length >= 2;

  const handleSubmit = useCallback(() => {
    if (!canSubmit) return;
    const trimmed = options.map((o) => o.trim()).filter(Boolean);
    onSubmit(question.trim(), trimmed, multiple);
    onClose();
  }, [canSubmit, question, options, multiple, onSubmit, onClose]);

  return (
    <div
      className={styles.backdrop}
      onClick={onClose}
      onKeyDown={(e) => { if (e.key === "Escape") onClose(); }}
      role="presentation"
    >
      <div
        className={styles.modal}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
      >
        <div className={styles.header}>
          <h3 className={styles.title}>Create a Poll</h3>
          <button className={styles.closeBtn} onClick={onClose}>
            ✕
          </button>
        </div>

        <div className={styles.body}>
          {/* Question */}
          <label className={styles.label}>Question</label>
          <input
            className={styles.input}
            placeholder="What do you want to ask?"
            value={question}
            onChange={(e) => setQuestion(e.target.value)}
            autoFocus
            maxLength={300}
          />

          {/* Options */}
          <label className={styles.label}>Options</label>
          {options.map((opt, i) => (
            <div key={i} className={styles.optionRow}>
              <span className={styles.optionBullet}>
                {multiple ? "☐" : "○"}
              </span>
              <input
                className={styles.input}
                placeholder={`Option ${i + 1}`}
                value={opt}
                onChange={(e) => updateOption(i, e.target.value)}
                maxLength={200}
              />
              {options.length > 2 && (
                <button
                  className={styles.removeBtn}
                  onClick={() => removeOption(i)}
                  title="Remove option"
                >
                  ✕
                </button>
              )}
            </div>
          ))}
          {options.length < 10 && (
            <button className={styles.addBtn} onClick={addOption}>
              + Add option
            </button>
          )}

          {/* Multiple choice toggle */}
          <label className={styles.checkboxLabel}>
            <input
              type="checkbox"
              checked={multiple}
              onChange={(e) => setMultiple(e.target.checked)}
            />
            Allow multiple selections
          </label>
        </div>

        <div className={styles.footer}>
          <button className={styles.cancelBtn} onClick={onClose}>
            Cancel
          </button>
          <button
            className={styles.submitBtn}
            onClick={handleSubmit}
            disabled={!canSubmit}
          >
            Create Poll
          </button>
        </div>
      </div>
    </div>
  );
}
