//! Signal Protocol bridge - C FFI layer for Sender Key group encryption.
//!
//! This crate wraps `libsignal-protocol`'s Sender Key operations behind
//! a stable C ABI so that MIT-licensed consumers can load it at runtime
//! via `libloading` without AGPL contamination.
//!
//! # Thread safety
//!
//! Each `SignalBridgeCtx` is **not** thread-safe. Callers must ensure
//! exclusive access (or wrap in their own mutex).

mod context;

use std::ffi::CStr;
use std::os::raw::c_char;
use std::ptr;
use std::slice;

use context::SignalBridgeCtx;

/// Result codes returned by all bridge functions.
pub const SIGNAL_OK: i32 = 0;
pub const SIGNAL_ERR_NULL_PTR: i32 = -1;
pub const SIGNAL_ERR_INVALID_UTF8: i32 = -2;
pub const SIGNAL_ERR_PROTOCOL: i32 = -3;
pub const SIGNAL_ERR_NO_KEY: i32 = -4;
pub const SIGNAL_ERR_SERIALIZE: i32 = -5;

// ---------------------------------------------------------------------------
// Context lifecycle
// ---------------------------------------------------------------------------

/// Create a new bridge context.
///
/// `our_address` is a NUL-terminated C string identifying this client
/// (typically the TLS cert hash). The caller owns the returned pointer
/// and must free it with [`signal_bridge_destroy`].
///
/// # Safety
///
/// `our_address` must be a valid, NUL-terminated C string pointer
/// (or null, in which case null is returned).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn signal_bridge_create(our_address: *const c_char) -> *mut SignalBridgeCtx {
    if our_address.is_null() {
        return ptr::null_mut();
    }
    let addr = match unsafe { CStr::from_ptr(our_address) }.to_str() {
        Ok(s) => s.to_owned(),
        Err(_) => return ptr::null_mut(),
    };
    Box::into_raw(Box::new(SignalBridgeCtx::new(addr)))
}

/// Destroy a context previously created by [`signal_bridge_create`].
///
/// # Safety
///
/// `ctx` must be a valid pointer returned by [`signal_bridge_create`],
/// or null (which is a no-op).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn signal_bridge_destroy(ctx: *mut SignalBridgeCtx) {
    if !ctx.is_null() {
        drop(unsafe { Box::from_raw(ctx) });
    }
}

// ---------------------------------------------------------------------------
// Sender Key operations
// ---------------------------------------------------------------------------

/// Create a Sender Key distribution message for a channel.
///
/// The distribution message bytes are written to `*out_msg` with length
/// `*out_len`. The caller must free the buffer with [`signal_bridge_free_buf`].
///
/// `channel_id` is used to derive a deterministic distribution UUID.
#[unsafe(no_mangle)]
pub extern "C" fn signal_bridge_create_distribution(
    ctx: *mut SignalBridgeCtx,
    channel_id: u32,
    out_msg: *mut *mut u8,
    out_len: *mut u32,
) -> i32 {
    let (ctx, out_msg, out_len) = match validate_ctx_and_out(ctx, out_msg, out_len) {
        Some(v) => v,
        None => return SIGNAL_ERR_NULL_PTR,
    };
    match ctx.create_distribution(channel_id) {
        Ok(bytes) => write_output(bytes, out_msg, out_len),
        Err(_) => SIGNAL_ERR_PROTOCOL,
    }
}

/// Process a peer's Sender Key distribution message for a channel.
///
/// After this call, messages from `sender_address` on `channel_id` can
/// be decrypted.
#[unsafe(no_mangle)]
pub extern "C" fn signal_bridge_process_distribution(
    ctx: *mut SignalBridgeCtx,
    sender_address: *const c_char,
    channel_id: u32,
    msg: *const u8,
    msg_len: u32,
) -> i32 {
    let ctx = match validate_ctx(ctx) {
        Some(c) => c,
        None => return SIGNAL_ERR_NULL_PTR,
    };
    let sender = match cstr_to_string(sender_address) {
        Some(s) => s,
        None => return SIGNAL_ERR_INVALID_UTF8,
    };
    let data = match safe_slice(msg, msg_len) {
        Some(s) => s,
        None => return SIGNAL_ERR_NULL_PTR,
    };
    match ctx.process_distribution(&sender, channel_id, data) {
        Ok(()) => SIGNAL_OK,
        Err(_) => SIGNAL_ERR_PROTOCOL,
    }
}

/// Encrypt a plaintext message for a channel using our Sender Key.
///
/// Output buffer written to `*out_ct` / `*out_ct_len`.
/// Free with [`signal_bridge_free_buf`].
#[unsafe(no_mangle)]
pub extern "C" fn signal_bridge_group_encrypt(
    ctx: *mut SignalBridgeCtx,
    channel_id: u32,
    plaintext: *const u8,
    plaintext_len: u32,
    out_ct: *mut *mut u8,
    out_ct_len: *mut u32,
) -> i32 {
    let (ctx, out_ct, out_ct_len) = match validate_ctx_and_out(ctx, out_ct, out_ct_len) {
        Some(v) => v,
        None => return SIGNAL_ERR_NULL_PTR,
    };
    let pt = match safe_slice(plaintext, plaintext_len) {
        Some(s) => s,
        None => return SIGNAL_ERR_NULL_PTR,
    };
    match ctx.group_encrypt(channel_id, pt) {
        Ok(bytes) => write_output(bytes, out_ct, out_ct_len),
        Err(_) => SIGNAL_ERR_PROTOCOL,
    }
}

