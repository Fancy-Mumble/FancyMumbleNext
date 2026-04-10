/**
 * Server-relayed screen sharing via getDisplayMedia + WebRTC SFU.
 *
 * Architecture: the broadcaster sends ONE WebRTC stream to the Mumble
 * server's SFU (Selective Forwarding Unit), which re-broadcasts it to
 * each viewer via separate WebRTC connections.  Broadcaster upload is
 * O(1) regardless of viewer count.
 *
 * All signaling travels over the existing Mumble TCP connection using
 * WebRtcSignal protobuf messages (ID 120).  Media flows via WebRTC
 * UDP between each client and the server (never client-to-client).
 *
 * SignalType enum (matches proto):
 *   START         = 0  - broadcaster announces (channel broadcast)
 *   STOP          = 1  - broadcaster stops (channel broadcast)
 *   SDP_OFFER     = 2  - client sends offer to server SFU
 *   SDP_ANSWER    = 3  - server SFU replies with answer
 *   ICE_CANDIDATE = 4  - client sends ICE candidate to server
 */
import { useEffect, useCallback, useState, useRef } from "react";
import { useAppStore, onWebRtcSignal } from "../../store";
import {
  getPreviewPc,
  handlePreviewAnswer,
  handlePreviewIceCandidate,
  clearThumbnail,
  closePreview,
  storeLocalThumbnail,
} from "./useStreamPreview";

// Proto SignalType enum values (must match Mumble.proto).
const SIGNAL_START = 0;
const SIGNAL_STOP = 1;
const SIGNAL_SDP_OFFER = 2;
const SIGNAL_SDP_ANSWER = 3;
const SIGNAL_ICE_CANDIDATE = 4;

// STUN servers for the client to discover its public address.
// The server SFU uses ICE-lite and needs no STUN.
const RTC_CONFIG: RTCConfiguration = {
  iceServers: [
    { urls: "stun:stun.l.google.com:19302" },
    { urls: "stun:stun1.l.google.com:19302" },
  ],
};

// ---------------------------------------------------------------------------
// Signal helpers
// ---------------------------------------------------------------------------

/** Send a signaling message to a specific session (or 0 for channel broadcast). */
function sendSignal(targetSession: number, signalType: number, payload: string): void {
  const { sendWebRtcSignal } = useAppStore.getState();
  sendWebRtcSignal(targetSession, signalType, payload);
}

/** Broadcast a signal to all users in our channel (target_session = 0). */
function broadcastSignal(signalType: number, payload: string): void {
  sendSignal(0, signalType, payload);
}

// ---------------------------------------------------------------------------
// Broadcaster state (module-level singleton - only one broadcast at a time)
// ---------------------------------------------------------------------------

/** Active local media stream from getDisplayMedia. */
let localStream: MediaStream | null = null;

/** Single peer connection from broadcaster to the server SFU. */
let broadcasterPc: RTCPeerConnection | null = null;

/** ICE candidates received before the broadcaster peer had a remote description. */
let broadcasterPendingIce: RTCIceCandidateInit[] = [];

/** Interval handle for periodic WebRTC stats logging. */
let broadcasterStatsInterval: ReturnType<typeof setInterval> | null = null;

/** Log outbound video stats from the broadcaster PC for diagnostics. */
function startBroadcasterStatsLog(pc: RTCPeerConnection): void {
  stopBroadcasterStatsLog();
  broadcasterStatsInterval = setInterval(async () => {
    if (pc.connectionState !== "connected") return;
    const stats = await pc.getStats();
    stats.forEach((report) => {
      if (report.type === "outbound-rtp" && report.kind === "video") {
        console.log(
          `[sfu] outbound-rtp: qualityLimitationReason=${report.qualityLimitationReason}` +
            ` targetBitrate=${report.targetBitrate}` +
            ` bytesSent=${report.bytesSent}` +
            ` packetsSent=${report.packetsSent}` +
            ` framesPerSecond=${report.framesPerSecond}` +
            ` frameWidth=${report.frameWidth}x${report.frameHeight}` +
            ` encoderImplementation=${report.encoderImplementation}`,
        );
      }
    });
  }, 2000);
}

function stopBroadcasterStatsLog(): void {
  if (broadcasterStatsInterval !== null) {
    clearInterval(broadcasterStatsInterval);
    broadcasterStatsInterval = null;
  }
}

