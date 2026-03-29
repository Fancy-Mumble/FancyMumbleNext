//! Safe Rust wrapper around the signal-bridge C FFI.
//!
//! Loads the signal-bridge shared library at runtime via `libloading`
//! and provides a safe, typed API for Sender Key group operations.
//!
//! # Safety
//!
//! This module uses `unsafe` to call into the signal-bridge C FFI.
//! All unsafe blocks are carefully bounded: pointer validity is checked
//! before each call, and output buffers are copied into owned `Vec<u8>`
//! before being returned.
#![allow(unsafe_code, reason = "FFI calls to signal-bridge require unsafe")]

use std::ffi::CString;
use std::path::Path;
use std::sync::Mutex;

use crate::error::{Error, Result};

/// Error codes returned by the signal-bridge FFI functions.
const SIGNAL_OK: i32 = 0;

/// Opaque context type from the C library.
///
/// This is never constructed directly in Rust; the pointer
/// comes from `signal_bridge_create` and is freed by
/// `signal_bridge_destroy`.
#[repr(C)]
struct SignalBridgeCtx {
    _opaque: [u8; 0],
}

/// Type aliases for the FFI function pointers.
type CreateFn =
    unsafe extern "C" fn(our_address: *const std::os::raw::c_char) -> *mut SignalBridgeCtx;
type DestroyFn = unsafe extern "C" fn(ctx: *mut SignalBridgeCtx);
type CreateDistFn = unsafe extern "C" fn(
    ctx: *mut SignalBridgeCtx,
    channel_id: u32,
    out_msg: *mut *mut u8,
    out_len: *mut u32,
) -> i32;
type ProcessDistFn = unsafe extern "C" fn(
    ctx: *mut SignalBridgeCtx,
    sender: *const std::os::raw::c_char,
    channel_id: u32,
    msg: *const u8,
    msg_len: u32,
) -> i32;
type GroupEncryptFn = unsafe extern "C" fn(
    ctx: *mut SignalBridgeCtx,
    channel_id: u32,
    plaintext: *const u8,
    plaintext_len: u32,
    out_ct: *mut *mut u8,
    out_ct_len: *mut u32,
) -> i32;
type GroupDecryptFn = unsafe extern "C" fn(
    ctx: *mut SignalBridgeCtx,
    sender: *const std::os::raw::c_char,
    channel_id: u32,
    ciphertext: *const u8,
    ciphertext_len: u32,
    out_pt: *mut *mut u8,
    out_pt_len: *mut u32,
) -> i32;
type HasKeyFn = unsafe extern "C" fn(
    ctx: *mut SignalBridgeCtx,
    sender: *const std::os::raw::c_char,
    channel_id: u32,
) -> i32;
type RemoveChannelFn = unsafe extern "C" fn(ctx: *mut SignalBridgeCtx, channel_id: u32) -> i32;
type ExportStateFn = unsafe extern "C" fn(
    ctx: *mut SignalBridgeCtx,
    out_data: *mut *mut u8,
    out_len: *mut u32,
) -> i32;
type ImportStateFn = unsafe extern "C" fn(
    ctx: *mut SignalBridgeCtx,
    data: *const u8,
    data_len: u32,
) -> i32;
type FreeBufFn = unsafe extern "C" fn(ptr: *mut u8, len: u32);

/// Holds loaded function pointers from the signal-bridge shared library.
struct BridgeSymbols {
    _lib: libloading::Library,
    create: CreateFn,
    destroy: DestroyFn,
    create_distribution: CreateDistFn,
    process_distribution: ProcessDistFn,
    group_encrypt: GroupEncryptFn,
    group_decrypt: GroupDecryptFn,
    has_key: HasKeyFn,
    remove_channel: RemoveChannelFn,
    export_state: ExportStateFn,
    import_state: ImportStateFn,
    free_buf: FreeBufFn,
}