/// Decrypt a ciphertext message from a peer on a channel.
///
/// Output buffer written to `*out_pt` / `*out_pt_len`.
/// Free with [`signal_bridge_free_buf`].
#[unsafe(no_mangle)]
pub extern "C" fn signal_bridge_group_decrypt(
    ctx: *mut SignalBridgeCtx,
    sender_address: *const c_char,
    channel_id: u32,
    ciphertext: *const u8,
    ciphertext_len: u32,
    out_pt: *mut *mut u8,
    out_pt_len: *mut u32,
) -> i32 {
    let (ctx, out_pt, out_pt_len) = match validate_ctx_and_out(ctx, out_pt, out_pt_len) {
        Some(v) => v,
        None => return SIGNAL_ERR_NULL_PTR,
    };
    let sender = match cstr_to_string(sender_address) {
        Some(s) => s,
        None => return SIGNAL_ERR_INVALID_UTF8,
    };
    let ct = match safe_slice(ciphertext, ciphertext_len) {
        Some(s) => s,
        None => return SIGNAL_ERR_NULL_PTR,
    };
    match ctx.group_decrypt(&sender, channel_id, ct) {
        Ok(bytes) => write_output(bytes, out_pt, out_pt_len),
        Err(_) => SIGNAL_ERR_PROTOCOL,
    }
}

/// Check whether we have a sender key for a given peer on a channel.
///
/// Returns 1 if the key exists, 0 if not, or a negative error code.
#[unsafe(no_mangle)]
pub extern "C" fn signal_bridge_has_key(
    ctx: *mut SignalBridgeCtx,
    sender_address: *const c_char,
    channel_id: u32,
) -> i32 {
    let ctx = match validate_ctx(ctx) {
        Some(c) => c,
        None => return SIGNAL_ERR_NULL_PTR,
    };
    let sender = match cstr_to_string(sender_address) {
        Some(s) => s,
        None => return SIGNAL_ERR_INVALID_UTF8,
    };
    i32::from(ctx.has_key(&sender, channel_id))
}

/// Remove all sender key state for a channel.
#[unsafe(no_mangle)]
pub extern "C" fn signal_bridge_remove_channel(ctx: *mut SignalBridgeCtx, channel_id: u32) -> i32 {
    let ctx = match validate_ctx(ctx) {
        Some(c) => c,
        None => return SIGNAL_ERR_NULL_PTR,
    };
    ctx.remove_channel(channel_id);
    SIGNAL_OK
}

// ---------------------------------------------------------------------------
// State persistence
// ---------------------------------------------------------------------------

/// Export all internal state as a JSON blob for persistence.
///
/// Output buffer written to `*out_data` / `*out_len`.
/// Free with [`signal_bridge_free_buf`].
#[unsafe(no_mangle)]
pub extern "C" fn signal_bridge_export_state(
    ctx: *mut SignalBridgeCtx,
    out_data: *mut *mut u8,
    out_len: *mut u32,
) -> i32 {
    let (ctx, out_data, out_len) = match validate_ctx_and_out(ctx, out_data, out_len) {
        Some(v) => v,
        None => return SIGNAL_ERR_NULL_PTR,
    };
    match ctx.export_state() {
        Ok(bytes) => write_output(bytes, out_data, out_len),
        Err(_) => SIGNAL_ERR_SERIALIZE,
    }
}

/// Import state from a JSON blob previously exported by
/// [`signal_bridge_export_state`].
#[unsafe(no_mangle)]
pub extern "C" fn signal_bridge_import_state(
    ctx: *mut SignalBridgeCtx,
    data: *const u8,
    data_len: u32,
) -> i32 {
    let ctx = match validate_ctx(ctx) {
        Some(c) => c,
        None => return SIGNAL_ERR_NULL_PTR,
    };
    let blob = match safe_slice(data, data_len) {
        Some(s) => s,
        None => return SIGNAL_ERR_NULL_PTR,
    };
    match ctx.import_state(blob) {
        Ok(()) => SIGNAL_OK,
        Err(_) => SIGNAL_ERR_SERIALIZE,
    }
}

/// Free a buffer previously allocated by the bridge.
///
/// # Safety
///
/// `ptr` must be a buffer returned by a bridge function (e.g.
/// `signal_bridge_export_state`) with matching `len`, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn signal_bridge_free_buf(ptr: *mut u8, len: u32) {
    if !ptr.is_null() && len > 0 {
        drop(unsafe { Vec::from_raw_parts(ptr, len as usize, len as usize) });
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn validate_ctx(ctx: *mut SignalBridgeCtx) -> Option<&'static mut SignalBridgeCtx> {
    if ctx.is_null() {
        return None;
    }
    Some(unsafe { &mut *ctx })
}

fn validate_ctx_and_out(
    ctx: *mut SignalBridgeCtx,
    out: *mut *mut u8,
    out_len: *mut u32,
) -> Option<(&'static mut SignalBridgeCtx, &'static mut *mut u8, &'static mut u32)> {
    if ctx.is_null() || out.is_null() || out_len.is_null() {
        return None;
    }
    Some(unsafe { (&mut *ctx, &mut *out, &mut *out_len) })
}

fn cstr_to_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(ptr) }.to_str().ok().map(String::from)
}

fn safe_slice(ptr: *const u8, len: u32) -> Option<&'static [u8]> {
    if ptr.is_null() || len == 0 {
        return None;
    }
    Some(unsafe { slice::from_raw_parts(ptr, len as usize) })
}

/// Write a `Vec<u8>` through FFI output pointers and return `SIGNAL_OK`.
fn write_output(data: Vec<u8>, out: &mut *mut u8, out_len: &mut u32) -> i32 {
    let len = data.len() as u32;
    let mut boxed = data.into_boxed_slice();
    *out = boxed.as_mut_ptr();
    *out_len = len;
    std::mem::forget(boxed);
    SIGNAL_OK
}
