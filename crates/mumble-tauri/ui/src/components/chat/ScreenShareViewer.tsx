/**
 * Screen share viewer components.
 *
 * - OwnBroadcastPreview: shows local MediaStream directly (zero encoding)
 * - RemoteViewer: displays the WebRTC remote stream from another user
 * - StreamControls: overlay controls (play/pause, volume, fullscreen)
 * - ScreenShareViewer: container panel (video only, header is in ChatHeader)
 * - BroadcastBanner: notification bar shown when someone else is sharing
 */
import { useRef, useEffect, useMemo, useState, useCallback } from "react";
import { useRemoteStream } from "./useScreenShare";
import ScreenShareIcon from "../../assets/icons/communication/screen-share.svg?react";
import CloseIcon from "../../assets/icons/action/close.svg?react";
import PlayIcon from "../../assets/icons/status/play.svg?react";
import PauseIcon from "../../assets/icons/status/pause.svg?react";
import VolumeIcon from "../../assets/icons/audio/volume.svg?react";
import VolumeOffIcon from "../../assets/icons/audio/volume-off.svg?react";
import FullscreenIcon from "../../assets/icons/action/fullscreen.svg?react";
import FullscreenExitIcon from "../../assets/icons/action/fullscreen-exit.svg?react";
import styles from "./ScreenShareViewer.module.css";

// ---------------------------------------------------------------------------
// Stream controls overlay
// ---------------------------------------------------------------------------

interface StreamControlsProps {
  readonly videoRef: React.RefObject<HTMLVideoElement | null>;
  readonly containerRef: React.RefObject<HTMLDivElement | null>;
  /** Whether this is the own preview (volume/pause disabled). */
  readonly isOwnPreview?: boolean;
}

function StreamControls({ videoRef, containerRef, isOwnPreview }: StreamControlsProps) {
  const [paused, setPaused] = useState(false);
  const [muted, setMuted] = useState(false);
  const [volume, setVolume] = useState(100);
  const [isFullscreen, setIsFullscreen] = useState(false);
  const [showVolSlider, setShowVolSlider] = useState(false);

  // Sync fullscreen state.
  useEffect(() => {
    const onChange = () => {
      setIsFullscreen(document.fullscreenElement === containerRef.current);
    };
    document.addEventListener("fullscreenchange", onChange);
    return () => document.removeEventListener("fullscreenchange", onChange);
  }, [containerRef]);

  const togglePause = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    if (video.paused) {
      video.play().catch(() => {});
      setPaused(false);
    } else {
      video.pause();
      setPaused(true);
    }
  }, [videoRef]);

  const toggleMute = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    video.muted = !video.muted;
    setMuted(video.muted);
  }, [videoRef]);

  const handleVolumeChange = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const video = videoRef.current;
    if (!video) return;
    const val = Number(e.target.value);
    setVolume(val);
    video.volume = val / 100;
    if (val === 0) {
      video.muted = true;
      setMuted(true);
    } else if (video.muted) {
      video.muted = false;
      setMuted(false);
    }
  }, [videoRef]);

  const toggleFullscreen = useCallback(() => {
    const container = containerRef.current;
    if (!container) return;
    if (document.fullscreenElement === container) {
      document.exitFullscreen().catch(() => {});
    } else {
      container.requestFullscreen().catch(() => {});
    }
  }, [containerRef]);

  return (
    <div className={styles.streamControls}>
      {/* Play / Pause (only for remote streams) */}
      {!isOwnPreview && (
        <button
          type="button"
          className={styles.controlBtn}
          onClick={togglePause}
          title={paused ? "Play" : "Pause"}
          aria-label={paused ? "Play" : "Pause"}
        >
          {paused
            ? <PlayIcon width={16} height={16} />
            : <PauseIcon width={16} height={16} />}
        </button>
      )}

      {/* Volume (only for remote streams) */}
      {!isOwnPreview && (
        <div
          className={styles.volumeGroup}
          onMouseEnter={() => setShowVolSlider(true)}
          onMouseLeave={() => setShowVolSlider(false)}
        >
          <button
            type="button"
            className={styles.controlBtn}
            onClick={toggleMute}
            title={muted ? "Unmute" : "Mute"}
            aria-label={muted ? "Unmute" : "Mute"}
          >
            {muted || volume === 0
              ? <VolumeOffIcon width={16} height={16} />
              : <VolumeIcon width={16} height={16} />}
          </button>
          {showVolSlider && (
            <input
              type="range"
              min={0}
              max={100}
              value={muted ? 0 : volume}
              onChange={handleVolumeChange}
              className={styles.volumeSlider}
              aria-label="Volume"
            />
          )}
        </div>
      )}

      {/* Spacer */}
      <div className={styles.controlsSpacer} />

      {/* Fullscreen */}
      <button
        type="button"
        className={styles.controlBtn}
        onClick={toggleFullscreen}
        title={isFullscreen ? "Exit fullscreen" : "Fullscreen"}
        aria-label={isFullscreen ? "Exit fullscreen" : "Fullscreen"}
      >
        {isFullscreen
          ? <FullscreenExitIcon width={16} height={16} />
          : <FullscreenIcon width={16} height={16} />}
      </button>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Own broadcast preview - just a <video> element with the local stream
// ---------------------------------------------------------------------------

interface OwnPreviewProps {
  readonly stream: MediaStream;
}

function OwnBroadcastPreview({ stream }: OwnPreviewProps) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    video.srcObject = stream;
    return () => { video.srcObject = null; };
  }, [stream]);

  return (
    <div ref={containerRef} className={styles.streamViewport}>
      <video
        ref={videoRef}
        autoPlay
        playsInline
        muted
        className={styles.videoElement}
      />
      <StreamControls videoRef={videoRef} containerRef={containerRef} isOwnPreview />
    </div>
  );
}

