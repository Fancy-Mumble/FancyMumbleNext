import type { ChannelEntry } from "../../types";

/**
 * Returns the ID of the root channel (parent === null or self-parent), or
 * 0 as a safe Mumble fallback when no channels are loaded yet.
 */
export function rootChannelId(channels: readonly ChannelEntry[]): number {
  const root = channels.find((c) => c.parent_id === null || c.parent_id === c.id);
  return root?.id ?? 0;
}
