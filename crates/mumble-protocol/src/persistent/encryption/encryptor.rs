//! Symmetric encryption trait and XChaCha20-Poly1305 implementation.

use crate::error::{Error, Result};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::Rng;

use super::{ENCRYPTION_VERSION, KEY_LEN};

/// Block size for ciphertext padding (bytes).
const PADDING_BLOCK: usize = 256;
/// XChaCha20-Poly1305 nonce length.
const NONCE_LEN: usize = 24;

// ---- Trait: Encryptor -----------------------------------------------

/// Trait abstracting symmetric encryption and decryption.
///
/// Enables testing with mock ciphers and future algorithm upgrades.
pub trait Encryptor: Send + Sync {
    /// Encrypt plaintext with the given key and associated data.
    ///
    /// Returns the encrypted payload including version byte, nonce,
    /// and AEAD ciphertext+tag.
    fn encrypt(&self, key: &[u8], plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>>;

    /// Decrypt a payload produced by [`encrypt`](Encryptor::encrypt).
    fn decrypt(&self, key: &[u8], payload: &[u8], aad: &[u8]) -> Result<Vec<u8>>;

    /// The version byte this encryptor produces and accepts.
    fn version(&self) -> u8;
}

// ---- XChaCha20-Poly1305 implementation ------------------------------

/// Production encryptor using XChaCha20-Poly1305.
#[derive(Debug, Clone, Default)]
pub struct XChaChaEncryptor;

impl Encryptor for XChaChaEncryptor {
    fn encrypt(&self, key: &[u8], plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
        if key.len() != KEY_LEN {
            return Err(Error::InvalidState(format!(
                "encryption key must be {KEY_LEN} bytes, got {}",
                key.len()
            )));
        }

        let padded = pad_plaintext(plaintext)?;

        let cipher =
            XChaCha20Poly1305::new_from_slice(key).map_err(|e| Error::Other(e.to_string()))?;

        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut nonce_bytes);
        let nonce = XNonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(
                nonce,
                chacha20poly1305::aead::Payload {
                    msg: &padded,
                    aad,
                },
            )
            .map_err(|e| Error::Other(format!("encryption failed: {e}")))?;

        // Version(1) + Nonce(24) + Ciphertext+Tag
        let mut output = Vec::with_capacity(1 + NONCE_LEN + ciphertext.len());
        output.push(ENCRYPTION_VERSION);
        output.extend_from_slice(&nonce_bytes);
        output.extend_from_slice(&ciphertext);
        Ok(output)
    }

    fn decrypt(&self, key: &[u8], payload: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
        if key.len() != KEY_LEN {
            return Err(Error::InvalidState(format!(
                "decryption key must be {KEY_LEN} bytes, got {}",
                key.len()
            )));
        }

        let min_len = 1 + NONCE_LEN + 16; // version + nonce + tag
        if payload.len() < min_len {
            return Err(Error::InvalidState("payload too short".into()));
        }

        let version = payload[0];
        if version != ENCRYPTION_VERSION {
            return Err(Error::InvalidState(format!(
                "unsupported encryption version: {version:#04x}"
            )));
        }

        let nonce = XNonce::from_slice(&payload[1..1 + NONCE_LEN]);
        let ciphertext = &payload[1 + NONCE_LEN..];

        let cipher =
            XChaCha20Poly1305::new_from_slice(key).map_err(|e| Error::Other(e.to_string()))?;

        let padded = cipher
            .decrypt(
                nonce,
                chacha20poly1305::aead::Payload {
                    msg: ciphertext,
                    aad,
                },
            )
            .map_err(|e| Error::Other(format!("decryption failed: {e}")))?;

        unpad_plaintext(&padded)
    }

    fn version(&self) -> u8 {
        ENCRYPTION_VERSION
    }
}

// ---- Padding --------------------------------------------------------

/// Pad plaintext using randomized block-aligned padding (section 10.1).
///
/// 1. Round up to next multiple of `PADDING_BLOCK` (256 bytes).
/// 2. Add random jitter of 0..255 bytes from CSPRNG.
/// 3. Append `pad_count` as 2-byte big-endian u16.
///
/// `pad_count` includes the 2-byte trailer itself (minimum value: 2).
fn pad_plaintext(plaintext: &[u8]) -> Result<Vec<u8>> {
    let min_padded_len = plaintext.len() + 2; // at least 2 bytes for pad_count
    let blocks_needed = min_padded_len.div_ceil(PADDING_BLOCK);
    let block_aligned = blocks_needed * PADDING_BLOCK;

    let jitter = (rand::rng().next_u32() % PADDING_BLOCK as u32) as usize;
    let padded_length = block_aligned + jitter;
    let pad_count = padded_length - plaintext.len();

    // pad_count must fit in u16
    let pad_count_u16: u16 = pad_count
        .try_into()
        .map_err(|_| Error::InvalidState("message too large for padding".into()))?;

    let mut output = Vec::with_capacity(padded_length);
    output.extend_from_slice(plaintext);
    // Zero padding bytes (pad_count - 2 of them)
    output.resize(padded_length - 2, 0x00);
    // pad_count trailer (2 bytes big-endian)
    output.extend_from_slice(&pad_count_u16.to_be_bytes());
    Ok(output)
}

