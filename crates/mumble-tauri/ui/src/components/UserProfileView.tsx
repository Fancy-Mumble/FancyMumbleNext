/**
 * Full-height right-side panel showing a user's full profile.
 *
 * Opened by clicking a username in the channel sidebar.
 * Renders the same ProfilePreviewCard used elsewhere, plus an
 * expanded bio section that isn't line-clamped.
 */

import { useEffect, useMemo, useState } from "react";
import { useAppStore } from "../store";
import { SafeHtml } from "./SafeHtml";
import type { UserEntry, FancyProfile, UserMode } from "../types";
import { textureToDataUrl, parseComment } from "../profileFormat";
import { getPreferences } from "../preferencesStorage";
import { useUserStats } from "../hooks/useUserStats";
import { formatDuration } from "../utils/format";
import UserInfoPanel from "./UserInfoPanel";
import {
  DECORATIONS,
  NAMEPLATES,
  EFFECTS,
  FONTS,
  CARD_BACKGROUNDS,
  AVATAR_BORDERS,
} from "../pages/settings/profileData";
import styles from "./UserProfileView.module.css";

// --- Helpers ------------------------------------------------------

function resolveCardBg(profile: FancyProfile): React.CSSProperties {
  const id = profile.cardBackground ?? "default";
  if (id === "custom" && profile.cardBackgroundCustom) {
    return { background: profile.cardBackgroundCustom };
  }
  const preset = CARD_BACKGROUNDS.find((b) => b.id === id);
  if (!preset) return {};
  return { background: preset.value, ...preset.extra };
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

  // In DM mode, always show the DM partner's profile even if the user
  // closed the generic profile panel.
  const effectiveSession = selectedUser ?? selectedDmUser;
  const isDmProfile = selectedUser === null && selectedDmUser !== null;

  const user: UserEntry | undefined = useMemo(
    () => users.find((u) => u.session === effectiveSession),
    [users, effectiveSession],
  );

  if (!user) return null;

  return (
    <UserProfilePanel
      user={user}
      onClose={isDmProfile ? undefined : () => selectUser(null)}
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

  const avatarDataUrl = useMemo(
    () =>
      user.texture && user.texture.length > 0
        ? textureToDataUrl(user.texture)
        : null,
    [user.texture],
  );

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

  const cardBgStyle = resolveCardBg(profile);
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
          <svg
            width="18"
            height="18"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <line x1="18" y1="6" x2="6" y2="18" />
            <line x1="6" y1="6" x2="18" y2="18" />
          </svg>
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
                {user.name}
              </span>
              {user.user_id != null && user.user_id > 0 && (
                <span className={styles.registeredBadge} title="Registered">
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
                    <polyline points="9 12 11 14 15 10" />
                  </svg>
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
                    <svg className={styles.activityIcon} width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                      <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
                    </svg>
                    {formatDuration(stats.idlesecs)}
                  </span>
                )}
              </div>
            )}
          </div>
        </div>

        {/* Body */}
        <div className={styles.body}>
          {/* Status */}
          {profile.status && (
            <p className={styles.status}>{profile.status}</p>
          )}
        </div>
      </div>

      {/* -- Expanded sections below the card ----------------- */}

      {bio && (
        <section className={styles.section}>
          <h3 className={styles.sectionTitle}>About Me</h3>
          <SafeHtml html={bio} className={styles.bioContent} />
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
