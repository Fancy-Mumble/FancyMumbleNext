/**
 * Browser-native screen sharing via getDisplayMedia + WebRTC.
 *
 * - Capture: uses navigator.mediaDevices.getDisplayMedia() (hardware-accel)
 * - Local preview: raw MediaStream in a <video> element (zero encoding)
 * - Remote viewing: WebRTC peer connections, signaled through a dedicated
 *   WebRtcSignal proto message (ID 120, send_webrtc_signal / onWebRtcSignal)
 *
 * SignalType enum (matches proto):
 *   START         = 0  - broadcaster announces
 *   STOP          = 1  - broadcaster stops
 *   SDP_OFFER     = 2  - viewer sends offer to broadcaster
 *   SDP_ANSWER    = 3  - broadcaster replies with answer
 *   ICE_CANDIDATE = 4  - ICE candidate exchange
 */
import { useEffect, useCallback, useState, useRef } from "react";
import { useAppStore, onWebRtcSignal } from "../../store";

// Proto SignalType enum values (must match Mumble.proto).
const SIGNAL_START = 0;
const SIGNAL_STOP = 1;
const SIGNAL_SDP_OFFER = 2;
const SIGNAL_SDP_ANSWER = 3;
const SIGNAL_ICE_CANDIDATE = 4;

// Public STUN servers for NAT traversal.
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

/** Peer connections from broadcaster to each viewer, keyed by viewer session. */
const viewerPeers = new Map<number, RTCPeerConnection>();

/** Pending ICE candidates received before the peer connection was ready. */
const pendingIceCandidates = new Map<number, RTCIceCandidateInit[]>();

/** Clean up a single viewer peer connection. */
function closeViewerPeer(viewerSession: number): void {
  const pc = viewerPeers.get(viewerSession);
  if (pc) {
    pc.close();
    viewerPeers.delete(viewerSession);
  }
  pendingIceCandidates.delete(viewerSession);
}

/** Clean up all broadcaster state. */
function stopBroadcasting(): void {
  if (localStream) {
    for (const track of localStream.getTracks()) track.stop();
    localStream = null;
  }
  for (const session of [...viewerPeers.keys()]) {
    closeViewerPeer(session);
  }
}

/** Handle an offer from a viewer who wants to watch our broadcast. */
async function handleViewerOffer(viewerSession: number, sdp: string): Promise<void> {
  if (!localStream) return;

  // Close any existing connection from this viewer (renegotiation).
  closeViewerPeer(viewerSession);

  const pc = new RTCPeerConnection(RTC_CONFIG);
  viewerPeers.set(viewerSession, pc);

  // Add our screen tracks to the connection.
  for (const track of localStream.getTracks()) {
    pc.addTrack(track, localStream);
  }

  // Forward ICE candidates to the viewer.
  pc.onicecandidate = (e) => {
    if (e.candidate) {
      sendSignal(viewerSession, SIGNAL_ICE_CANDIDATE, JSON.stringify(e.candidate.toJSON()));
    }
  };

  await pc.setRemoteDescription({ type: "offer", sdp });

  // Flush any ICE candidates that arrived before we had the peer.
  const queued = pendingIceCandidates.get(viewerSession);
  if (queued) {
    for (const c of queued) await pc.addIceCandidate(c);
    pendingIceCandidates.delete(viewerSession);
  }

  const answer = await pc.createAnswer();
  await pc.setLocalDescription(answer);

  sendSignal(viewerSession, SIGNAL_SDP_ANSWER, answer.sdp!);
}

// ---------------------------------------------------------------------------
// Viewer state (module-level - one active watch at a time)
// ---------------------------------------------------------------------------

let viewerPc: RTCPeerConnection | null = null;
let viewerRemoteStream: MediaStream | null = null;
/** Callbacks registered by the ScreenShareViewer component to receive the remote stream. */
const remoteStreamListeners = new Set<(stream: MediaStream | null) => void>();

