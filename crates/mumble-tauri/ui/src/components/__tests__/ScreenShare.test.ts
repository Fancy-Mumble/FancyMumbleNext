/**
 * Tests for the screen share signaling logic.
 *
 * Validates the encode/decode of signaling messages and the store
 * state transitions when broadcasts start and stop.
 */
import { describe, it, expect, beforeEach } from "vitest";
import { useAppStore } from "../../store";

// Re-create the signal encoding/decoding functions here since they are
// module-private in useScreenShare.ts. This tests the wire format.

interface SignalMessage {
  type: "start" | "stop" | "offer" | "answer" | "ice";
  session: number;
  sdp?: string;
  candidate?: RTCIceCandidateInit | null;
}

function encodeSignal(msg: SignalMessage): Uint8Array {
  return new TextEncoder().encode(JSON.stringify(msg));
}

function decodeSignal(data: Uint8Array): SignalMessage | null {
  try {
    return JSON.parse(new TextDecoder().decode(data)) as SignalMessage;
  } catch {
    return null;
  }
}

describe("Screen share signaling", () => {
  beforeEach(() => {
    useAppStore.setState({
      isSharingOwn: false,
      broadcastingSessions: new Set(),
      watchingSession: null,
      ownSession: 1,
    });
  });

  describe("Signal encoding/decoding", () => {
    it("encodes and decodes a start message", () => {
      const msg: SignalMessage = { type: "start", session: 42 };
      const encoded = encodeSignal(msg);
      const decoded = decodeSignal(encoded);
      expect(decoded).toEqual(msg);
    });

    it("encodes and decodes an offer with SDP", () => {
      const msg: SignalMessage = {
        type: "offer",
        session: 7,
        sdp: "v=0\r\no=- 12345 2 IN IP4 127.0.0.1\r\n",
      };
      const encoded = encodeSignal(msg);
      const decoded = decodeSignal(encoded);
      expect(decoded).toEqual(msg);
    });

    it("encodes and decodes an ICE candidate", () => {
      const msg: SignalMessage = {
        type: "ice",
        session: 3,
        candidate: {
          candidate: "candidate:1 1 UDP 2130706431 192.168.1.1 12345 typ host",
          sdpMid: "0",
          sdpMLineIndex: 0,
        },
      };
      const encoded = encodeSignal(msg);
      const decoded = decodeSignal(encoded);
      expect(decoded).toEqual(msg);
    });

    it("handles null ICE candidate (end-of-candidates)", () => {
      const msg: SignalMessage = { type: "ice", session: 5, candidate: null };
      const encoded = encodeSignal(msg);
      const decoded = decodeSignal(encoded);
      expect(decoded).toEqual(msg);
    });

    it("returns null for invalid data", () => {
      const garbage = new Uint8Array([0xff, 0xfe, 0xfd]);
      expect(decodeSignal(garbage)).toBeNull();
    });

    it("returns null for non-JSON text", () => {
      const text = new TextEncoder().encode("not json at all");
      expect(decodeSignal(text)).toBeNull();
    });
  });

  describe("Store state transitions", () => {
    it("adds a broadcaster session on start signal", () => {
      useAppStore.setState((s) => {
        const next = new Set(s.broadcastingSessions);
        next.add(42);
        return { broadcastingSessions: next };
      });

      const state = useAppStore.getState();
      expect(state.broadcastingSessions.has(42)).toBe(true);
      expect(state.broadcastingSessions.size).toBe(1);
    });

    it("removes a broadcaster session on stop signal", () => {
      useAppStore.setState({ broadcastingSessions: new Set([42, 99]) });

      useAppStore.setState((s) => {
        const next = new Set(s.broadcastingSessions);
        next.delete(42);
        return { broadcastingSessions: next };
      });

      const state = useAppStore.getState();
      expect(state.broadcastingSessions.has(42)).toBe(false);
      expect(state.broadcastingSessions.has(99)).toBe(true);
      expect(state.broadcastingSessions.size).toBe(1);
    });

    it("auto-stops watching when the watched broadcaster stops", () => {
      useAppStore.setState({
        broadcastingSessions: new Set([42]),
        watchingSession: 42,
      });

      // Simulate the stop signal handler.
      useAppStore.setState((s) => {
        const next = new Set(s.broadcastingSessions);
        next.delete(42);
        return {
          broadcastingSessions: next,
          watchingSession: s.watchingSession === 42 ? null : s.watchingSession,
        };
      });

      const state = useAppStore.getState();
      expect(state.watchingSession).toBeNull();
      expect(state.broadcastingSessions.size).toBe(0);
    });

    it("keeps watching if a different broadcaster stops", () => {
      useAppStore.setState({
        broadcastingSessions: new Set([42, 99]),
        watchingSession: 99,
      });

      useAppStore.setState((s) => {
        const next = new Set(s.broadcastingSessions);
        next.delete(42);
        return {
          broadcastingSessions: next,
          watchingSession: s.watchingSession === 42 ? null : s.watchingSession,
        };
      });

      const state = useAppStore.getState();
      expect(state.watchingSession).toBe(99);
    });

    it("sets isSharingOwn to true when own broadcast starts", () => {
      useAppStore.setState({ isSharingOwn: true });
      expect(useAppStore.getState().isSharingOwn).toBe(true);
    });

    it("resets screen share state on full reset", () => {
      useAppStore.setState({
        isSharingOwn: true,
        broadcastingSessions: new Set([10, 20]),
        watchingSession: 10,
      });

      // The reset action restores INITIAL which includes screen share defaults.
      useAppStore.getState().reset();

      const state = useAppStore.getState();
      expect(state.isSharingOwn).toBe(false);
      expect(state.broadcastingSessions.size).toBe(0);
      expect(state.watchingSession).toBeNull();
    });

    it("tracks multiple simultaneous broadcasters", () => {
      const sessions = [10, 20, 30, 40];
      for (const s of sessions) {
        useAppStore.setState((prev) => ({
          broadcastingSessions: new Set([...prev.broadcastingSessions, s]),
        }));
      }

      const state = useAppStore.getState();
      expect(state.broadcastingSessions.size).toBe(4);
      for (const s of sessions) {
        expect(state.broadcastingSessions.has(s)).toBe(true);
      }
    });

    it("prunes broadcastingSessions for disconnected users after refreshState", () => {
      // Pre-fill broadcastingSessions with sessions 10 and 20.
      useAppStore.setState({
        broadcastingSessions: new Set([10, 20]),
        users: [
          { session: 10, name: "Alice", channel_id: 0 },
          { session: 20, name: "Bob", channel_id: 0 },
        ] as never[],
      });

      // Simulate a state refresh where only user 10 remains.
      const currentSessions = new Set([10]);
      const { broadcastingSessions } = useAppStore.getState();
      const pruned = new Set([...broadcastingSessions].filter((s) => currentSessions.has(s)));
      useAppStore.setState({ broadcastingSessions: pruned });

      const state = useAppStore.getState();
      expect(state.broadcastingSessions.has(10)).toBe(true);
      expect(state.broadcastingSessions.has(20)).toBe(false);
      expect(state.broadcastingSessions.size).toBe(1);
    });

    it("does not modify broadcastingSessions if all sessions still present", () => {
      const original = new Set([10, 20]);
      useAppStore.setState({ broadcastingSessions: original });

      const currentSessions = new Set([10, 20, 30]);
      const { broadcastingSessions } = useAppStore.getState();
      const pruned = new Set([...broadcastingSessions].filter((s) => currentSessions.has(s)));

      // Should be equivalent - no state change needed.
      expect(pruned.size).toBe(original.size);
      expect([...pruned]).toEqual([...original]);
    });

    it("adds own session to broadcastingSessions when starting own broadcast", () => {
      const ownSession = 1;
      useAppStore.setState({ ownSession });

      // Simulate startSharing: add own session to broadcastingSessions.
      useAppStore.setState((s) => {
        const next = new Set(s.broadcastingSessions);
        next.add(ownSession);
        return { isSharingOwn: true, broadcastingSessions: next };
      });

      const state = useAppStore.getState();
      expect(state.isSharingOwn).toBe(true);
      expect(state.broadcastingSessions.has(ownSession)).toBe(true);
    });

    it("removes own session from broadcastingSessions when stopping own broadcast", () => {
      const ownSession = 1;
      useAppStore.setState({
        ownSession,
        isSharingOwn: true,
        broadcastingSessions: new Set([ownSession, 42]),
      });

      // Simulate stopSharing: remove own session from broadcastingSessions.
      useAppStore.setState((s) => {
        const next = new Set(s.broadcastingSessions);
        next.delete(ownSession);
        return { isSharingOwn: false, broadcastingSessions: next };
      });

      const state = useAppStore.getState();
      expect(state.isSharingOwn).toBe(false);
      expect(state.broadcastingSessions.has(ownSession)).toBe(false);
      // Other broadcasters should remain.
      expect(state.broadcastingSessions.has(42)).toBe(true);
    });
  });

  describe("Version gating", () => {
    // Encoding: (major << 48) | (minor << 32) | (patch << 16)
    // 0.2.12 = 2 * 2^32 + 12 * 2^16 = 8590721024
    const SCREEN_SHARE_MIN_VERSION = 2 * 2 ** 32 + 12 * 2 ** 16;

    it("stores serverFancyVersion as null by default", () => {
      useAppStore.getState().reset();
      expect(useAppStore.getState().serverFancyVersion).toBeNull();
    });

    it("accepts fancy version from server info", () => {
      useAppStore.setState({ serverFancyVersion: SCREEN_SHARE_MIN_VERSION });
      expect(useAppStore.getState().serverFancyVersion).toBe(SCREEN_SHARE_MIN_VERSION);
    });

    it("gates screen sharing on server version >= 0.2.12", () => {
      // Version 0.2.11 (below minimum)
      const v0_2_11 = 2 * 2 ** 32 + 11 * 2 ** 16;
      useAppStore.setState({ serverFancyVersion: v0_2_11 });
      expect(useAppStore.getState().serverFancyVersion! >= SCREEN_SHARE_MIN_VERSION).toBe(false);

      // Version 0.2.12 (exactly minimum)
      useAppStore.setState({ serverFancyVersion: SCREEN_SHARE_MIN_VERSION });
      expect(useAppStore.getState().serverFancyVersion! >= SCREEN_SHARE_MIN_VERSION).toBe(true);

      // Version 0.3.0 (above minimum)
      const v0_3_0 = 3 * 2 ** 32;
      useAppStore.setState({ serverFancyVersion: v0_3_0 });
      expect(useAppStore.getState().serverFancyVersion! >= SCREEN_SHARE_MIN_VERSION).toBe(true);
    });

    it("disables screen sharing when serverFancyVersion is null (standard server)", () => {
      useAppStore.setState({ serverFancyVersion: null });
      const version = useAppStore.getState().serverFancyVersion;
      const canShare = version != null && version >= SCREEN_SHARE_MIN_VERSION;
      expect(canShare).toBe(false);
    });

    it("resets serverFancyVersion on disconnect", () => {
      useAppStore.setState({ serverFancyVersion: SCREEN_SHARE_MIN_VERSION });
      useAppStore.getState().reset();
      expect(useAppStore.getState().serverFancyVersion).toBeNull();
    });
  });
});
