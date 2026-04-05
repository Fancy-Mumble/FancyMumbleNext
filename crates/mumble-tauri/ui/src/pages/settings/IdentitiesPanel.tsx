import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save, open } from "@tauri-apps/plugin-dialog";
import { deleteProfileData } from "./profileData";
import styles from "./SettingsPage.module.css";

export function IdentitiesPanel({
  identities,
  connectedCertLabel,
  onRefresh,
  onEditProfile,
  isExpert,
}: Readonly<{
  identities: string[];
  connectedCertLabel: string | null;
  onRefresh: () => void;
  onEditProfile: (label: string) => void;
  isExpert: boolean;
}>) {
  const [newLabel, setNewLabel] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);

  const handleCreate = useCallback(async () => {
    const label = newLabel.trim();
    if (!label) return;
    setError(null);
    try {
      await invoke("generate_certificate", { label });
      setNewLabel("");
      onRefresh();
    } catch (e) {
      setError(String(e));
    }
  }, [newLabel, onRefresh]);

  const handleDelete = useCallback(
    async (label: string) => {
      setError(null);
      try {
        await invoke("delete_certificate", { label });
        await deleteProfileData(label);
        setConfirmDelete(null);
        onRefresh();
      } catch (e) {
        setError(String(e));
      }
    },
    [onRefresh],
  );

  const handleExport = useCallback(async (label: string) => {
    setError(null);
    try {
      const destPath = await save({
        defaultPath: `${label}.fmid`,
        filters: [{ name: "Fancy Mumble Identity", extensions: ["fmid"] }],
      });
      if (!destPath) return;
      await invoke("export_certificate", { label, destPath });
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const handleImport = useCallback(async () => {
    setError(null);
    try {
      const selected = await open({
        multiple: false,
        filters: [{ name: "Fancy Mumble Identity", extensions: ["fmid"] }],
      });
      if (!selected) return;
      await invoke("import_certificate", { srcPath: selected });
      onRefresh();
    } catch (e) {
      setError(String(e));
    }
  }, [onRefresh]);

  return (
    <>
      <h2 className={styles.panelTitle}>Identities</h2>

      <section className={styles.section}>
        <p className={styles.fieldHint}>
          Identities are TLS client certificates used to authenticate with
          Mumble servers. Each identity is unique to you and persists across
          sessions.
        </p>

        {error && <p className={styles.error}>{error}</p>}

        {identities.length === 0 ? (
          <p className={styles.fieldHint} style={{ fontStyle: "italic" }}>
            No identities yet. Create or import one below.
          </p>
        ) : (
          <ul className={styles.identityList}>
            {identities.map((label) => (
              <li
                key={label}
                className={`${styles.identityItem}${label === connectedCertLabel ? ` ${styles.identityItemActive}` : ""}`}
              >
                <span className={styles.identityLabel}>
                  {label}
                  {label === connectedCertLabel && (
                    <span className={styles.identityActiveBadge}>connected</span>
                  )}
                </span>

                <div className={styles.identityActions}>
                  {isExpert && (
                    <button
                      type="button"
                      className={styles.identityEditBtn}
                      onClick={() => onEditProfile(label)}
                    >
                      Edit Profile
                    </button>
                  )}
                  <button
                    type="button"
                    className={styles.ghostBtn}
                    onClick={() => handleExport(label)}
                  >
                    Export
                  </button>

                  {confirmDelete === label ? (
                    <div className={styles.confirmBtns}>
                      <button
                        type="button"
                        className={styles.dangerBtn}
                        onClick={() => handleDelete(label)}
                      >
                        Confirm
                      </button>
                      <button
                        type="button"
                        className={styles.ghostBtn}
                        onClick={() => setConfirmDelete(null)}
                      >
                        Cancel
                      </button>
                    </div>
                  ) : (
                    <button
                      type="button"
                      className={styles.dangerBtn}
                      onClick={() => setConfirmDelete(label)}
                    >
                      Delete
                    </button>
                  )}
                </div>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Create new identity</h3>
        <div className={styles.identityCreateRow}>
          <input
            type="text"
            className={styles.input}
            value={newLabel}
            onChange={(e) => setNewLabel(e.target.value)}
            placeholder="Identity name..."
            onKeyDown={(e) => {
              if (e.key === "Enter") handleCreate();
            }}
          />
          <button
            type="button"
            className={styles.ghostBtn}
            onClick={handleCreate}
            disabled={!newLabel.trim()}
          >
            Create
          </button>
        </div>
      </section>

      <section className={styles.section}>
        <button
          type="button"
          className={styles.ghostBtn}
          onClick={handleImport}
        >
          Import identity...
        </button>
      </section>
    </>
  );
}
