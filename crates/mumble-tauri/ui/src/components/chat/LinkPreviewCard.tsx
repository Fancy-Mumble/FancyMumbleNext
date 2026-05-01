import { memo, useState, useCallback } from "react";
import type { EmbedMedia, LinkEmbed } from "../../types";
import styles from "./LinkPreviewCard.module.css";

/**
 * Resolve the image source for a media field, prioritising the
 * server-provided base64 preview (which never causes a network
 * request to the origin) and only falling back to the remote URL
 * when the user has explicitly opted-in to external resources.
 *
 * Returning `undefined` signals that no privacy-preserving source is
 * available; callers should render a placeholder instead of leaking
 * the user's IP to the origin server.
 */
function previewSrc(
  media: EmbedMedia | undefined,
  allowExternal: boolean,
): string | undefined {
  if (!media) return undefined;
  if (media.preview?.data_url) return media.preview.data_url;
  return allowExternal ? media.url : undefined;
}

function isSpotifyEmbed(embed: LinkEmbed): boolean {
  const siteName = embed.site_name?.toLowerCase() ?? "";
  const providerName = embed.provider?.name?.toLowerCase() ?? "";
  if (siteName.includes("spotify") || providerName.includes("spotify")) {
    return true;
  }

  const videoUrl = embed.video?.url;
  if (!videoUrl) return false;
  try {
    return new URL(videoUrl).hostname.includes("spotify.com");
  } catch {
    return videoUrl.includes("spotify.com");
  }
}

function videoContainerStyle(embed: LinkEmbed): React.CSSProperties {
  const width = embed.video?.width;
  const height = embed.video?.height;
  const spotify = isSpotifyEmbed(embed);

  if (spotify) {
    const spotifyHeight = typeof height === "number" && height > 0
      ? Math.min(Math.max(height, 80), 420)
      : 152;
    return { height: `${spotifyHeight}px` };
  }

  if (typeof width === "number" && width > 0 && typeof height === "number" && height > 0) {
    return { aspectRatio: `${width} / ${height}` };
  }

  return {};
}

interface LinkPreviewCardProps {
  readonly embeds: LinkEmbed[];
  readonly allowExternalResources: boolean;
}

function EmbedCard({ embed, allowExternalResources }: Readonly<{ embed: LinkEmbed; allowExternalResources: boolean }>) {
  const [videoConsented, setVideoConsented] = useState(false);

  const handleConsent = useCallback(() => setVideoConsented(true), []);

  const colorHex = embed.color != null
    ? `#${embed.color.toString(16).padStart(6, "0")}`
    : undefined;

  const imageSrc = previewSrc(embed.image, allowExternalResources);
  const thumbSrc = previewSrc(embed.thumbnail, allowExternalResources);
  const hasLargeImage = embed.type === "image" && !!imageSrc;
  const hasVideo = embed.type === "video" && !!embed.video?.url;
  const showThumbnailOnSide = !!thumbSrc && !hasLargeImage && !hasVideo;

  return (
    <div
      className={styles.embed}
      style={colorHex ? { "--embed-color": colorHex } as React.CSSProperties : undefined}
    >
      <div className={styles.embedBody}>
        {embed.site_name && (
          <span className={styles.siteName}>{embed.site_name}</span>
        )}
        {embed.author?.name && (
          <span className={styles.author}>
            {embed.author.url ? (
              <a
                className={styles.authorLink}
                href={embed.author.url}
                target="_blank"
                rel="noopener noreferrer"
              >
                {embed.author.name}
              </a>
            ) : (
              embed.author.name
            )}
          </span>
        )}
        {embed.title && (
          <a
            className={styles.title}
            href={embed.url}
            target="_blank"
            rel="noopener noreferrer"
          >
            {embed.title}
          </a>
        )}
        {embed.description && (
          <p className={styles.description}>{embed.description}</p>
        )}
        {hasLargeImage && imageSrc && (
          <img
            className={styles.largeImage}
            src={imageSrc}
            alt={embed.title ?? ""}
            loading="lazy"
          />
        )}
        {hasVideo && (
          <VideoEmbed
            embed={embed}
            allowExternalResources={allowExternalResources}
            consented={videoConsented}
            onConsent={handleConsent}
          />
        )}
        {!hasVideo && !hasLargeImage && thumbSrc && !showThumbnailOnSide && (
          <img
            className={styles.largeImage}
            src={thumbSrc}
            alt={embed.title ?? ""}
            loading="lazy"
          />
        )}
      </div>
      {showThumbnailOnSide && thumbSrc && (
        <img
          className={styles.thumbnail}
          src={thumbSrc}
          alt=""
          loading="lazy"
        />
      )}
    </div>
  );
}

function VideoEmbed({
  embed,
  allowExternalResources,
  consented,
  onConsent,
}: Readonly<{
  embed: LinkEmbed;
  allowExternalResources: boolean;
  consented: boolean;
  onConsent: () => void;
}>) {
  if (!embed.video?.url) return null;

  const containerStyle = videoContainerStyle(embed);
  const spotify = isSpotifyEmbed(embed);

  if (!allowExternalResources && !consented) {
    return (
      <div className={styles.privacyGate}>
        <span className={styles.privacyText}>
          This embed loads external content from <strong>{embed.site_name ?? new URL(embed.video.url).hostname}</strong>.
        </span>
        <button type="button" className={styles.privacyButton} onClick={onConsent}>
          Load content
        </button>
      </div>
    );
  }

  if (!consented) {
    const posterSrc = previewSrc(embed.thumbnail, allowExternalResources);
    return (
      <div className={`${styles.videoContainer} ${!spotify ? styles.videoContainerDefault : ""}`} style={containerStyle}>
        {posterSrc && (
          <img
            src={posterSrc}
            alt={embed.title ?? ""}
            style={{ width: "100%", height: "100%", objectFit: "cover" }}
          />
        )}
        <button type="button" className={styles.videoOverlay} onClick={onConsent} aria-label="Play video">
          <span className={styles.playButton}>
            <span className={styles.playTriangle} />
          </span>
        </button>
      </div>
    );
  }

  return (
    <div className={`${styles.videoContainer} ${!spotify ? styles.videoContainerDefault : ""}`} style={containerStyle}>
      <iframe
        className={styles.videoIframe}
        src={embed.video.url}
        title={embed.title ?? "Embedded video"}
        allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture"
        allowFullScreen
        loading="lazy"
        sandbox="allow-scripts allow-same-origin allow-popups"
        style={{ background: "transparent" }}
      />
    </div>
  );
}

export default memo(function LinkPreviewCard({ embeds, allowExternalResources }: LinkPreviewCardProps) {
  if (embeds.length === 0) return null;

  return (
    <div className={styles.embedContainer}>
      {embeds.map((embed) => (
        <EmbedCard
          key={embed.url}
          embed={embed}
          allowExternalResources={allowExternalResources}
        />
      ))}
    </div>
  );
});
