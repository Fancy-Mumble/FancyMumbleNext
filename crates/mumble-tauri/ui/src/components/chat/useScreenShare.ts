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
// Viewer state (module-level - one active watch at a time)
// ---------------------------------------------------------------------------

let viewerPc: RTCPeerConnection | null = null;
let viewerPendingIce: RTCIceCandidateInit[] = [];
let viewerRemoteStream: MediaStream | null = null;
/** Callbacks registered by the ScreenShareViewer component to receive the remote stream. */
const remoteStreamListeners = new Set<(stream: MediaStream | null) => void>();

function notifyRemoteStreamListeners(stream: MediaStream | null): void {
  for (const cb of remoteStreamListeners) cb(stream);
}

/** Flush queued ICE candidates after viewer remote description is set. */
function flushViewerIce(): void {
  if (!viewerPc) return;
  for (const c of viewerPendingIce) {
    viewerPc.addIceCandidate(c).catch((e) =>
      console.error("[sfu] viewer addIceCandidate error:", e),
    );
  }
  viewerPendingIce = [];
}

function closeViewer(): void {
  if (viewerPc) {
    viewerPc.close();
    viewerPc = null;
  }
  viewerPendingIce = [];
  viewerRemoteStream = null;
  notifyRemoteStreamListeners(null);
}

/** Connect to the server SFU to watch a broadcaster's stream. */
async function startWatching(broadcasterSession: number): Promise<void> {
  closeViewer();

  const pc = new RTCPeerConnection(RTC_CONFIG);
  viewerPc = pc;

  pc.addTransceiver("video", { direction: "recvonly" });
  pc.addTransceiver("audio", { direction: "recvonly" });

  pc.ontrack = (e) => {
    // Accumulate all tracks into one MediaStream so a late audio
    // ontrack doesn't overwrite the video stream (str0m may use
    // different MSIDs per media section).
    if (!viewerRemoteStream) {
      viewerRemoteStream = new MediaStream();
    }
    if (!viewerRemoteStream.getTrackById(e.track.id)) {
      viewerRemoteStream.addTrack(e.track);
    }
    notifyRemoteStreamListeners(viewerRemoteStream);
  };

  // Send our ICE candidates to the server (routed via broadcaster session).
  pc.onicecandidate = (e) => {
    if (e.candidate) {
      sendSignal(broadcasterSession, SIGNAL_ICE_CANDIDATE, JSON.stringify(e.candidate.toJSON()));
    }
  };

  pc.onconnectionstatechange = () => {
    if (pc !== viewerPc) return; // stale closure
    if (pc.connectionState === "failed" || pc.connectionState === "disconnected") {
      closeViewer();
      useAppStore.setState({ watchingSession: null });
    }
  };

  const offer = await pc.createOffer();
  if (viewerPc !== pc) return; // replaced while awaiting
  await pc.setLocalDescription(offer);
  if (viewerPc !== pc) return; // replaced while awaiting

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

function handleSignal(senderSession: number, targetSession: number | null, signalType: number, payload: string): void {
  const ownSession = useAppStore.getState().ownSession;
  if (senderSession === ownSession) return; // ignore own echoes

  // Ignore signals targeted at other sessions (e.g. SDP answers the server
  // broadcasts to all channel members but only one session should process).
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
          // Auto-stop watching if we were watching this person.
          watchingSession: s.watchingSession === senderSession ? null : s.watchingSession,
        };
      });
      if (useAppStore.getState().watchingSession === null) {
        closeViewer();
      }
      break;

    case SIGNAL_SDP_ANSWER:
      // Server SFU answered our offer.
      // Route to the peer that is actually waiting for an answer.
      // Use signalingState instead of remoteDescription to avoid races
      // where a new PC was created but hasn't sent its offer yet.
      if (broadcasterPc?.signalingState === "have-local-offer") {
        handleServerAnswer(broadcasterPc, payload)
          .then(flushBroadcasterIce)
          .catch((e) => console.error("[sfu] broadcaster setRemoteDescription error:", e));
      } else if (viewerPc?.signalingState === "have-local-offer") {
        handleServerAnswer(viewerPc, payload)
          .then(flushViewerIce)
          .catch((e) => console.error("[sfu] viewer setRemoteDescription error:", e));
      } else {
        console.warn(
          "[sfu] SDP answer received but no peer is expecting one",
          "broadcaster:", broadcasterPc?.signalingState,
          "viewer:", viewerPc?.signalingState,
        );
      }
      break;

    case SIGNAL_ICE_CANDIDATE: {
      // Server sent us an ICE candidate (only if server is not using ICE-lite).
      let candidate: RTCIceCandidateInit | null = null;
      try {
        candidate = JSON.parse(payload) as RTCIceCandidateInit;
      } catch {
        break;
      }
      if (!candidate) break;

      // Route to active peer. Prefer broadcaster if both exist.
      if (broadcasterPc) {
        if (broadcasterPc.remoteDescription) {
          broadcasterPc.addIceCandidate(candidate).catch(console.error);
        } else {
          broadcasterPendingIce.push(candidate);
        }
      } else if (viewerPc) {
        if (viewerPc.remoteDescription) {
          viewerPc.addIceCandidate(candidate).catch(console.error);
        } else {
          viewerPendingIce.push(candidate);
        }
      }
      break;
    }

    // SDP_OFFER is not expected from the server in the SFU model.
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

  const watchBroadcast = useCallback((session: number) => {
    useAppStore.setState({ watchingSession: session });
    startWatching(session).catch((e) =>
      console.error("[screenshare] startWatching failed:", e),
    );
  }, []);

  const stopWatchingCb = useCallback(() => {
    closeViewer();
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
 * Subscribe to the remote MediaStream when watching a broadcast.
 * Returns the current remote stream (or null).
 */
export function useRemoteStream(): MediaStream | null {
  const [stream, setStream] = useState<MediaStream | null>(viewerRemoteStream);

  useEffect(() => {
    const handler = (s: MediaStream | null) => setStream(s);
    remoteStreamListeners.add(handler);
    // In case the stream was already set before we subscribed.
    setStream(viewerRemoteStream);
    return () => { remoteStreamListeners.delete(handler); };
  }, []);

  return stream;
}
