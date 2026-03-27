/**
 * Mumble ACL permission bit definitions.
 *
 * **Single source of truth** — must match `ACL.h` on the server.
 * When a new permission is added server-side, add it here and both the
 * Dev-tab (ChannelInfoPanel) and the ACL editor (AclRulesPanel) pick it up
 * automatically.
 */

export interface PermissionDef {
  /** Bitmask value (must be a power of two). */
  readonly bit: number;
  /** Human-readable label shown in the UI. */
  readonly label: string;
}

/** Complete ordered list of Mumble permission bits. */
export const PERMISSIONS: readonly PermissionDef[] = [
  { bit: 0x01, label: "Write" },
  { bit: 0x02, label: "Traverse" },
  { bit: 0x04, label: "Enter" },
  { bit: 0x08, label: "Speak" },
  { bit: 0x10, label: "Mute/Deafen" },
  { bit: 0x20, label: "Move" },
  { bit: 0x40, label: "Make Channel" },
  { bit: 0x80, label: "Link Channel" },
  { bit: 0x100, label: "Whisper" },
  { bit: 0x200, label: "Text Message" },
  { bit: 0x400, label: "Make Temp Channel" },
  { bit: 0x800, label: "Listen" },
  { bit: 0x1000, label: "Delete Message" },
  { bit: 0x10000, label: "Kick" },
  { bit: 0x20000, label: "Ban" },
  { bit: 0x40000, label: "Register" },
  { bit: 0x80000, label: "Self-Register" },
  { bit: 0x100000, label: "Reset User Content" },
  { bit: 0x200000, label: "Key Owner" },
] as const;

/** Well-known permission bits for use in permission checks. */
export const PERM_KEY_OWNER = 0x200000;