/** Flush queued ICE candidates after remote description is set. */
function flushBroadcasterIce(): void {
  if (!broadcasterPc) return;
  for (const c of broadcasterPendingIce) {
    broadcasterPc.addIceCandidate(c).catch((e) =>
      console.error("[sfu] broadcaster addIceCandidate error:", e),
    );
  }
  broadcasterPendingIce = [];
}

/** Send our screen stream to the server SFU via a single WebRTC connection. */
async function connectBroadcasterToServer(): Promise<void> {
  if (!localStream) return;

  // Close any stale broadcaster peer.
  if (broadcasterPc) {
    broadcasterPc.close();
    broadcasterPc = null;
  }
  broadcasterPendingIce = [];

  const pc = new RTCPeerConnection(RTC_CONFIG);
  broadcasterPc = pc;

  // Add screen tracks (video + optional audio).
  for (const track of localStream.getTracks()) {
    pc.addTrack(track, localStream);
  }

  // Tell Chrome to prefer framerate over resolution when bandwidth is limited.
  // Without this, Chrome's default "balanced" degradation drops both framerate
  // and resolution, often resulting in <1fps screen shares through the SFU.
  for (const sender of pc.getSenders()) {
    if (sender.track?.kind === "video") {
      const params = sender.getParameters();
      params.degradationPreference = "maintain-framerate";
      await sender.setParameters(params);
    }
  }

  // Send our ICE candidates to the server (target=0).
  pc.onicecandidate = (e) => {
    if (e.candidate) {
      sendSignal(0, SIGNAL_ICE_CANDIDATE, JSON.stringify(e.candidate.toJSON()));
    }
  };

  pc.onconnectionstatechange = () => {
    if (pc !== broadcasterPc) return; // stale closure
    if (pc.connectionState === "connected") {
      startBroadcasterStatsLog(pc);
    } else if (pc.connectionState === "failed" || pc.connectionState === "disconnected") {
      console.warn("[sfu] broadcaster connection to server lost");
      stopBroadcasterStatsLog();
    }
  };

  const offer = await pc.createOffer();
  if (broadcasterPc !== pc) return; // replaced while awaiting
  await pc.setLocalDescription(offer);
  if (broadcasterPc !== pc) return; // replaced while awaiting

  // Send offer to the server SFU (target=0 tells server this is our broadcast offer).
  sendSignal(0, SIGNAL_SDP_OFFER, offer.sdp!);
}

/** Clean up all broadcaster state. */
function stopBroadcasting(): void {
  stopBroadcasterStatsLog();
  if (localStream) {
    for (const track of localStream.getTracks()) track.stop();
    localStream = null;
  }
  if (broadcasterPc) {
    broadcasterPc.close();
    broadcasterPc = null;
  }
  broadcasterPendingIce = [];
}

// ---------------------------------------------------------------------------
// Viewer state (module-level - one WebRTC connection per broadcaster)
// ---------------------------------------------------------------------------

interface ViewerState {
  pc: RTCPeerConnection;
  pendingIce: RTCIceCandidateInit[];
  stream: MediaStream | null;
}

const viewerPcs = new Map<number, ViewerState>();
const remoteStreamListeners = new Map<number, Set<(stream: MediaStream | null) => void>>();

function notifyStreamListeners(session: number, stream: MediaStream | null): void {
  const listeners = remoteStreamListeners.get(session);
  if (listeners) {
    for (const cb of listeners) cb(stream);
  }
}

function flushViewerIce(session: number): void {
  const state = viewerPcs.get(session);
  if (!state) return;
  for (const c of state.pendingIce) {
    state.pc.addIceCandidate(c).catch((e) =>
      console.error("[sfu] viewer addIceCandidate error:", e),
    );
  }
  state.pendingIce = [];
}

function closeViewer(session?: number): void {
  if (session !== undefined) {
    const state = viewerPcs.get(session);
    if (state) {
      state.pc.close();
      viewerPcs.delete(session);
      notifyStreamListeners(session, null);
    }
  } else {
    for (const [sess, state] of viewerPcs) {
      state.pc.close();
      notifyStreamListeners(sess, null);
    }
    viewerPcs.clear();
  }
}

