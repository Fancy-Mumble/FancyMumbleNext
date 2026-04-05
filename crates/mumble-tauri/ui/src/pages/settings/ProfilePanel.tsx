import { useRef, useState, useCallback } from "react";
import type { FancyProfile } from "../../types";
import { updatePreferences } from "../../preferencesStorage";
import {
  DECORATIONS,
  NAMEPLATES,
  EFFECTS,
  CARD_BACKGROUNDS,
  AVATAR_BORDERS,
} from "./profileData";
import { ImageEditor } from "./ImageEditor";
import { BioEditor } from "./BioEditor";
import { NameStyleSection } from "./NameStyleSection";
import styles from "./SettingsPage.module.css";

export function ProfilePanel({
  defaultUsername,
  setDefaultUsername,
  profile,
  onPatchProfile,
  bio,
  onBioChange,
  avatar,
  onAvatarChange,
  profileError,
  isExpert,
  activeIdentity,
  identities,
  connectedCertLabel,
  onSwitchIdentity,
  onGoToIdentities,
}: Readonly<{
  defaultUsername: string;
  setDefaultUsername: (v: string) => void;
  profile: FancyProfile;
  onPatchProfile: (patch: Partial<FancyProfile>) => void;
  bio: string;
  onBioChange: (v: string) => void;
  avatar: string | null;
  onAvatarChange: (v: string | null) => void;
  profileError: string | null;
  isExpert: boolean;
  activeIdentity: string | null;
  identities: string[];
  connectedCertLabel: string | null;
  onSwitchIdentity: (label: string | null) => void;
  onGoToIdentities: () => void;
}>) {
  const avatarInputRef = useRef<HTMLInputElement>(null);
  const bannerInputRef = useRef<HTMLInputElement>(null);

  // State for the image crop/zoom editor.
  const [editorImage, setEditorImage] = useState<{
    src: string;
    target: "avatar" | "banner";
  } | null>(null);

  const handleSaveUsername = useCallback(async () => {
    if (!defaultUsername.trim()) return;
    await updatePreferences({ defaultUsername: defaultUsername.trim() });
  }, [defaultUsername]);

  const handleAvatarFile = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () =>
      setEditorImage({ src: reader.result as string, target: "avatar" });
    reader.readAsDataURL(file);
    e.target.value = "";
  };

  const handleBannerFile = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () =>
      setEditorImage({ src: reader.result as string, target: "banner" });
    reader.readAsDataURL(file);
    e.target.value = "";
  };

  const handleEditorConfirm = (dataUrl: string) => {
    if (editorImage?.target === "avatar") {
      onAvatarChange(dataUrl);
    } else {
      onPatchProfile({
        banner: { ...profile.banner, image: dataUrl },
      });
    }
    setEditorImage(null);
  };

  const nameStyle = profile.nameStyle ?? {};
  const patchNameStyle = (patch: Partial<NonNullable<FancyProfile["nameStyle"]>>) =>
    onPatchProfile({ nameStyle: { ...nameStyle, ...patch } });

  return (
    <>
      <h2 className={styles.panelTitle}>Profile</h2>

      {/* -- Identity selector (advanced mode only) ------------- */}
      {isExpert && identities.length > 0 && (
        <section className={styles.identityBar}>
          <div className={styles.identityBarRow}>
            <label className={styles.identityBarLabel}>Identity</label>
            <select
              className={styles.select}
              value={activeIdentity ?? ""}
              onChange={(e) => onSwitchIdentity(e.target.value || null)}
            >
              {identities.map((label) => (
                <option key={label} value={label}>
                  {label}{label === connectedCertLabel ? " (connected)" : ""}
                </option>
              ))}
            </select>
            <button
              type="button"
              className={styles.ghostBtn}
              onClick={onGoToIdentities}
            >
              Manage identities
            </button>
          </div>
          {connectedCertLabel && activeIdentity !== connectedCertLabel && (
            <p className={styles.fieldHint}>
              Viewing profile for a different identity. Changes are saved locally
              but will not be applied to the server until you connect with this identity.
            </p>
          )}
        </section>
      )}

      {/* -- Default Username ----------------------------------- */}
      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Default Username</h3>
        <p className={styles.fieldHint}>
          Pre-filled when you add a new server.
        </p>
        <input
          className={styles.input}
          type="text"
          autoComplete="off"
          value={defaultUsername}
          onChange={(e) => setDefaultUsername(e.target.value)}
          onBlur={handleSaveUsername}
          onKeyDown={(e) => {
            if (e.key === "Enter") handleSaveUsername();
          }}
          placeholder="Your username"
        />
      </section>

      {/* -- Avatar --------------------------------------------- */}
      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Avatar</h3>
        <p className={styles.fieldHint}>
          Upload a PNG or JPEG image. Sent as the Mumble user texture.
        </p>
        <div className={styles.avatarRow}>
          <button
            type="button"
            className={styles.avatarPreview}
            onClick={() => avatarInputRef.current?.click()}
            aria-label="Choose avatar image"
          >
            {avatar ? (
              <img src={avatar} alt="Avatar" className={styles.avatarImg} />
            ) : (
              <span className={styles.avatarPlaceholder}>📷</span>
            )}
          </button>
          <div className={styles.avatarActions}>
            <button
              type="button"
              className={styles.ghostBtn}
              onClick={() => avatarInputRef.current?.click()}
            >
              Change
            </button>
            {avatar && (
              <button
                type="button"
                className={styles.ghostBtn}
                onClick={() => onAvatarChange(null)}
              >
                Remove
              </button>
            )}
          </div>
        </div>
        <input
          ref={avatarInputRef}
          type="file"
          accept="image/png,image/jpeg,image/webp"
          hidden
          onChange={handleAvatarFile}
        />
      </section>

      {/* -- Bio ------------------------------------------------ */}
      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Bio</h3>
        <p className={styles.fieldHint}>
          Visible to other Mumble clients as your user comment.
        </p>
        <BioEditor
          value={bio}
          onChange={onBioChange}
          maxLength={2000}
          placeholder="Tell others about yourself..."
        />
      </section>

      {/* -- Custom Status -------------------------------------- */}
      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Status</h3>
        <p className={styles.fieldHint}>
          A short status message shown below your name.
        </p>
        <input
          className={styles.input}
          type="text"
          autoComplete="off"
          maxLength={80}
          value={profile.status ?? ""}
          onChange={(e) =>
            onPatchProfile({
              status: e.target.value || undefined,
            })
          }
          placeholder="🎮 Playing a game..."
        />
      </section>

      {/* -- Card Background ------------------------------------ */}
      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Card Background</h3>
        <p className={styles.fieldHint}>
          Background style for your profile card - gradients, glass,
          transparency, or a solid colour.
        </p>
        <div className={styles.optionGrid}>
          {CARD_BACKGROUNDS
            .filter((bg) => bg.id !== "custom" || isExpert)
            .map((bg) => (
              <button
                key={bg.id}
                type="button"
                className={`${styles.cardBgCard} ${
                  (profile.cardBackground ?? "default") === bg.id
                    ? styles.optionCardSelected
                    : ""
                }`}
                style={{
                  background: bg.value || "var(--color-glass)",
                  ...bg.extra,
                }}
                onClick={() =>
                  onPatchProfile({
                    cardBackground: bg.id === "default" ? undefined : bg.id,
                  })
                }
              >
                <span className={styles.optionLabel}>{bg.label}</span>
              </button>
            ))}
        </div>
        {isExpert && profile.cardBackground === "custom" && (
          <div className={styles.field} style={{ marginTop: 8 }}>
            <label className={styles.fieldLabel}>Custom CSS background</label>
            <input
              className={styles.input}
              type="text"
              value={profile.cardBackgroundCustom ?? ""}
              onChange={(e) =>
                onPatchProfile({ cardBackgroundCustom: e.target.value || undefined })
              }
              placeholder="linear-gradient(135deg, #1a1a2e, #2d1b38)"
            />
          </div>
        )}
      </section>

      {/* -- Avatar Border -------------------------------------- */}
      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Avatar Border</h3>
        <p className={styles.fieldHint}>
          Border style around your profile picture.
        </p>
        <div className={styles.optionGrid}>
          {AVATAR_BORDERS
            .filter((ab) => ab.id !== "custom" || isExpert)
            .map((ab) => {
              const isRainbow = ab.id === "rainbow";
              const borderStyle: React.CSSProperties = {
                border: ab.border || "2px solid var(--color-glass-border)",
                boxShadow: ab.shadow,
                outline: ab.outline,
                ...(isRainbow
                  ? {
                      backgroundImage:
                        "linear-gradient(var(--color-bg-secondary, #1a1a2e), var(--color-bg-secondary, #1a1a2e)), " +
                        "conic-gradient(#ef4444, #f97316, #eab308, #22c55e, #3b82f6, #8b5cf6, #ef4444)",
                      backgroundOrigin: "border-box",
                      backgroundClip: "padding-box, border-box",
                    }
                  : {}),
              };
              return (
                <button
                  key={ab.id}
                  type="button"
                  className={`${styles.avatarBorderCard} ${
                    (profile.avatarBorder ?? "default") === ab.id
                      ? styles.optionCardSelected
                      : ""
                  }`}
                  onClick={() =>
                    onPatchProfile({
                      avatarBorder: ab.id === "default" ? undefined : ab.id,
                    })
                  }
                >
                  <span className={styles.borderPreview} style={borderStyle} />
                  <span className={styles.optionLabel}>{ab.label}</span>
                </button>
              );
            })}
        </div>
        {isExpert && profile.avatarBorder === "custom" && (
          <div className={styles.field} style={{ marginTop: 8 }}>
            <label className={styles.fieldLabel}>Custom CSS border</label>
            <input
              className={styles.input}
              type="text"
              value={profile.avatarBorderCustom ?? ""}
              onChange={(e) =>
                onPatchProfile({ avatarBorderCustom: e.target.value || undefined })
              }
              placeholder="3px solid #ff00ff"
            />
          </div>
        )}
      </section>

      {/* -- Profile Decoration --------------------------------- */}
      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Profile Decoration</h3>
        <p className={styles.fieldHint}>
          An overlay around your avatar frame.
        </p>
        <div className={styles.optionGrid}>
          {DECORATIONS.map((d) => (
            <button
              key={d.id}
              type="button"
              className={`${styles.optionCard} ${
                (profile.decoration ?? "none") === d.id
                  ? styles.optionCardSelected
                  : ""
              }`}
              onClick={() =>
                onPatchProfile({
                  decoration: d.id === "none" ? undefined : d.id,
                })
              }
            >
              <span className={styles.optionPreview}>{d.preview}</span>
              <span className={styles.optionLabel}>{d.label}</span>
            </button>
          ))}
        </div>
      </section>

      {/* -- Nameplate ------------------------------------------ */}
      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Nameplate</h3>
        <p className={styles.fieldHint}>
          Background style behind your display name.
        </p>
        <div className={styles.optionGrid}>
          {NAMEPLATES.map((n) => (
            <button
              key={n.id}
              type="button"
              className={`${styles.nameplateCard} ${
                (profile.nameplate ?? "none") === n.id
                  ? styles.optionCardSelected
                  : ""
              }`}
              style={{ background: n.bg }}
              onClick={() =>
                onPatchProfile({
                  nameplate: n.id === "none" ? undefined : n.id,
                })
              }
            >
              <span className={styles.optionLabel}>{n.label}</span>
            </button>
          ))}
        </div>
      </section>

      {/* -- Profile Effect ------------------------------------- */}
      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Profile Effect</h3>
        <p className={styles.fieldHint}>
          Animated effect shown on your profile card.
        </p>
        <div className={styles.optionGrid}>
          {EFFECTS.map((fx) => (
            <button
              key={fx.id}
              type="button"
              className={`${styles.optionCard} ${
                (profile.effect ?? "none") === fx.id
                  ? styles.optionCardSelected
                  : ""
              }`}
              onClick={() =>
                onPatchProfile({
                  effect: fx.id === "none" ? undefined : fx.id,
                })
              }
            >
              <span className={styles.optionPreview}>{fx.preview}</span>
              <span className={styles.optionLabel}>{fx.label}</span>
            </button>
          ))}
        </div>
      </section>

      {/* -- Name Style ----------------------------------------- */}
      <NameStyleSection
        nameStyle={nameStyle}
        onPatch={patchNameStyle}
        displayName={defaultUsername}
      />

      {/* -- Banner --------------------------------------------- */}
      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Banner</h3>
        <p className={styles.fieldHint}>
          Background colour and optional image for your profile card banner.
        </p>

        {/* Colour */}
        <div className={styles.field}>
          <div className={styles.fieldRow}>
            <label className={styles.fieldLabel}>Colour</label>
            <input
              type="color"
              className={styles.colorInput}
              value={profile.banner?.color || "#1a1a2e"}
              onChange={(e) =>
                onPatchProfile({
                  banner: { ...profile.banner, color: e.target.value },
                })
              }
            />
          </div>
        </div>

        {/* Image */}
        <div className={styles.field}>
          <label className={styles.fieldLabel}>Image</label>
          <div className={styles.avatarRow}>
            {profile.banner?.image && (
              <img
                src={profile.banner.image}
                alt="Banner"
                className={styles.bannerThumb}
              />
            )}
            <div className={styles.avatarActions}>
              <button
                type="button"
                className={styles.ghostBtn}
                onClick={() => bannerInputRef.current?.click()}
              >
                {profile.banner?.image ? "Change" : "Upload"}
              </button>
              {profile.banner?.image && (
                <button
                  type="button"
                  className={styles.ghostBtn}
                  onClick={() =>
                    onPatchProfile({
                      banner: { ...profile.banner, image: undefined },
                    })
                  }
                >
                  Remove
                </button>
              )}
            </div>
          </div>
          <input
            ref={bannerInputRef}
            type="file"
            accept="image/png,image/jpeg,image/webp"
            hidden
            onChange={handleBannerFile}
          />
        </div>
      </section>

      {/* Profile errors (e.g. too large) */}
      {profileError && (
        <section className={styles.section}>
          <p className={styles.error}>{profileError}</p>
        </section>
      )}

      {/* Image crop/zoom editor modal */}
      {editorImage && (
        <ImageEditor
          src={editorImage.src}
          cropShape={editorImage.target === "avatar" ? "circle" : "rect"}
          targetWidth={editorImage.target === "avatar" ? 128 : 400}
          targetHeight={editorImage.target === "avatar" ? 128 : 150}
          maxBytes={editorImage.target === "avatar" ? 100_000 : 80_000}
          onConfirm={handleEditorConfirm}
          onCancel={() => setEditorImage(null)}
        />
      )}
    </>
  );
}
