import { DirectMediaAdapter } from "./DirectMediaAdapter";
import { YouTubeAdapter } from "./YouTubeAdapter";
import type { PlayerAdapter, PlayerAdapterArgs } from "./PlayerAdapter";
import type { WatchSourceKind } from "./watchTypes";

/**
 * Construct a [`PlayerAdapter`] for the given source kind.
 *
 * Throws when `kind === "youtube"` but `allowExternal` is false so the
 * caller cannot accidentally bypass the user's
 * `enableExternalEmbeds` preference.
 */
export function createPlayerAdapter(
  kind: WatchSourceKind,
  args: PlayerAdapterArgs,
  allowExternal: boolean,
): PlayerAdapter {
  if (kind === "youtube") {
    if (!allowExternal) {
      throw new Error(
        "YouTube playback is disabled (enable external embeds in settings)",
      );
    }
    return new YouTubeAdapter(args);
  }
  return new DirectMediaAdapter(args);
}