/** Connect to the server SFU to watch a broadcaster's stream. Returns immediately if already connected. */
async function startWatching(broadcasterSession: number): Promise<void> {
  if (viewerPcs.has(broadcasterSession)) return;

  closePreview();

  const pc = new RTCPeerConnection(RTC_CONFIG);
  const state: ViewerState = { pc, pendingIce: [], stream: null };
  viewerPcs.set(broadcasterSession, state);

  pc.addTransceiver("video", { direction: "recvonly" });
  pc.addTransceiver("audio", { direction: "recvonly" });

  pc.ontrack = (e) => {
    const s = viewerPcs.get(broadcasterSession);
    if (!s) return;
    s.stream ??= new MediaStream();
    if (!s.stream.getTrackById(e.track.id)) {
      s.stream.addTrack(e.track);
    }
    notifyStreamListeners(broadcasterSession, s.stream);
  };

  // Send our ICE candidates to the server (routed via broadcaster session).
  pc.onicecandidate = (e) => {
    if (e.candidate) {
      sendSignal(broadcasterSession, SIGNAL_ICE_CANDIDATE, JSON.stringify(e.candidate.toJSON()));
    }
  };

  pc.onconnectionstatechange = () => {
    if (viewerPcs.get(broadcasterSession)?.pc !== pc) return; // stale closure
    if (pc.connectionState === "failed" || pc.connectionState === "disconnected") {
      closeViewer(broadcasterSession);
      const { watchingSession } = useAppStore.getState();
      if (watchingSession === broadcasterSession) {
        useAppStore.setState({ watchingSession: null });
      }
    }
  };

  const offer = await pc.createOffer();
  if (viewerPcs.get(broadcasterSession)?.pc !== pc) return; // replaced while awaiting
  await pc.setLocalDescription(offer);
  if (viewerPcs.get(broadcasterSession)?.pc !== pc) return; // replaced while awaiting

  // Send offer to server, targeting the broadcaster session.
  // The server intercepts this and creates an SFU outbound peer.
  sendSignal(broadcasterSession, SIGNAL_SDP_OFFER, offer.sdp!);
}

/** Handle an SDP answer from the server SFU. */
async function handleServerAnswer(pc: RTCPeerConnection, sdp: string): Promise<void> {
  await pc.setRemoteDescription({ type: "answer", sdp });
}

// ---------------------------------------------------------------------------
// Incoming signal dispatcher
// ---------------------------------------------------------------------------

/** Route an SDP answer to the peer that is waiting for one. */
function routeSdpAnswer(senderSession: number, payload: string): void {
  const ownSession = useAppStore.getState().ownSession;

  // Answer for our broadcaster PC: the SFU echoes back our own session as
  // the broadcaster context, so senderSession equals our session ID.
  if (senderSession === ownSession && broadcasterPc?.signalingState === "have-local-offer") {
    handleServerAnswer(broadcasterPc, payload)
      .then(flushBroadcasterIce)
      .catch((e) => console.error("[sfu] broadcaster setRemoteDescription error:", e));
    return;
  }

  // Answer for a viewer PC: senderSession is the broadcaster's session.
  const state = viewerPcs.get(senderSession);
  if (state?.pc.signalingState === "have-local-offer") {
    handleServerAnswer(state.pc, payload)
      .then(() => flushViewerIce(senderSession))
      .catch((e) => console.error("[sfu] viewer setRemoteDescription error:", e));
    return;
  }

  if (getPreviewPc()?.signalingState === "have-local-offer") {
    handlePreviewAnswer(payload);
    return;
  }

  console.warn(
    "[sfu] SDP answer received but no peer is expecting one",
    { senderSession, viewerSessions: [...viewerPcs.keys()] },
  );
}

/** Route an ICE candidate to the correct peer (broadcaster > viewer by sender session > preview). */
function routeIceCandidate(senderSession: number, payload: string): void {
  let candidate: RTCIceCandidateInit | null = null;
  try {
    candidate = JSON.parse(payload) as RTCIceCandidateInit;
  } catch {
    return;
  }
  if (!candidate) return;

  if (broadcasterPc) {
    if (broadcasterPc.remoteDescription) {
      broadcasterPc.addIceCandidate(candidate).catch(console.error);
    } else {
      broadcasterPendingIce.push(candidate);
    }
    return;
  }

  const viewerState = viewerPcs.get(senderSession);
  if (viewerState) {
    if (viewerState.pc.remoteDescription) {
      viewerState.pc.addIceCandidate(candidate).catch(console.error);
    } else {
      viewerState.pendingIce.push(candidate);
    }
    return;
  }

  if (getPreviewPc()) {
    handlePreviewIceCandidate(candidate);
  }
}