// ---------------------------------------------------------------------------
// Remote viewer - displays the WebRTC stream from the broadcaster
// ---------------------------------------------------------------------------

function RemoteViewer() {
  const videoRef = useRef<HTMLVideoElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const remoteStream = useRemoteStream();

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    video.srcObject = remoteStream;
    if (remoteStream) {
      video.play().catch(() => {});
    }
    return () => { video.srcObject = null; };
  }, [remoteStream]);

  return (
    <div ref={containerRef} className={styles.streamViewport}>
      {!remoteStream && (
        <div className={styles.streamPlaceholder}>
          <ScreenShareIcon className={styles.streamPlaceholderIcon} />
          <div className={styles.streamPlaceholderText}>
            <strong>Connecting...</strong>
            Establishing peer connection
          </div>
        </div>
      )}
      <video
        ref={videoRef}
        autoPlay
        playsInline
        muted
        className={styles.videoElement}
        style={{ display: remoteStream ? "block" : "none" }}
      />
      {remoteStream && (
        <StreamControls videoRef={videoRef} containerRef={containerRef} />
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Main viewer panel
// ---------------------------------------------------------------------------

interface ScreenShareViewerProps {
  readonly isOwnBroadcast: boolean;
  readonly localStream: MediaStream | null;
}

export default function ScreenShareViewer({
  isOwnBroadcast,
  localStream,
}: ScreenShareViewerProps) {
  return (
    <div className={styles.broadcastArea}>
      {isOwnBroadcast && localStream
        ? <OwnBroadcastPreview stream={localStream} />
        : <RemoteViewer />}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Small UI elements for reuse in ChatHeader
// ---------------------------------------------------------------------------

/** "Share Screen" toggle button for the chat header. */
export function ShareScreenButton({
  active,
  onClick,
}: Readonly<{ active: boolean; onClick: () => void }>) {
  return (
    <button
      type="button"
      className={`${styles.shareScreenBtn} ${active ? styles.shareScreenBtnActive : ""}`}
      onClick={onClick}
      title={active ? "Stop sharing" : "Share screen"}
      aria-label={active ? "Stop sharing" : "Share screen"}
    >
      <ScreenShareIcon className={styles.shareScreenBtnIcon} />
    </button>
  );
}

// ---------------------------------------------------------------------------
// Broadcast notification banner
// ---------------------------------------------------------------------------

interface BroadcastBannerProps {
  /** Session IDs currently broadcasting (filtered to current channel). */
  readonly broadcasters: { session: number; name: string }[];
  /** Called when user clicks "Watch". */
  readonly onWatch: (session: number) => void;
}

/**
 * Notification bar shown above the chat when another user is sharing their
 * screen. Provides a "Watch" button to join the broadcast.
 */
export function BroadcastBanner({ broadcasters, onWatch }: BroadcastBannerProps) {
  const [dismissed, setDismissed] = useState<Set<number>>(new Set());

  // Reset dismissed state when broadcasters change (new broadcaster should show).
  const broadcasterIds = useMemo(
    () => broadcasters.map((b) => b.session).sort().join(","),
    [broadcasters],
  );
  useEffect(() => {
    setDismissed((prev) => {
      const active = new Set(broadcasters.map((b) => b.session));
      const next = new Set([...prev].filter((id) => active.has(id)));
      return next.size === prev.size ? prev : next;
    });
  }, [broadcasterIds]);

  const handleDismiss = useCallback((session: number) => {
    setDismissed((prev) => new Set([...prev, session]));
  }, []);

  const visible = broadcasters.filter((b) => !dismissed.has(b.session));
  if (visible.length === 0) return null;

  return (
    <>
      {visible.map((b) => (
        <div key={b.session} className={styles.broadcastBanner} role="status">
          <span className={styles.broadcastBannerDot} />
          <span className={styles.broadcastBannerText}>
            <span className={styles.broadcastBannerName}>{b.name}</span> is sharing their screen
          </span>
          <button
            type="button"
            className={styles.broadcastBannerWatchBtn}
            onClick={() => onWatch(b.session)}
          >
            <ScreenShareIcon width={12} height={12} />
            Watch
          </button>
          <button
            type="button"
            className={styles.broadcastBannerDismiss}
            onClick={() => handleDismiss(b.session)}
            title="Dismiss"
            aria-label="Dismiss notification"
          >
            <CloseIcon width={14} height={14} />
          </button>
        </div>
      ))}
    </>
  );
}
