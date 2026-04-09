/**
 * On-demand screen share thumbnail preview.
 *
 * Creates a brief WebRTC viewer connection to capture a single video frame,
 * stores the result as a JPEG data URL in a module-level cache, and tears
 * the connection down.  Refreshes at most once per minute while hovering.
 *
 * Signal routing (SDP answers, ICE candidates) is handled by the main
 * signal dispatcher in useScreenShare.ts through the exported helpers.
 */
import { useState, useEffect, useRef } from "react";
import { useAppStore } from "../../store";

const SIGNAL_SDP_OFFER = 2;
const SIGNAL_ICE_CANDIDATE = 4;

const RTC_CONFIG: RTCConfiguration = {
  iceServers: [
    { urls: "stun:stun.l.google.com:19302" },
    { urls: "stun:stun1.l.google.com:19302" },
  ],
};

const THUMBNAIL_TTL = 60_000;
const CAPTURE_TIMEOUT = 10_000;
const THUMBNAIL_MAX_WIDTH = 320;

// ---------------------------------------------------------------------------
// Module-level state
// ---------------------------------------------------------------------------

let previewPc: RTCPeerConnection | null = null;
let previewPendingIce: RTCIceCandidateInit[] = [];
let previewCaptureResolve: ((url: string | null) => void) | null = null;
let previewTimeoutHandle: ReturnType<typeof setTimeout> | null = null;

const thumbnailCache = new Map<number, { dataUrl: string; capturedAt: number }>();

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function sendSignal(targetSession: number, signalType: number, payload: string): void {
  useAppStore.getState().sendWebRtcSignal(targetSession, signalType, payload);
}

function finishCapture(url: string | null): void {
  if (previewCaptureResolve) {
    const resolve = previewCaptureResolve;
    previewCaptureResolve = null;
    resolve(url);
  }
}

export function closePreview(): void {
  if (previewTimeoutHandle !== null) {
    clearTimeout(previewTimeoutHandle);
    previewTimeoutHandle = null;
  }
  if (previewPc) {
    previewPc.close();
    previewPc = null;
  }
  previewPendingIce = [];
  finishCapture(null);
}

function flushPreviewIce(): void {
  if (!previewPc) return;
  for (const c of previewPendingIce) {
    previewPc.addIceCandidate(c).catch((e) =>
      console.error("[sfu-preview] addIceCandidate error:", e),
    );
  }
  previewPendingIce = [];
}

// ---------------------------------------------------------------------------
// Frame capture
// ---------------------------------------------------------------------------

function captureFrame(track: MediaStreamTrack): Promise<string | null> {
  return new Promise((resolve) => {
    const video = document.createElement("video");
    video.srcObject = new MediaStream([track]);
    video.muted = true;
    video.playsInline = true;

    const cleanup = () => {
      video.pause();
      video.srcObject = null;
    };

    const grabFrame = () => {
      if (video.videoWidth === 0 || video.videoHeight === 0) {
        cleanup();
        resolve(null);
        return;
      }
      const scale = Math.min(1, THUMBNAIL_MAX_WIDTH / video.videoWidth);
      const w = Math.round(video.videoWidth * scale);
      const h = Math.round(video.videoHeight * scale);

      const canvas = document.createElement("canvas");
      canvas.width = w;
      canvas.height = h;
      const ctx = canvas.getContext("2d");
      if (ctx) {
        ctx.drawImage(video, 0, 0, w, h);
        cleanup();
        resolve(canvas.toDataURL("image/jpeg", 0.7));
      } else {
        cleanup();
        resolve(null);
      }
    };

    video.play().then(() => {
      if ("requestVideoFrameCallback" in video) {
        (video as HTMLVideoElement & { requestVideoFrameCallback: (cb: () => void) => void })
          .requestVideoFrameCallback(grabFrame);
      } else {
        setTimeout(grabFrame, 500);
      }
    }).catch(() => {
      cleanup();
      resolve(null);
    });
  });
}

// ---------------------------------------------------------------------------
// Thumbnail request
// ---------------------------------------------------------------------------

