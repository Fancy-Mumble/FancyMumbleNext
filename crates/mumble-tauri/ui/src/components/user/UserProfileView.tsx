import { CloseIcon, MoonIcon, ShieldCheckIcon } from "../../icons";
/**
 * Full-height right-side panel showing a user's full profile.
 *
 * Opened by clicking a username in the channel sidebar.
 * Renders the same ProfilePreviewCard used elsewhere, plus an
 * expanded bio section that isn't line-clamped.
 */

import { useEffect, useMemo, useState } from "react";
import { useAppStore } from "../../store";
import { SafeHtml } from "../elements/SafeHtml";
import type { UserEntry, FancyProfile, UserMode } from "../../types";
import { parseComment } from "../../profileFormat";
import { useUserAvatar } from "../../lazyBlobs";
import { getPreferences } from "../../preferencesStorage";
import { useUserStats } from "../../hooks/useUserStats";
import { formatDuration } from "../../utils/format";
import UserInfoPanel from "./UserInfoPanel";
import {
  DECORATIONS,
  NAMEPLATES,
  EFFECTS,
  FONTS,
  AVATAR_BORDERS,
} from "../../pages/settings/profileData";
import { resolveThemePalette } from "../../utils/colorUtils";
import styles from "./UserProfileView.module.css";

// --- Helpers ------------------------------------------------------

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

// --- Component ----------------------------------------------------

export default function UserProfileView() {
  const selectedUser = useAppStore((s) => s.selectedUser);
  const selectedDmUser = useAppStore((s) => s.selectedDmUser);
  const users = useAppStore((s) => s.users);
  const selectUser = useAppStore((s) => s.selectUser);
  const selectDmUser = useAppStore((s) => s.selectDmUser);

  // In DM mode, always show the DM partner's profile even if the user
  // closed the generic profile panel.
  const effectiveSession = selectedUser ?? selectedDmUser;

  const user: UserEntry | undefined = useMemo(
    () => users.find((u) => u.session === effectiveSession),
    [users, effectiveSession],
  );

  if (!user) return null;

  const handleClose = selectedDmUser === null
    ? () => selectUser(null)
    : () => { void selectDmUser(selectedDmUser); };

  return (
    <UserProfilePanel
      user={user}
      onClose={handleClose}
    />
  );
}

function UserProfilePanel({
  user,
  onClose,
}: Readonly<{
  user: UserEntry;
  onClose?: () => void;
}>) {
  const [userMode, setUserMode] = useState<UserMode>("normal");

  const isExpert = userMode !== "normal";

  // Load user mode preference on mount.
  useEffect(() => {
    getPreferences().then((p) => setUserMode(p.userMode));
  }, []);

  // Always request user stats so the activity bar (online/idle) is
  // available regardless of user mode.
  const stats = useUserStats(user.session, true);

  const avatarDataUrl = useUserAvatar(user.session, user.texture_size);

  const parsed = useMemo(
    () => (user.comment ? parseComment(user.comment) : null),
    [user.comment],
  );

  const profile: FancyProfile = parsed?.profile ?? {};
  const bio = parsed?.bio ?? "";

  const nameStyle = profile.nameStyle ?? {};
  const decoration = DECORATIONS.find(
    (d) => d.id === (profile.decoration ?? "none"),
  );
  const nameplate = NAMEPLATES.find(
    (n) => n.id === (profile.nameplate ?? "none"),
  );
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

  const effectClass =
    effect && effect.id !== "none" && effect.animation
      ? styles[effect.animation] ?? ""
      : "";

  return (
    <aside className={styles.panel}>
      {/* Close button (hidden in DM mode where the panel is always shown) */}
      {onClose && (
        <button
          className={styles.closeBtn}
          onClick={onClose}
          aria-label="Close profile"
        >
          <CloseIcon width={18} height={18} />
        </button>
      )}

      {/* Card */}
      <div className={styles.card} style={cardBgStyle}>
        {/* Effect overlay */}
        {effect && effect.id !== "none" && (
          <div
            className={`${styles.effectOverlay} ${effectClass}`}
            style={effect.css}
          />
        )}

        {/* Banner */}
        <div className={styles.banner} style={bannerStyle} />

        {/* Avatar + name row - flex row so name centre aligns with avatar centre */}
        <div className={styles.avatarArea}>
          <div className={styles.avatarWrapper} style={avatarBorderStyle}>
            {avatarDataUrl ? (
              <img
                src={avatarDataUrl}
                alt={user.name}
                className={styles.avatarImg}
              />
            ) : (
              <span className={styles.avatarPlaceholder}>👤</span>
            )}
            {decoration && decoration.id !== "none" && (
              <span className={styles.decoration}>{decoration.preview}</span>
            )}
          </div>

          {/* Name sits to the right, text centre == avatar centre */}
          <div className={styles.nameInline}>
            <div className={styles.nameRow}>
              {nameplate && nameplate.id !== "none" && (
                <span
                  className={styles.nameplate}
                  style={{ background: nameplate.bg }}
                />
              )}
              <span
                className={styles.name}
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
                {user.name}
              </span>
              {user.user_id != null && user.user_id > 0 && (
                <span className={styles.registeredBadge} title="Registered">
                  <ShieldCheckIcon width={14} height={14} strokeWidth={2.5} />
                </span>
              )}
            </div>

            {/* Activity pills (compact, directly under the name) */}
            {stats && (stats.onlinesecs != null || (stats.idlesecs != null && stats.idlesecs > 0)) && (
              <div className={styles.activityBar}>
                {stats.onlinesecs != null && (
                  <span className={`${styles.activityPill} ${styles.activityOnline}`}>
                    <span className={styles.activityDot} />
                    {formatDuration(stats.onlinesecs)}
                  </span>
                )}
                {stats.idlesecs != null && stats.idlesecs > 0 && (
                  <span className={`${styles.activityPill} ${styles.activityIdle}`}>
                    <MoonIcon className={styles.activityIcon} width={12} height={12} strokeWidth={2.5} />
                    {formatDuration(stats.idlesecs)}
                  </span>
                )}
              </div>
            )}
          </div>
        </div>

        {/* Body */}
        <div
          className={styles.body}
          style={themeTextColor ? { color: themeTextColor } : undefined}
        >
          {/* Status */}
          {profile.status && (
            <p
              className={styles.status}
              style={{ color: accentColor ?? (themeTextColor ? "inherit" : undefined) }}
            >
              {profile.status}
            </p>
          )}
        </div>
      </div>

      {/* -- Expanded sections below the card ----------------- */}

      {bio && (
        <section className={styles.section}>
          <h3 className={styles.sectionTitle}>About Me</h3>
          <SafeHtml
            html={bio}
            className={styles.bioContent}
            style={themeTextColor ? { color: themeTextColor } : undefined}
          />
        </section>
      )}

      <section className={styles.section}>
        <h3 className={styles.sectionTitle}>Info</h3>
        <div className={styles.infoGrid}>
          <span className={styles.infoLabel}>Session</span>
          <span className={styles.infoValue}>{user.session}</span>
          <span className={styles.infoLabel}>Channel</span>
          <span className={styles.infoValue}>{user.channel_id}</span>
          <span className={styles.infoLabel}>Registered</span>
          <span className={styles.infoValue}>
            {user.user_id != null && user.user_id > 0 ? "Yes" : "No"}
          </span>
        </div>
      </section>

      {isExpert && stats && <UserInfoPanel stats={stats} />}
    </aside>
  );
}