function handleSignal(senderSession: number, targetSession: number | null, signalType: number, payload: string): void {
  const ownSession = useAppStore.getState().ownSession;
  // SDP_ANSWER sender_session is the broadcaster context (not the human sender),
  // so skip the own-session filter for that signal type.
  if (signalType !== SIGNAL_SDP_ANSWER && senderSession === ownSession) return;
  if (targetSession !== null && targetSession !== 0 && targetSession !== ownSession) return;

  switch (signalType) {
    case SIGNAL_START:
      useAppStore.setState((s) => {
        const next = new Set(s.broadcastingSessions);
        next.add(senderSession);
        return { broadcastingSessions: next };
      });
      break;

    case SIGNAL_STOP:
      useAppStore.setState((s) => {
        const next = new Set(s.broadcastingSessions);
        next.delete(senderSession);
        return {
          broadcastingSessions: next,
          watchingSession: s.watchingSession === senderSession ? null : s.watchingSession,
        };
      });
      clearThumbnail(senderSession);
      closeViewer(senderSession);
      break;

    case SIGNAL_SDP_ANSWER:
      routeSdpAnswer(senderSession, payload);
      break;

    case SIGNAL_ICE_CANDIDATE:
      routeIceCandidate(senderSession, payload);
      break;

    default:
      break;
  }
}

// ---------------------------------------------------------------------------
// Public hook
// ---------------------------------------------------------------------------

export interface ScreenShareHook {
  /** Whether we are currently broadcasting our screen. */
  isBroadcasting: boolean;
  /** Session IDs of other users currently broadcasting. */
  broadcastingSessions: Set<number>;
  /** Session we are currently watching (null if not watching). */
  watchingSession: number | null;
  /** The local MediaStream (for own preview). null if not broadcasting. */
  localStream: MediaStream | null;
  /** Start sharing our screen. */
  startSharing: () => Promise<void>;
  /** Stop sharing our screen. */
  stopSharing: () => void;
  /** Start watching another user's broadcast. */
  watchBroadcast: (session: number) => void;
  /** Stop watching. */
  stopWatching: () => void;
}