impl BridgeSymbols {
    fn load(lib_path: &Path) -> Result<Self> {
        // SAFETY: We trust the signal-bridge library at the given path.
        // The library is built from our own signal-bridge crate.
        let lib = unsafe { libloading::Library::new(lib_path) }.map_err(|e| {
            Error::Other(format!(
                "failed to load signal bridge library at {}: {e}",
                lib_path.display()
            ))
        })?;

        // SAFETY: All symbols match the C ABI defined in signal-bridge/src/lib.rs.
        unsafe {
            let create = *lib
                .get::<CreateFn>(b"signal_bridge_create\0")
                .map_err(|e| Error::Other(format!("missing symbol signal_bridge_create: {e}")))?;
            let destroy = *lib
                .get::<DestroyFn>(b"signal_bridge_destroy\0")
                .map_err(|e| Error::Other(format!("missing symbol signal_bridge_destroy: {e}")))?;
            let create_distribution = *lib
                .get::<CreateDistFn>(b"signal_bridge_create_distribution\0")
                .map_err(|e| {
                    Error::Other(format!(
                        "missing symbol signal_bridge_create_distribution: {e}"
                    ))
                })?;
            let process_distribution = *lib
                .get::<ProcessDistFn>(b"signal_bridge_process_distribution\0")
                .map_err(|e| {
                    Error::Other(format!(
                        "missing symbol signal_bridge_process_distribution: {e}"
                    ))
                })?;
            let group_encrypt = *lib
                .get::<GroupEncryptFn>(b"signal_bridge_group_encrypt\0")
                .map_err(|e| {
                    Error::Other(format!("missing symbol signal_bridge_group_encrypt: {e}"))
                })?;
            let group_decrypt = *lib
                .get::<GroupDecryptFn>(b"signal_bridge_group_decrypt\0")
                .map_err(|e| {
                    Error::Other(format!("missing symbol signal_bridge_group_decrypt: {e}"))
                })?;
            let has_key = *lib
                .get::<HasKeyFn>(b"signal_bridge_has_key\0")
                .map_err(|e| Error::Other(format!("missing symbol signal_bridge_has_key: {e}")))?;
            let remove_channel = *lib
                .get::<RemoveChannelFn>(b"signal_bridge_remove_channel\0")
                .map_err(|e| {
                    Error::Other(format!("missing symbol signal_bridge_remove_channel: {e}"))
                })?;
            let export_state = *lib
                .get::<ExportStateFn>(b"signal_bridge_export_state\0")
                .map_err(|e| {
                    Error::Other(format!("missing symbol signal_bridge_export_state: {e}"))
                })?;
            let import_state = *lib
                .get::<ImportStateFn>(b"signal_bridge_import_state\0")
                .map_err(|e| {
                    Error::Other(format!("missing symbol signal_bridge_import_state: {e}"))
                })?;
            let free_buf = *lib
                .get::<FreeBufFn>(b"signal_bridge_free_buf\0")
                .map_err(|e| {
                    Error::Other(format!("missing symbol signal_bridge_free_buf: {e}"))
                })?;

            Ok(Self {
                _lib: lib,
                create,
                destroy,
                create_distribution,
                process_distribution,
                group_encrypt,
                group_decrypt,
                has_key,
                remove_channel,
                export_state,
                import_state,
                free_buf,
            })
        }
    }
}

/// Safe wrapper around the signal-bridge dynamic library.
///
/// Manages the loaded library and the opaque context pointer.
/// All operations are serialized through an internal mutex because
/// the underlying C context is not thread-safe.
pub struct SignalBridge {
    syms: BridgeSymbols,
    ctx: Mutex<*mut SignalBridgeCtx>,
}

impl std::fmt::Debug for SignalBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignalBridge")
            .field("loaded", &true)
            .finish_non_exhaustive()
    }
}

// SAFETY: The context pointer is protected by a Mutex. The library
// symbols are plain function pointers (Copy + Send + Sync).
unsafe impl Send for SignalBridge {}
unsafe impl Sync for SignalBridge {}

impl SignalBridge {
    /// Load the signal-bridge library and create a context.
    ///
    /// `lib_path` points to the shared library file
    /// (`signal_bridge.dll` / `libsignal_bridge.so` / `libsignal_bridge.dylib`).
    /// `our_address` is the TLS cert hash identifying this client.
    pub fn new(lib_path: &Path, our_address: &str) -> Result<Self> {
        let syms = BridgeSymbols::load(lib_path)?;
        let c_addr = CString::new(our_address)
            .map_err(|_| Error::Other("our_address contains NUL byte".into()))?;

        let ctx = unsafe { (syms.create)(c_addr.as_ptr()) };
        if ctx.is_null() {
            return Err(Error::Other(
                "signal_bridge_create returned null".into(),
            ));
        }

        Ok(Self {
            syms,
            ctx: Mutex::new(ctx),
        })
    }

    /// Create a sender key distribution message for a channel.
    ///
    /// Returns the serialized distribution message bytes that must be
    /// sent to all other channel members.
    pub fn create_distribution(&self, channel_id: u32) -> Result<Vec<u8>> {
        let ctx = self.ctx.lock().map_err(|_| Error::Other("mutex poisoned".into()))?;
        let mut out_ptr: *mut u8 = std::ptr::null_mut();
        let mut out_len: u32 = 0;

        let rc = unsafe {
            (self.syms.create_distribution)(*ctx, channel_id, &mut out_ptr, &mut out_len)
        };
        check_rc(rc, "create_distribution")?;
        Ok(self.take_buf(out_ptr, out_len))
    }

    /// Process a peer's sender key distribution message.
    ///
    /// After this, messages from `sender_hash` on `channel_id` can be decrypted.
    pub fn process_distribution(
        &self,
        sender_hash: &str,
        channel_id: u32,
        distribution_bytes: &[u8],
    ) -> Result<()> {
        let ctx = self.ctx.lock().map_err(|_| Error::Other("mutex poisoned".into()))?;
        let c_sender = CString::new(sender_hash)
            .map_err(|_| Error::Other("sender_hash contains NUL byte".into()))?;

        let rc = unsafe {
            (self.syms.process_distribution)(
                *ctx,
                c_sender.as_ptr(),
                channel_id,
                distribution_bytes.as_ptr(),
                distribution_bytes.len() as u32,
            )
        };
        check_rc(rc, "process_distribution")
    }

