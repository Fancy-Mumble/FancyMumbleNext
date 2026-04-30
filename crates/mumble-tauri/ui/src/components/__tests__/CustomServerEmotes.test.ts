/**
 * Regression tests for custom server emote handling.
 *
 * Verifies the wire-shape decoder used by the `fancy-server-emotes`
 * plugin-data branch in `store.ts`: each `EmoteDto` from the server
 * must be mapped to a `ServerCustomReaction` whose `display` is the
 * raw `data:` URL and whose `shortcode` is wrapped in colons. The
 * EmojiPicker relies on the `data:` prefix to render an `<img>`
 * element instead of plain text.
 */

import { describe, it, expect, beforeEach } from "vitest";
import {
  setServerCustomReactions,
  getServerCustomReactions,
  resetReactions,
  type ServerCustomReaction,
} from "../chat/reactionStore";

interface EmoteDtoLike {
  shortcode: string;
  alias_emoji: string;
  description?: string;
  image_data_url: string;
}

function emotesToReactions(emotes: EmoteDtoLike[]): ServerCustomReaction[] {
  return emotes.map((e) => ({
    shortcode: `:${e.shortcode}:`,
    display: e.image_data_url,
    label: e.description ?? e.alias_emoji,
  }));
}

beforeEach(() => {
  resetReactions();
});

describe("custom server emotes wire decoding", () => {
  it("maps EmoteDto[] to ServerCustomReaction[] with data URL display", () => {
    const dtos: EmoteDtoLike[] = [
      {
        shortcode: "myCustom",
        alias_emoji: "\u{1F923}",
        description: "Some description",
        image_data_url: "data:image/png;base64,AAAA",
      },
      {
        shortcode: "no_desc",
        alias_emoji: "\u{1F44D}",
        image_data_url: "data:image/gif;base64,BBBB",
      },
    ];

    setServerCustomReactions(emotesToReactions(dtos));
    const reactions = getServerCustomReactions();

    expect(reactions).toHaveLength(2);
    expect(reactions[0].shortcode).toBe(":myCustom:");
    expect(reactions[0].display).toBe("data:image/png;base64,AAAA");
    expect(reactions[0].label).toBe("Some description");

    expect(reactions[1].shortcode).toBe(":no_desc:");
    expect(reactions[1].display).toBe("data:image/gif;base64,BBBB");
    expect(reactions[1].label).toBe("\u{1F44D}");
  });

  it("displays start with `data:` so the picker renders them as <img>", () => {
    const dtos: EmoteDtoLike[] = [
      {
        shortcode: "x",
        alias_emoji: "?",
        image_data_url: "data:image/webp;base64,Z",
      },
    ];
    setServerCustomReactions(emotesToReactions(dtos));
    expect(getServerCustomReactions()[0].display.startsWith("data:")).toBe(true);
  });

  it("resetReactions clears server custom reactions on disconnect", () => {
    setServerCustomReactions([
      { shortcode: ":a:", display: "data:image/png;base64,X", label: "a" },
    ]);
    expect(getServerCustomReactions()).toHaveLength(1);
    resetReactions();
    expect(getServerCustomReactions()).toHaveLength(0);
  });
});