function notifyRemoteStreamListeners(stream: MediaStream | null): void {
  for (const cb of remoteStreamListeners) cb(stream);
}

function closeViewer(): void {
  if (viewerPc) {
    viewerPc.close();
    viewerPc = null;
  }
  viewerRemoteStream = null;
  notifyRemoteStreamListeners(null);
}

/** Start watching a broadcaster. Creates an RTCPeerConnection and sends an offer. */
async function startWatching(broadcasterSession: number): Promise<void> {
  closeViewer();

  const pc = new RTCPeerConnection(RTC_CONFIG);
  viewerPc = pc;

  pc.addTransceiver("video", { direction: "recvonly" });
  pc.addTransceiver("audio", { direction: "recvonly" });

  pc.ontrack = (e) => {
    // Use the first stream from the track event.
    const stream = e.streams[0] ?? new MediaStream([e.track]);
    viewerRemoteStream = stream;
    notifyRemoteStreamListeners(stream);
  };

  pc.onicecandidate = (e) => {
    if (e.candidate) {
      sendSignal(broadcasterSession, SIGNAL_ICE_CANDIDATE, JSON.stringify(e.candidate.toJSON()));
    }
  };

  pc.onconnectionstatechange = () => {
    if (pc.connectionState === "failed" || pc.connectionState === "disconnected") {
      closeViewer();
    }
  };

  const offer = await pc.createOffer();
  await pc.setLocalDescription(offer);

  sendSignal(broadcasterSession, SIGNAL_SDP_OFFER, offer.sdp!);
}

/** Handle an answer from the broadcaster to our offer. */
async function handleBroadcasterAnswer(sdp: string): Promise<void> {
  if (!viewerPc) return;
  await viewerPc.setRemoteDescription({ type: "answer", sdp });
}

/** Handle an ICE candidate from a peer (either broadcaster or viewer). */
async function handleIceCandidate(
  senderSession: number,
  candidate: RTCIceCandidateInit | null,
): Promise<void> {
  if (!candidate) return;

  // Check if this is for our broadcaster connection (we are a viewer).
  if (viewerPc && viewerPc.remoteDescription) {
    await viewerPc.addIceCandidate(candidate);
    return;
  }

  // Check if this is for one of our viewer connections (we are broadcasting).
  const pc = viewerPeers.get(senderSession);
  if (pc && pc.remoteDescription) {
    await pc.addIceCandidate(candidate);
    return;
  }

  // Queue the candidate - the peer connection may not be ready yet.
  const existing = pendingIceCandidates.get(senderSession) ?? [];
  existing.push(candidate);
  pendingIceCandidates.set(senderSession, existing);
}

// ---------------------------------------------------------------------------
// Incoming signal dispatcher
// ---------------------------------------------------------------------------

function handleSignal(senderSession: number, signalType: number, payload: string): void {
  const ownSession = useAppStore.getState().ownSession;
  if (senderSession === ownSession) return; // ignore own echoes

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

    case SIGNAL_SDP_OFFER:
      // Someone wants to watch our broadcast.
      handleViewerOffer(senderSession, payload).catch((e) =>
        console.error("[screenshare] failed to handle viewer offer:", e),
      );
      break;

    case SIGNAL_SDP_ANSWER:
      // The broadcaster answered our watch request.
      handleBroadcasterAnswer(payload).catch((e) =>
        console.error("[screenshare] failed to handle answer:", e),
      );
      break;

    case SIGNAL_ICE_CANDIDATE: {
      let candidate: RTCIceCandidateInit | null = null;
      try {
        candidate = JSON.parse(payload) as RTCIceCandidateInit;
      } catch {
        // ignore malformed ICE
      }
      handleIceCandidate(senderSession, candidate).catch((e) =>
        console.error("[screenshare] failed to handle ICE candidate:", e),
      );
      break;
    }
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
    const unregister = onWebRtcSignal((senderSession, signalType, payload) => {
      if (senderSession === null) return;
      handleSignal(senderSession, signalType, payload);
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
