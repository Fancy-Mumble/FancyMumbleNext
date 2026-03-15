import type { FancyProfile } from "../../types";
import { SafeHtml } from "../../components/SafeHtml";
import {
  DECORATIONS,
  NAMEPLATES,
  EFFECTS,
  FONTS,
  CARD_BACKGROUNDS,
  AVATAR_BORDERS,
} from "./profileData";
import styles from "./SettingsPage.module.css";

interface ProfilePreviewCardProps {
  profile: FancyProfile;
  bio: string;
  avatar: string | null;
  displayName: string;
}

/** Resolve the card background CSS. */
function resolveCardBg(profile: FancyProfile): React.CSSProperties {
  const id = profile.cardBackground ?? "default";
  if (id === "custom" && profile.cardBackgroundCustom) {
    return { background: profile.cardBackgroundCustom };
  }
  const preset = CARD_BACKGROUNDS.find((b) => b.id === id);
  if (!preset) return {};
  return { background: preset.value, ...preset.extra };
}

/** Resolve the avatar border CSS. */
function resolveAvatarBorder(profile: FancyProfile): React.CSSProperties {
  const id = profile.avatarBorder ?? "default";
  if (id === "custom" && profile.avatarBorderCustom) {
    return { border: profile.avatarBorderCustom };
  }
  const preset = AVATAR_BORDERS.find((b) => b.id === id);
  if (!preset) return {};
  const out: React.CSSProperties = { border: preset.border };
  if (preset.shadow) out.boxShadow = preset.shadow;
  if (preset.outline) out.outline = preset.outline;
  if (id === "rainbow") {
    out.backgroundImage =
      "linear-gradient(var(--color-bg-secondary, #1a1a2e), var(--color-bg-secondary, #1a1a2e)), " +
      "conic-gradient(#ef4444, #f97316, #eab308, #22c55e, #3b82f6, #8b5cf6, #ef4444)";
    out.backgroundOrigin = "border-box";
    out.backgroundClip = "padding-box, border-box";
  }
  return out;
}

export function ProfilePreviewCard({
  profile,
  bio,
  avatar,
  displayName,
}: Readonly<ProfilePreviewCardProps>) {
  const nameStyle = profile.nameStyle ?? {};
  const decoration = DECORATIONS.find((d) => d.id === (profile.decoration ?? "none"));
  const nameplate = NAMEPLATES.find((n) => n.id === (profile.nameplate ?? "none"));
  const effect = EFFECTS.find((e) => e.id === (profile.effect ?? "none"));
  const fontCss =
    FONTS.find((f) => f.id === (nameStyle.font ?? "default"))?.css ?? "inherit";

  const bannerBg = profile.banner?.image
    ? `url(${profile.banner.image})`
    : profile.banner?.color ?? "#1a1a2e";

  const bannerStyle: React.CSSProperties = profile.banner?.image
    ? {
        backgroundImage: bannerBg,
        backgroundSize: "cover",
        backgroundPosition: "center",
      }
    : { background: bannerBg };

  const cardBgStyle = resolveCardBg(profile);
  const avatarBorderStyle = resolveAvatarBorder(profile);

  // Determine effect overlay class name
  const effectClass =
    effect && effect.id !== "none" && effect.animation
      ? styles[effect.animation] ?? ""
      : "";

  return (
    <div className={styles.previewCard} style={cardBgStyle}>
      {/* Effect overlay (animated layer) */}
      {effect && effect.id !== "none" && (
        <div
          className={`${styles.previewEffectOverlay} ${effectClass}`}
          style={effect.css}
        />
      )}

      {/* Banner */}
      <div className={styles.previewBanner} style={bannerStyle} />

      {/* Avatar area - flex row: avatar on left, name on right centred to avatar midpoint */}
      <div className={styles.previewAvatarArea}>
        <div className={styles.previewAvatarWrapper} style={avatarBorderStyle}>
          {avatar ? (
            <img
              src={avatar}
              alt="Avatar"
              className={styles.previewAvatarImg}
            />
          ) : (
            <span className={styles.previewAvatarPlaceholder}>👤</span>
          )}
          {decoration && decoration.id !== "none" && (
            <span className={styles.previewDecoration}>
              {decoration.preview}
            </span>
          )}
        </div>

        {/* Name sits to the right of the avatar, vertically centred */}
        <div className={styles.previewNameInline}>
          <div className={styles.previewNameRow}>
            {nameplate && nameplate.id !== "none" && (
              <span
                className={styles.previewNameplate}
                style={{ background: nameplate.bg }}
              />
            )}
            <span
              className={styles.previewName}
              style={{
                fontFamily: fontCss,
                color: nameStyle.gradient
                  ? "transparent"
                  : nameStyle.color || "var(--color-text-primary)",
                fontWeight: nameStyle.bold ? "bold" : 600,
                fontStyle: nameStyle.italic ? "italic" : "normal",
                textShadow: nameStyle.glow
                  ? `0 0 ${nameStyle.glow.size}px ${nameStyle.glow.color}`
                  : "none",
                background: nameStyle.gradient
                  ? `linear-gradient(135deg,${nameStyle.gradient[0]},${nameStyle.gradient[1]})`
                  : "transparent",
                WebkitBackgroundClip: nameStyle.gradient ? "text" : undefined,
                WebkitTextFillColor: nameStyle.gradient
                  ? "transparent"
                  : undefined,
              }}
            >
              {displayName || "Your Name"}
            </span>
          </div>
        </div>
      </div>

      {/* Body */}
      <div className={styles.previewBody}>
        {/* Custom status */}
        {profile.status && (
          <p className={styles.previewStatus}>{profile.status}</p>
        )}

        {/* Bio (sanitised to prevent XSS from untrusted comments) */}
        {bio && (
          <SafeHtml html={bio} className={styles.previewBio} />
        )}
      </div>
    </div>
  );
}