export function useScreenShare(): ScreenShareHook {
  const ownSession = useAppStore((s) => s.ownSession);
  const users = useAppStore((s) => s.users);
  const currentChannel = useAppStore((s) => s.currentChannel);
  const broadcastingSessions = useAppStore((s) => s.broadcastingSessions);
  const watchingSession = useAppStore((s) => s.watchingSession);
  const isBroadcasting = useAppStore((s) => s.isSharingOwn);
  const [stream, setStream] = useState<MediaStream | null>(localStream);

  // Track channel members so we can re-announce to late joiners.
  const prevChannelSessionsRef = useRef<Set<number>>(new Set());

  // Register the WebRTC signal handler for screen share signaling.
  useEffect(() => {
    const unregister = onWebRtcSignal((senderSession, targetSession, signalType, payload) => {
      if (senderSession === null) return;
      handleSignal(senderSession, targetSession, signalType, payload);
    });
    return unregister;
  }, []);

  // Re-announce broadcast when new users join our channel (late-joiner fix).
  useEffect(() => {
    if (!localStream || !ownSession || currentChannel === null) return;
    const currentSessions = new Set(
      users.filter((u) => u.channel_id === currentChannel).map((u) => u.session),
    );
    const prev = prevChannelSessionsRef.current;
    // Check if any sessions are new (not in previous set).
    const hasNewMembers = [...currentSessions].some((s) => s !== ownSession && !prev.has(s));
    if (hasNewMembers) {
      broadcastSignal(SIGNAL_START, "");
    }
    prevChannelSessionsRef.current = currentSessions;
  }, [users, currentChannel, ownSession]);

  // Clean up when the user disconnects.
  useEffect(() => {
    if (!ownSession) {
      stopBroadcasting();
      closeViewer();
      setStream(null);
    }
  }, [ownSession]);

  // Maintain a live thumbnail of the own stream so it can appear as a
  // secondary panel in StreamFocusView while watching another broadcaster.
  // Refreshes every 55 s (well within the 60 s TTL) to prevent stale cache.
  useEffect(() => {
    if (!isBroadcasting || !stream || !ownSession) return;
    storeLocalThumbnail(ownSession, stream).catch(console.error);
    const interval = setInterval(() => {
      if (localStream) storeLocalThumbnail(ownSession, localStream).catch(console.error);
    }, 55_000);
    return () => {
      clearInterval(interval);
      clearThumbnail(ownSession);
    };
  }, [isBroadcasting, stream, ownSession]);

  const startSharing = useCallback(async () => {
    if (localStream) return; // already broadcasting

    const { serverConfig } = useAppStore.getState();
    if (serverConfig.webrtc_sfu_available) {
      console.info("[screen-share] server has WebRTC SFU - media will be relayed via server");
    } else {
      console.warn("[screen-share] server does NOT have WebRTC SFU - screen sharing may not work");
    }

    try {
      const mediaStream = await navigator.mediaDevices.getDisplayMedia({
        video: true,
        audio: true,
      });

      localStream = mediaStream;
      setStream(mediaStream);
      useAppStore.setState((s) => {
        const next = new Set(s.broadcastingSessions);
        if (ownSession) next.add(ownSession);
        return { isSharingOwn: true, broadcastingSessions: next };
      });

      // Announce to all channel members.
      broadcastSignal(SIGNAL_START, "");

      // Connect to the server SFU (single WebRTC connection).
      await connectBroadcasterToServer();

      // Listen for the user stopping via the browser's built-in "Stop sharing" button.
      const videoTrack = mediaStream.getVideoTracks()[0];
      if (videoTrack) {
        videoTrack.addEventListener("ended", () => {
          stopBroadcasting();
          setStream(null);
          useAppStore.setState((s) => {
            const next = new Set(s.broadcastingSessions);
            const own = useAppStore.getState().ownSession;
            if (own) next.delete(own);
            return { isSharingOwn: false, broadcastingSessions: next };
          });
          broadcastSignal(SIGNAL_STOP, "");
        });
      }
    } catch (e) {
      // User cancelled the screen picker dialog - not an error.
      console.warn("[screenshare] getDisplayMedia failed or cancelled:", e);
    }
  }, [ownSession]);

  const stopSharingCb = useCallback(() => {
    stopBroadcasting();
    setStream(null);
    useAppStore.setState((s) => {
      const next = new Set(s.broadcastingSessions);
      if (ownSession) next.delete(ownSession);
      return { isSharingOwn: false, broadcastingSessions: next };
    });
    if (ownSession) {
      broadcastSignal(SIGNAL_STOP, "");
    }
  }, [ownSession]);

  // Auto-connect to all active broadcasters in our channel so streams are
  // ready before the user clicks into focus view, and disconnect from
  // sessions that stopped broadcasting.
  useEffect(() => {
    if (!ownSession) return;
    for (const session of broadcastingSessions) {
      if (session !== ownSession && !viewerPcs.has(session)) {
        startWatching(session).catch((e) =>
          console.error("[screenshare] auto-connect failed for session", session, e),
        );
      }
    }
    for (const [session] of viewerPcs) {
      if (!broadcastingSessions.has(session)) {
        closeViewer(session);
      }
    }
  }, [broadcastingSessions, ownSession]);

  const watchBroadcast = useCallback((session: number) => {
    useAppStore.setState({ watchingSession: session });
    // startWatching is a no-op if already connected (auto-connect effect above).
    startWatching(session).catch((e) =>
      console.error("[screenshare] startWatching failed:", e),
    );
  }, []);

  const stopWatchingCb = useCallback(() => {
    useAppStore.setState({ watchingSession: null });
  }, []);

  return {
    isBroadcasting,
    broadcastingSessions,
    watchingSession,
    localStream: stream,
    startSharing,
    stopSharing: stopSharingCb,
    watchBroadcast,
    stopWatching: stopWatchingCb,
  };
}

// ---------------------------------------------------------------------------
// Remote stream hook for the viewer component
// ---------------------------------------------------------------------------

/**
 * Subscribe to the remote MediaStream for a specific broadcaster.
 * Returns the current stream for that session (or null while connecting).
 */
export function useRemoteStream(session: number): MediaStream | null {
  const [stream, setStream] = useState<MediaStream | null>(
    () => viewerPcs.get(session)?.stream ?? null,
  );

  useEffect(() => {
    const handler = (s: MediaStream | null) => setStream(s);
    let listeners = remoteStreamListeners.get(session);
    if (!listeners) {
      listeners = new Set();
      remoteStreamListeners.set(session, listeners);
    }
    listeners.add(handler);
    // Sync in case the stream arrived before we subscribed.
    setStream(viewerPcs.get(session)?.stream ?? null);
    return () => {
      const ls = remoteStreamListeners.get(session);
      if (ls) {
        ls.delete(handler);
        if (ls.size === 0) remoteStreamListeners.delete(session);
      }
    };
  }, [session]);

  return stream;
}