function requestThumbnail(broadcasterSession: number): Promise<string | null> {
  const cached = thumbnailCache.get(broadcasterSession);
  if (cached && Date.now() - cached.capturedAt < THUMBNAIL_TTL) {
    return Promise.resolve(cached.dataUrl);
  }

  closePreview();

  return new Promise((resolve) => {
    previewCaptureResolve = resolve;

    const pc = new RTCPeerConnection(RTC_CONFIG);
    previewPc = pc;

    previewTimeoutHandle = setTimeout(() => {
      console.warn("[sfu-preview] thumbnail capture timed out");
      closePreview();
    }, CAPTURE_TIMEOUT);

    pc.addTransceiver("video", { direction: "recvonly" });

    pc.ontrack = (e) => {
      captureFrame(e.track).then((url) => {
        if (url) {
          thumbnailCache.set(broadcasterSession, { dataUrl: url, capturedAt: Date.now() });
        }
        finishCapture(url);
        closePreview();
      });
    };

    pc.onicecandidate = (e) => {
      if (e.candidate) {
        sendSignal(broadcasterSession, SIGNAL_ICE_CANDIDATE, JSON.stringify(e.candidate.toJSON()));
      }
    };

    pc.onconnectionstatechange = () => {
      if (pc !== previewPc) return;
      if (pc.connectionState === "failed") {
        closePreview();
      }
    };

    pc.createOffer()
      .then(async (offer) => {
        if (previewPc !== pc) return;
        await pc.setLocalDescription(offer);
        if (previewPc !== pc) return;
        sendSignal(broadcasterSession, SIGNAL_SDP_OFFER, offer.sdp ?? "");
      })
      .catch(() => closePreview());
  });
}

// ---------------------------------------------------------------------------
// Signal routing (called from useScreenShare's handleSignal)
// ---------------------------------------------------------------------------

export function getPreviewPc(): RTCPeerConnection | null {
  return previewPc;
}

export function handlePreviewAnswer(sdp: string): void {
  if (!previewPc) return;
  previewPc
    .setRemoteDescription({ type: "answer", sdp })
    .then(flushPreviewIce)
    .catch((e) => console.error("[sfu-preview] setRemoteDescription error:", e));
}

export function handlePreviewIceCandidate(candidate: RTCIceCandidateInit): void {
  if (!previewPc) return;
  if (previewPc.remoteDescription) {
    previewPc.addIceCandidate(candidate).catch(console.error);
  } else {
    previewPendingIce.push(candidate);
  }
}

export function clearThumbnail(session: number): void {
  thumbnailCache.delete(session);
}

// ---------------------------------------------------------------------------
// React hook
// ---------------------------------------------------------------------------

/**
 * Returns a thumbnail data URL for a broadcasting user's screen share.
 * Only fetches / refreshes while `isHovering` is true.  Cached for 60 s.
 */
export function useStreamThumbnail(session: number, isHovering: boolean): string | null {
  const isBroadcasting = useAppStore((s) => s.broadcastingSessions.has(session));
  const [thumbnail, setThumbnail] = useState<string | null>(() => {
    const cached = thumbnailCache.get(session);
    return cached ? cached.dataUrl : null;
  });
  const inFlight = useRef(false);

  useEffect(() => {
    if (!isHovering || !isBroadcasting) return;

    let cancelled = false;

    const fetchIfStale = () => {
      if (inFlight.current || cancelled) return;
      const cached = thumbnailCache.get(session);
      if (cached && Date.now() - cached.capturedAt < THUMBNAIL_TTL) {
        setThumbnail(cached.dataUrl);
        return;
      }
      inFlight.current = true;
      requestThumbnail(session).then((url) => {
        inFlight.current = false;
        if (!cancelled && url) setThumbnail(url);
      });
    };

    fetchIfStale();

    const interval = setInterval(fetchIfStale, THUMBNAIL_TTL);
    return () => {
      cancelled = true;
      clearInterval(interval);
      closePreview();
    };
  }, [session, isHovering, isBroadcasting]);

  if (!isBroadcasting) return null;
  return thumbnail;
}