/// Remove padding, recovering the original plaintext.
fn unpad_plaintext(padded: &[u8]) -> Result<Vec<u8>> {
    if padded.len() < 2 {
        return Err(Error::InvalidState("padded data too short".into()));
    }
    let pad_count =
        u16::from_be_bytes([padded[padded.len() - 2], padded[padded.len() - 1]]) as usize;

    if pad_count < 2 || pad_count > padded.len() {
        return Err(Error::InvalidState(format!(
            "invalid pad_count: {pad_count} for data length {}",
            padded.len()
        )));
    }

    let plaintext_len = padded.len() - pad_count;
    Ok(padded[..plaintext_len].to_vec())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "unwrap is acceptable in test code")]
    use super::*;
    use crate::persistent::encryption::build_aad;

    #[test]
    fn pad_unpad_roundtrip() {
        let plaintext = b"Hello, world!";
        let padded = pad_plaintext(plaintext).unwrap();
        assert!(padded.len() >= plaintext.len() + 2);
        assert!(padded.len().is_multiple_of(PADDING_BLOCK) || padded.len() > PADDING_BLOCK);
        let recovered = unpad_plaintext(&padded).unwrap();
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn pad_minimum_size() {
        let padded = pad_plaintext(b"").unwrap();
        // minimum: 1 block (256) + 0..255 jitter
        assert!(padded.len() >= PADDING_BLOCK);
        let recovered = unpad_plaintext(&padded).unwrap();
        assert!(recovered.is_empty());
    }

    #[test]
    fn unpad_rejects_short_data() {
        assert!(unpad_plaintext(&[0]).is_err());
        assert!(unpad_plaintext(&[]).is_err());
    }

    #[test]
    fn unpad_rejects_invalid_pad_count() {
        // pad_count = 0 (invalid, minimum is 2)
        assert!(unpad_plaintext(&[0x00, 0x00]).is_err());
        // pad_count = 1 (invalid, minimum is 2)
        assert!(unpad_plaintext(&[0x00, 0x01]).is_err());
    }

    #[test]
    fn xchacha_encrypt_decrypt_roundtrip() {
        let enc = XChaChaEncryptor;
        let key = [0x42u8; KEY_LEN];
        let plaintext = b"Secret message for testing";
        let aad = build_aad(1, &[0u8; 16], 1234567890);

        let ciphertext = enc.encrypt(&key, plaintext, &aad).unwrap();
        assert_ne!(&ciphertext[1 + NONCE_LEN..], plaintext);

        let decrypted = enc.decrypt(&key, &ciphertext, &aad).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn xchacha_wrong_key_fails() {
        let enc = XChaChaEncryptor;
        let key = [0x42u8; KEY_LEN];
        let wrong_key = [0x43u8; KEY_LEN];
        let aad = build_aad(1, &[0u8; 16], 0);

        let ciphertext = enc.encrypt(&key, b"test", &aad).unwrap();
        assert!(enc.decrypt(&wrong_key, &ciphertext, &aad).is_err());
    }

    #[test]
    fn xchacha_wrong_aad_fails() {
        let enc = XChaChaEncryptor;
        let key = [0x42u8; KEY_LEN];
        let aad1 = build_aad(1, &[0u8; 16], 0);
        let aad2 = build_aad(2, &[0u8; 16], 0); // different channel

        let ciphertext = enc.encrypt(&key, b"test", &aad1).unwrap();
        assert!(enc.decrypt(&key, &ciphertext, &aad2).is_err());
    }

    #[test]
    fn xchacha_version_byte() {
        let enc = XChaChaEncryptor;
        let key = [0x42u8; KEY_LEN];
        let ciphertext = enc.encrypt(&key, b"test", &[]).unwrap();
        assert_eq!(ciphertext[0], ENCRYPTION_VERSION);
    }

    #[test]
    fn xchacha_rejects_wrong_version() {
        let enc = XChaChaEncryptor;
        let key = [0x42u8; KEY_LEN];
        let mut ciphertext = enc.encrypt(&key, b"test", &[]).unwrap();
        ciphertext[0] = 0xFF; // corrupt version
        assert!(enc.decrypt(&key, &ciphertext, &[]).is_err());
    }
}