    /// Encrypt plaintext for a channel using our sender key.
    pub fn group_encrypt(&self, channel_id: u32, plaintext: &[u8]) -> Result<Vec<u8>> {
        let ctx = self.ctx.lock().map_err(|_| Error::Other("mutex poisoned".into()))?;
        let mut out_ptr: *mut u8 = std::ptr::null_mut();
        let mut out_len: u32 = 0;

        let rc = unsafe {
            (self.syms.group_encrypt)(
                *ctx,
                channel_id,
                plaintext.as_ptr(),
                plaintext.len() as u32,
                &mut out_ptr,
                &mut out_len,
            )
        };
        check_rc(rc, "group_encrypt")?;
        Ok(self.take_buf(out_ptr, out_len))
    }

    /// Decrypt ciphertext from a peer on a channel.
    pub fn group_decrypt(
        &self,
        sender_hash: &str,
        channel_id: u32,
        ciphertext: &[u8],
    ) -> Result<Vec<u8>> {
        let ctx = self.ctx.lock().map_err(|_| Error::Other("mutex poisoned".into()))?;
        let c_sender = CString::new(sender_hash)
            .map_err(|_| Error::Other("sender_hash contains NUL byte".into()))?;
        let mut out_ptr: *mut u8 = std::ptr::null_mut();
        let mut out_len: u32 = 0;

        let rc = unsafe {
            (self.syms.group_decrypt)(
                *ctx,
                c_sender.as_ptr(),
                channel_id,
                ciphertext.as_ptr(),
                ciphertext.len() as u32,
                &mut out_ptr,
                &mut out_len,
            )
        };
        check_rc(rc, "group_decrypt")?;
        Ok(self.take_buf(out_ptr, out_len))
    }

    /// Check if we have a sender key for a peer on a channel.
    pub fn has_key(&self, sender_hash: &str, channel_id: u32) -> Result<bool> {
        let ctx = self.ctx.lock().map_err(|_| Error::Other("mutex poisoned".into()))?;
        let c_sender = CString::new(sender_hash)
            .map_err(|_| Error::Other("sender_hash contains NUL byte".into()))?;

        let rc = unsafe { (self.syms.has_key)(*ctx, c_sender.as_ptr(), channel_id) };
        if rc < 0 {
            return Err(Error::Other(format!("has_key failed: error code {rc}")));
        }
        Ok(rc != 0)
    }

    /// Remove all sender key state for a channel.
    pub fn remove_channel(&self, channel_id: u32) -> Result<()> {
        let ctx = self.ctx.lock().map_err(|_| Error::Other("mutex poisoned".into()))?;

        let rc = unsafe { (self.syms.remove_channel)(*ctx, channel_id) };
        check_rc(rc, "remove_channel")
    }

    /// Export internal state for persistence.
    pub fn export_state(&self) -> Result<Vec<u8>> {
        let ctx = self.ctx.lock().map_err(|_| Error::Other("mutex poisoned".into()))?;
        let mut out_ptr: *mut u8 = std::ptr::null_mut();
        let mut out_len: u32 = 0;

        let rc =
            unsafe { (self.syms.export_state)(*ctx, &mut out_ptr, &mut out_len) };
        check_rc(rc, "export_state")?;
        Ok(self.take_buf(out_ptr, out_len))
    }

    /// Import previously exported state.
    pub fn import_state(&self, data: &[u8]) -> Result<()> {
        let ctx = self.ctx.lock().map_err(|_| Error::Other("mutex poisoned".into()))?;

        let rc = unsafe {
            (self.syms.import_state)(*ctx, data.as_ptr(), data.len() as u32)
        };
        check_rc(rc, "import_state")
    }

    /// Take ownership of a buffer allocated by the bridge library.
    ///
    /// Copies the data into a Rust Vec and frees the original buffer.
    fn take_buf(&self, ptr: *mut u8, len: u32) -> Vec<u8> {
        if ptr.is_null() || len == 0 {
            return Vec::new();
        }
        let data = unsafe { std::slice::from_raw_parts(ptr, len as usize) }.to_vec();
        unsafe { (self.syms.free_buf)(ptr, len) };
        data
    }
}

impl Drop for SignalBridge {
    fn drop(&mut self) {
        if let Ok(ctx) = self.ctx.lock() {
            if !(*ctx).is_null() {
                unsafe { (self.syms.destroy)(*ctx) };
            }
        }
    }
}

/// Convert a non-zero FFI return code to an error.
fn check_rc(rc: i32, operation: &str) -> Result<()> {
    if rc == SIGNAL_OK {
        return Ok(());
    }
    let desc = match rc {
        -1 => "null pointer",
        -2 => "invalid UTF-8",
        -3 => "protocol error",
        -4 => "no key found",
        -5 => "serialization error",
        _ => "unknown error",
    };
    Err(Error::Other(format!(
        "signal bridge {operation} failed: {desc} (code {rc})"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_rc_ok() {
        assert!(check_rc(0, "test").is_ok());
    }

    #[test]
    fn check_rc_error() {
        assert!(check_rc(-3, "test").is_err());
    }
}
