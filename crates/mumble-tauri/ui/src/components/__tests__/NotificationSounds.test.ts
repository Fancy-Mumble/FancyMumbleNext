import { describe, it, expect } from "vitest";
import type {
  NotificationSoundSettings,
  NotificationEvent,
  NotificationEventConfig,
} from "../../types";
import {
  SOUND_OPTIONS,
  DEFAULT_NOTIFICATION_SOUNDS,
} from "../../pages/settings/NotificationsPanel";

describe("NotificationSoundSettings types and defaults", () => {
  it("DEFAULT_NOTIFICATION_SOUNDS has all required events", () => {
    const events: NotificationEvent[] = [
      "chatMessage",
      "directMessage",
      "userJoin",
      "userLeave",
      "userJoinChannel",
      "userLeaveChannel",
      "streamStart",
      "voiceActivity",
      "selfMuted",
    ];
    for (const key of events) {
      expect(DEFAULT_NOTIFICATION_SOUNDS.events[key]).toBeDefined();
      const cfg: NotificationEventConfig =
        DEFAULT_NOTIFICATION_SOUNDS.events[key];
      expect(typeof cfg.enabled).toBe("boolean");
      expect(typeof cfg.sound).toBe("string");
      expect(typeof cfg.volume).toBe("number");
      expect(cfg.volume).toBeGreaterThanOrEqual(0);
      expect(cfg.volume).toBeLessThanOrEqual(1);
    }
  });

  it("masterEnabled defaults to true", () => {
    expect(DEFAULT_NOTIFICATION_SOUNDS.masterEnabled).toBe(true);
  });

  it("voiceActivity defaults to disabled", () => {
    expect(DEFAULT_NOTIFICATION_SOUNDS.events.voiceActivity.enabled).toBe(
      false,
    );
  });

  it("SOUND_OPTIONS includes a none entry and at least 3 sound options", () => {
    const none = SOUND_OPTIONS.find((s) => s.id === "none");
    expect(none).toBeDefined();
    expect(none!.url).toBe("");

    const withUrl = SOUND_OPTIONS.filter((s) => s.url !== "");
    expect(withUrl.length).toBeGreaterThanOrEqual(3);
  });

  it("every non-none SOUND_OPTION has a non-empty url", () => {
    for (const opt of SOUND_OPTIONS) {
      if (opt.id === "none") continue;
      expect(opt.url).toBeTruthy();
      expect(opt.label).toBeTruthy();
    }
  });

  it("all default sound IDs reference a valid SOUND_OPTION", () => {
    const validIds = new Set(SOUND_OPTIONS.map((s) => s.id));
    for (const cfg of Object.values(DEFAULT_NOTIFICATION_SOUNDS.events)) {
      expect(validIds.has(cfg.sound)).toBe(true);
    }
  });

  it("patching events preserves unmodified keys", () => {
    const base: NotificationSoundSettings = {
      ...DEFAULT_NOTIFICATION_SOUNDS,
      events: { ...DEFAULT_NOTIFICATION_SOUNDS.events },
    };
    const patched: NotificationSoundSettings = {
      ...base,
      events: {
        ...base.events,
        chatMessage: { ...base.events.chatMessage, enabled: false },
      },
    };
    expect(patched.events.chatMessage.enabled).toBe(false);
    expect(patched.events.directMessage).toEqual(base.events.directMessage);
    expect(patched.events.userJoin).toEqual(base.events.userJoin);
  });
});
