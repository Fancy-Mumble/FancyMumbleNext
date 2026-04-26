import { useState, useCallback } from "react";
import { useAppStore } from "../../store";
import { PERM_MANAGE_EMOTES } from "../../utils/permissions";
import styles from "./AdminPanel.module.css";

import { inferMimeType } from "../../utils/media";

const ALLOWED_MIME = ["image/png", "image/jpeg", "image/gif", "image/webp", "image/svg+xml"];

export function CustomEmotesTab() {
  const emotes = useAppStore((s) => s.customServerEmotes);
  const customEmotesSupported = useAppStore((s) => s.fileServerCapabilities?.features.custom_emotes ?? false);
  const rootChannelPerms = useAppStore((s) => s.channels.find((c) => c.id === 0)?.permissions ?? 0);
  const canManage = customEmotesSupported && (rootChannelPerms & PERM_MANAGE_EMOTES) !== 0;
  const addCustomEmote = useAppStore((s) => s.addCustomEmote);
  const removeCustomEmote = useAppStore((s) => s.removeCustomEmote);

  const [shortcode, setShortcode] = useState("");
  const [aliasEmoji, setAliasEmoji] = useState("");
  const [description, setDescription] = useState("");
  const [filePath, setFilePath] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [statusMsg, setStatusMsg] = useState<{ kind: "ok" | "err"; text: string } | null>(null);

  const handlePickFile = useCallback(async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({
        multiple: false,
        directory: false,
        filters: [{
          name: "Emote image",
          extensions: ["png", "jpg", "jpeg", "gif", "webp", "svg"],
        }],
      });
      if (typeof picked === "string") {
        setFilePath(picked);
      }
    } catch (e) {
      console.error("emote file picker failed:", e);
      setStatusMsg({ kind: "err", text: "Could not open file picker." });
    }
  }, []);

  const handleSubmit = useCallback(async (e: React.FormEvent) => {
    e.preventDefault();
    if (!filePath) {
      setStatusMsg({ kind: "err", text: "Please select an emote image." });
      return;
    }
    const mime = inferMimeType(filePath);
    if (!mime || !ALLOWED_MIME.includes(mime)) {
      setStatusMsg({ kind: "err", text: "Unsupported image type." });
      return;
    }
    if (!shortcode.trim() || !aliasEmoji.trim()) {
      setStatusMsg({ kind: "err", text: "Shortcode and alias emoji are required." });
      return;
    }
    setSubmitting(true);
    setStatusMsg(null);
    try {
      await addCustomEmote({
        shortcode: shortcode.trim(),
        aliasEmoji: aliasEmoji.trim(),
        description: description.trim() || undefined,
        filePath,
        mimeType: mime,
      });
      setShortcode("");
      setAliasEmoji("");
      setDescription("");
      setFilePath(null);
      setStatusMsg({ kind: "ok", text: "Emote added." });
    } catch (err) {
      const detail = err instanceof Error ? err.message : String(err);
      setStatusMsg({ kind: "err", text: `Add emote failed: ${detail}` });
    } finally {
      setSubmitting(false);
    }
  }, [filePath, shortcode, aliasEmoji, description, addCustomEmote]);

  const handleDelete = useCallback(async (sc: string) => {
    if (!confirm(`Delete emote :${sc}: ?`)) return;
    try {
      await removeCustomEmote(sc);
      setStatusMsg({ kind: "ok", text: `Removed :${sc}:` });
    } catch (err) {
      const detail = err instanceof Error ? err.message : String(err);
      setStatusMsg({ kind: "err", text: `Remove failed: ${detail}` });
    }
  }, [removeCustomEmote]);

  if (!canManage) {
    return (
      <div className={styles.content}>
        <p>You do not have permission to manage custom server emotes.</p>
      </div>
    );
  }

  return (
    <div className={styles.content}>
      <h3 className={styles.aclSectionTitle}>Add custom emote</h3>
      <form onSubmit={handleSubmit} className={styles.emoteForm}>
        <label className={styles.fieldLabel}>
          Shortcode
          <input
            type="text"
            className={styles.input}
            value={shortcode}
            onChange={(e) => setShortcode(e.target.value)}
            placeholder="myCustom"
            maxLength={64}
            pattern="[A-Za-z0-9_\-]+"
            required
          />
        </label>
        <label className={styles.fieldLabel}>
          Alias emoji
          <input
            type="text"
            className={styles.input}
            value={aliasEmoji}
            onChange={(e) => setAliasEmoji(e.target.value)}
            placeholder="&#x1F923;"
            maxLength={32}
            required
          />
        </label>
        <label className={styles.fieldLabel}>
          Description
          <input
            type="text"
            className={styles.input}
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder="(optional)"
            maxLength={256}
          />
        </label>
        <div className={styles.fieldLabel}>
          Image
          <div className={styles.emoteFileRow}>
            <button type="button" className={styles.addBtn} onClick={handlePickFile}>
              {filePath ? "Change file" : "Choose file"}
            </button>
            {filePath && <span className={styles.emoteFilePath}>{filePath}</span>}
          </div>
        </div>
        <div>
          <button type="submit" className={styles.saveBtn} disabled={submitting}>
            {submitting ? "Uploading..." : "Add emote"}
          </button>
        </div>
        {statusMsg && (
          <p className={statusMsg.kind === "err" ? styles.errorText : undefined}>
            {statusMsg.text}
          </p>
        )}
      </form>

      <h3 className={styles.aclSectionTitle}>Existing emotes ({emotes.length})</h3>
      {emotes.length === 0 ? (
        <p>No custom emotes yet.</p>
      ) : (
        <ul className={styles.emoteList}>
          {emotes.map((e) => (
            <li key={e.shortcode} className={styles.emoteItem}>
              <img
                src={e.imageDataUrl}
                alt={e.shortcode}
                className={styles.emoteThumb}
              />
              <div className={styles.emoteMeta}>
                <code>:{e.shortcode}:</code>
                <span>{e.aliasEmoji}</span>
                {e.description && <em>{e.description}</em>}
              </div>
              <button
                type="button"
                onClick={() => handleDelete(e.shortcode)}
                className={`${styles.removeBtn} ${styles.emoteDelete}`}
              >
                Delete
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
