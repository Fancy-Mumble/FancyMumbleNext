// **Single source of truth** for Mumble ACL permission bit definitions.
//
// Mirrors `enum ChanACL::Perm` in `mumble-server/src/ACL.h` (the canonical
// definition shipped by the Mumble C++ server).  Keep this file in sync
// with the server enum whenever a new permission is added; the frontend
// TypeScript table is regenerated from this file by
// `crates/mumble-tauri/build.rs` so the React UI never falls behind.
//
// This module is intentionally dependency-free and must remain valid as
// a stand-alone `include!()` target so build scripts can pull in the
// constants without compiling the rest of `fancy-utils`.  Module-level
// rustdoc lives on the `pub mod permissions;` declaration in `lib.rs`.

/// One permission flag entry.  Used to drive both Rust and TypeScript code
/// generation, so every field has to remain `&'static`.
#[derive(Debug, Clone, Copy)]
pub struct PermissionEntry {
    /// Bitmask value (always a single bit).
    pub bit: u32,
    /// SCREAMING_SNAKE identifier suffix.  The frontend gets a constant
    /// named `PERM_${ident}` (e.g. `PERM_WRITE`) and the Rust side gets
    /// `permissions::${ident}` (e.g. `permissions::WRITE`).
    pub ident: &'static str,
    /// Human-readable label shown in the UI.
    pub label: &'static str,
    /// `true` for permissions that only make sense on the root channel
    /// (Kick, Ban, Register, ...).  Used by the frontend to filter the
    /// per-channel ACL editor.
    pub root_only: bool,
}

// -- Named constants -------------------------------------------------------
//
// These mirror Mumble's `ChanACL::Perm` enum value-for-value.  Any new
// addition must also be appended to the `ENTRIES` table below.

/// Write access; on the root channel this is the canonical "is admin" flag.
pub const WRITE: u32 = 0x1;
/// May traverse the channel tree through this channel.
pub const TRAVERSE: u32 = 0x2;
/// May enter (join) the channel.
pub const ENTER: u32 = 0x4;
/// May transmit audio in the channel.
pub const SPEAK: u32 = 0x8;
/// May server-mute or server-deafen other users in the channel.
pub const MUTE_DEAFEN: u32 = 0x10;
/// May move users into or out of the channel.
pub const MOVE: u32 = 0x20;
/// May create sub-channels.
pub const MAKE_CHANNEL: u32 = 0x40;
/// May link channels together.
pub const LINK_CHANNEL: u32 = 0x80;
/// May whisper into the channel.
pub const WHISPER: u32 = 0x100;
/// May post text messages in the channel.
pub const TEXT_MESSAGE: u32 = 0x200;
/// May create temporary sub-channels.
pub const MAKE_TEMP_CHANNEL: u32 = 0x400;
/// May listen to (subscribe to) the channel.
pub const LISTEN: u32 = 0x800;
/// May delete messages in the channel.
pub const DELETE_MESSAGE: u32 = 0x1000;
/// May subscribe to push notifications for the channel.
pub const SUBSCRIBE_PUSH: u32 = 0x2000;
/// May upload and share files (any access mode).
pub const SHARE_FILES: u32 = 0x4000;
/// May share files via publicly accessible links (`public` / `password`).
pub const SHARE_FILES_PUBLIC: u32 = 0x8000;
/// Root-only: may kick users.
pub const KICK: u32 = 0x10000;
/// Root-only: may ban users.
pub const BAN: u32 = 0x20000;
/// Root-only: may register users.
pub const REGISTER: u32 = 0x40000;
/// Root-only: may self-register.
pub const SELF_REGISTER: u32 = 0x80000;
/// Root-only: may reset other users' customisation content.
pub const RESET_USER_CONTENT: u32 = 0x100000;
/// Root-only: may own/manage cryptographic keys.
pub const KEY_OWNER: u32 = 0x200000;
/// Root-only: may add and remove custom server emotes.
pub const MANAGE_EMOTES: u32 = 0x400000;

