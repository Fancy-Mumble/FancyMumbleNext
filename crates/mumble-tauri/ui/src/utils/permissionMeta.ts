/**
 * Mumble ACL permission descriptions used in the role editor.
 *
 * Lives next to the bit definitions but kept separate so generic ACL
 * editors can stay terse while the role editor can show long-form text.
 */

export interface PermissionMeta {
  /** Short title shown next to the toggle. */
  readonly title: string;
  /** One-sentence explanation of what granting this permission allows. */
  readonly description: string;
}

export const PERMISSION_META: Record<number, PermissionMeta> = {
  0x01: { title: "Write ACL", description: "Edit the channel ACL and groups." },
  0x02: { title: "Traverse", description: "See the channel and walk into its sub-channels." },
  0x04: { title: "Enter", description: "Join the channel." },
  0x08: { title: "Speak", description: "Transmit voice in the channel." },
  0x10: { title: "Mute / Deafen others", description: "Server-mute or server-deafen other users in this channel." },
  0x20: { title: "Move users", description: "Move other users into or out of this channel." },
  0x40: { title: "Make sub-channel", description: "Create permanent sub-channels." },
  0x80: { title: "Link channels", description: "Link this channel to other channels for cross-talk." },
  0x100: { title: "Whisper", description: "Send targeted whispers into this channel." },
  0x200: { title: "Send text messages", description: "Post text messages in the channel chat." },
  0x400: { title: "Make temporary sub-channel", description: "Create temporary sub-channels that vanish when empty." },
  0x800: { title: "Listen", description: "Receive audio from this channel without joining it." },
  0x1000: { title: "Delete messages", description: "Delete other users' messages in the channel." },
  0x2000: { title: "Subscribe to push", description: "Receive push notifications from the channel." },
  0x10000: { title: "Kick", description: "Kick users from the server." },
  0x20000: { title: "Ban", description: "Ban users from the server." },
  0x40000: { title: "Register users", description: "Register guest users into the server database." },
  0x80000: { title: "Self-register", description: "Users can register themselves." },
  0x100000: { title: "Reset user content", description: "Wipe avatars, comments and other content from a user." },
  0x200000: { title: "Key owner", description: "Marks the user as a key owner for the persistent-chat key share workflow." },
};
