import { MoonIcon, ShieldCheckIcon } from "../../icons";
import type { FancyProfile } from "../../types";
import { SafeHtml } from "../../components/elements/SafeHtml";
import { formatDuration } from "../../utils/format";
import { resolveThemePalette } from "../../utils/colorUtils";
import {
  DECORATIONS,
  NAMEPLATES,
  EFFECTS,
  FONTS,
  AVATAR_BORDERS,
} from "./profileData";
import styles from "./SettingsPage.module.css";

interface ProfilePreviewCardProps {
  profile: FancyProfile;
  bio: string;
  avatar: string | null;
  displayName: string;
  /** Seconds the user has been connected (from UserStats). */
  onlinesecs?: number | null;
  /** Seconds the user has been idle (from UserStats). */
  idlesecs?: number | null;
  /** Whether the user is registered on the server. */
  isRegistered?: boolean;
  /** ACL role chips to display at the bottom of the card. */
  groups?: readonly { name: string; color: string | null }[];
}

interface CardBgResult {
  style: React.CSSProperties;
  textColor?: string;
  accentColor?: string;
}

function resolveCardBg(profile: FancyProfile): CardBgResult {
  if (profile.cardBackground === "custom" && profile.cardBackgroundCustom) {
    return { style: { background: profile.cardBackgroundCustom } };
  }

  const colors = profile.themeColors ?? [];
  const glass = profile.cardGlass ?? false;
  const hasColors = colors.length > 0;

  if (hasColors) {
    const palette = resolveThemePalette(colors, glass);
    const style: React.CSSProperties = {
      background: palette.gradient,
      borderColor: palette.borderColor,
    };
    if (glass) style.backdropFilter = "blur(16px) saturate(1.4)";
    return { style, textColor: palette.textColor, accentColor: palette.accentColor };
  }

  if (glass) {
    return {
      style: {
        background: "rgba(255, 255, 255, 0.08)",
        backdropFilter: "blur(16px) saturate(1.4)",
      },
    };
  }

  return { style: { background: "var(--color-glass)" } };
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
  onlinesecs,
  idlesecs,
  isRegistered,
  groups,
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

  const cardBg = resolveCardBg(profile);
  const cardBgStyle = cardBg.style;
  const themeTextColor = cardBg.textColor;
  const accentColor = cardBg.accentColor;
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
                  : nameStyle.color || themeTextColor || "var(--color-text-primary)",
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
            {isRegistered && (
              <span className={styles.previewRegisteredBadge} title="Registered">
                <ShieldCheckIcon width={12} height={12} strokeWidth={2.5} />
              </span>
            )}
          </div>

          {/* Activity pills (compact, directly under the name) */}
          {(onlinesecs != null || (idlesecs != null && idlesecs > 0)) && (
            <div className={styles.previewActivityBar}>
              {onlinesecs != null && (
                <span className={`${styles.previewActivityPill} ${styles.previewActivityOnline}`}>
                  <span className={styles.previewActivityDot} />
                  {formatDuration(onlinesecs)}
                </span>
              )}
              {idlesecs != null && idlesecs > 0 && (
                <span className={`${styles.previewActivityPill} ${styles.previewActivityIdle}`}>
                  <MoonIcon className={styles.previewActivityIcon} width={11} height={11} strokeWidth={2.5} />
                  {formatDuration(idlesecs)}
                </span>
              )}
            </div>
          )}
        </div>
      </div>

      {/* Body */}
      <div
        className={styles.previewBody}
        style={themeTextColor ? { color: themeTextColor } : undefined}
      >
        {/* Custom status */}
        {profile.status && (
          <p
            className={styles.previewStatus}
            style={{ color: accentColor ?? (themeTextColor ? "inherit" : undefined) }}
          >
            {profile.status}
          </p>
        )}

        {/* Bio (sanitised to prevent XSS from untrusted comments) */}
        {bio && (
          <SafeHtml
            html={bio}
            className={styles.previewBio}
            style={themeTextColor ? { color: "inherit" } : undefined}
          />
        )}

        {/* Role chips */}
        {groups && groups.length > 0 && (
          <div className={styles.previewGroupChips}>
            {groups.map((g) => (
              <span
                key={g.name}
                className={styles.previewGroupChip}
                style={g.color ? {
                  color: g.color,
                  borderColor: `${g.color}55`,
                  background: `${g.color}18`,
                } : undefined}
              >
                {g.name}
              </span>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