/// Every permission entry, in display order.  This is the data that the
/// build script walks to generate the TypeScript table.
pub const ENTRIES: &[PermissionEntry] = &[
    PermissionEntry { bit: WRITE,              ident: "WRITE",              label: "Write",               root_only: false },
    PermissionEntry { bit: TRAVERSE,           ident: "TRAVERSE",           label: "Traverse",            root_only: false },
    PermissionEntry { bit: ENTER,              ident: "ENTER",              label: "Enter",               root_only: false },
    PermissionEntry { bit: SPEAK,              ident: "SPEAK",              label: "Speak",               root_only: false },
    PermissionEntry { bit: MUTE_DEAFEN,        ident: "MUTE_DEAFEN",        label: "Mute/Deafen",         root_only: false },
    PermissionEntry { bit: MOVE,               ident: "MOVE",               label: "Move",                root_only: false },
    PermissionEntry { bit: MAKE_CHANNEL,       ident: "MAKE_CHANNEL",       label: "Make Channel",        root_only: false },
    PermissionEntry { bit: LINK_CHANNEL,       ident: "LINK_CHANNEL",       label: "Link Channel",        root_only: false },
    PermissionEntry { bit: WHISPER,            ident: "WHISPER",            label: "Whisper",             root_only: false },
    PermissionEntry { bit: TEXT_MESSAGE,       ident: "TEXT_MESSAGE",       label: "Text Message",        root_only: false },
    PermissionEntry { bit: MAKE_TEMP_CHANNEL,  ident: "MAKE_TEMP_CHANNEL",  label: "Make Temp Channel",   root_only: false },
    PermissionEntry { bit: LISTEN,             ident: "LISTEN",             label: "Listen",              root_only: false },
    PermissionEntry { bit: DELETE_MESSAGE,     ident: "DELETE_MESSAGE",     label: "Delete Message",      root_only: false },
    PermissionEntry { bit: SUBSCRIBE_PUSH,     ident: "SUBSCRIBE_PUSH",     label: "Subscribe Push",      root_only: false },
    PermissionEntry { bit: SHARE_FILES,        ident: "SHARE_FILES",        label: "Share Files",         root_only: false },
    PermissionEntry { bit: SHARE_FILES_PUBLIC, ident: "SHARE_FILES_PUBLIC", label: "Share Files (Public)", root_only: false },
    PermissionEntry { bit: KICK,               ident: "KICK",               label: "Kick",                root_only: true  },
    PermissionEntry { bit: BAN,                ident: "BAN",                label: "Ban",                 root_only: true  },
    PermissionEntry { bit: REGISTER,           ident: "REGISTER",           label: "Register",            root_only: true  },
    PermissionEntry { bit: SELF_REGISTER,      ident: "SELF_REGISTER",      label: "Self-Register",       root_only: true  },
    PermissionEntry { bit: RESET_USER_CONTENT, ident: "RESET_USER_CONTENT", label: "Reset User Content",  root_only: true  },
    PermissionEntry { bit: KEY_OWNER,          ident: "KEY_OWNER",          label: "Key Owner",           root_only: true  },
    PermissionEntry { bit: MANAGE_EMOTES,      ident: "MANAGE_EMOTES",      label: "Manage Emotes",       root_only: true  },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_entry_bit_is_a_single_bit() {
        for e in ENTRIES {
            assert!(
                e.bit.is_power_of_two(),
                "{} (0x{:X}) is not a single bit",
                e.ident,
                e.bit
            );
        }
    }

    #[test]
    fn entry_bits_are_unique() {
        let mut seen = 0u32;
        for e in ENTRIES {
            assert_eq!(
                seen & e.bit,
                0,
                "{} (0x{:X}) collides with an earlier entry",
                e.ident,
                e.bit
            );
            seen |= e.bit;
        }
    }

    #[test]
    fn idents_are_unique() {
        let mut seen: Vec<&'static str> = Vec::new();
        for e in ENTRIES {
            assert!(!seen.contains(&e.ident), "duplicate ident {}", e.ident);
            seen.push(e.ident);
        }
    }
}
