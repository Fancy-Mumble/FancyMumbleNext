//! OCB2-AES128 encryption for Mumble UDP audio packets.
//!
//! Mumble uses a non-standard OCB2 (Offset Codebook Mode 2) with AES-128
//! for encrypting UDP datagrams. Keys are exchanged via the TCP `CryptSetup`
//! message. Each encrypted packet has a 4-byte header:
//!
//! ```text
//! [iv_byte_0] [tag_0] [tag_1] [tag_2] [ciphertext...]
//! ```
//!
//! Only the first byte of the 16-byte nonce (IV) is sent in the header.
//! The receiver reconstructs the full nonce from its internal counter state.
//! The 3-byte tag is the first 3 bytes of the full 16-byte OCB2 auth tag.

use std::sync::{Arc, Mutex};

use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use aes::Aes128;

use super::udp::CryptState;
use crate::error::{Error, Result};

const AES_BLOCK_SIZE: usize = 16;

/// Packet statistics for monitoring link quality.
#[derive(Debug, Clone, Default)]
pub struct PacketStats {
    /// Successfully decrypted and authenticated packets.
    pub good: u32,
    /// Out-of-order packets that still decrypted successfully.
    pub late: u32,
    /// Packets assumed lost (sequence gap).
    pub lost: u32,
    /// Nonce resync events.
    pub resync: u32,
}

/// Thread-safe handle to shared packet statistics.
///
/// The UDP reader task updates the stats on every decrypt, and the
/// ping loop reads a snapshot to include in outgoing Ping messages.
pub type SharedPacketStats = Arc<Mutex<PacketStats>>;

/// OCB2-AES128 crypt state compatible with Mumble servers.
///
/// Initialised from the `CryptSetup` message fields: `key` (16 bytes),
/// `client_nonce` (16 bytes, used for encrypt), `server_nonce` (16 bytes,
/// used for decrypt).
pub struct Ocb2CryptState {
    /// AES-128 cipher for encryption operations.
    cipher: Aes128,
    /// 16-byte raw key (kept for debug/resync).
    raw_key: [u8; AES_BLOCK_SIZE],
    /// Nonce for outbound (encrypt) direction.
    encrypt_iv: [u8; AES_BLOCK_SIZE],
    /// Nonce for inbound (decrypt) direction.
    decrypt_iv: [u8; AES_BLOCK_SIZE],
    /// Replay detection: stores `decrypt_iv[1]` for each `decrypt_iv[0]` value.
    decrypt_history: [u8; 256],
    /// Link quality counters.
    pub stats: PacketStats,
    /// Whether keys have been set.
    initialized: bool,
}

impl std::fmt::Debug for Ocb2CryptState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ocb2CryptState")
            .field("initialized", &self.initialized)
            .field("stats", &self.stats)
            .finish_non_exhaustive()
    }
}

impl Default for Ocb2CryptState {
    fn default() -> Self {
        Self::new()
    }
}

impl Ocb2CryptState {
    /// Create a new uninitialized crypt state.
    pub fn new() -> Self {
        Self {
            cipher: Aes128::new(&[0u8; AES_BLOCK_SIZE].into()),
            raw_key: [0u8; AES_BLOCK_SIZE],
            encrypt_iv: [0u8; AES_BLOCK_SIZE],
            decrypt_iv: [0u8; AES_BLOCK_SIZE],
            decrypt_history: [0u8; 256],
            stats: PacketStats::default(),
            initialized: false,
        }
    }

    /// Initialize from a `CryptSetup` message.
    ///
    /// - `key`: 16-byte AES key
    /// - `client_nonce`: 16-byte IV for encrypting (client->server)
    /// - `server_nonce`: 16-byte IV for decrypting (server->client)
    pub fn set_key(&mut self, key: &[u8], client_nonce: &[u8], server_nonce: &[u8]) -> Result<()> {
        if key.len() != AES_BLOCK_SIZE {
            return Err(Error::InvalidState(format!(
                "OCB2 key must be {} bytes, got {}",
                AES_BLOCK_SIZE,
                key.len()
            )));
        }
        if client_nonce.len() != AES_BLOCK_SIZE || server_nonce.len() != AES_BLOCK_SIZE {
            return Err(Error::InvalidState(
                "OCB2 nonces must be 16 bytes".into(),
            ));
        }

        self.raw_key.copy_from_slice(key);
        self.encrypt_iv.copy_from_slice(client_nonce);
        self.decrypt_iv.copy_from_slice(server_nonce);
        self.cipher = Aes128::new(key.into());
        // Initialize history so it cannot match decrypt_iv[1] on the first packet.
        // Use the bitwise complement of server_nonce[1] so the first check always passes.
        self.decrypt_history = [!server_nonce[1]; 256];
        self.initialized = true;
        Ok(())
    }

    /// Update the server nonce (used when the server sends a resync `CryptSetup`
    /// with only `server_nonce`).
    pub fn set_decrypt_iv(&mut self, server_nonce: &[u8]) {
        if server_nonce.len() == AES_BLOCK_SIZE {
            self.decrypt_iv.copy_from_slice(server_nonce);
        }
    }

    /// Get the current encrypt nonce (`client_nonce`) for resync requests.
    pub fn encrypt_iv(&self) -> &[u8; AES_BLOCK_SIZE] {
        &self.encrypt_iv
    }
}

impl CryptState for Ocb2CryptState {
    fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>> {
        if !self.initialized {
            return Err(Error::InvalidState("OCB2 not initialized".into()));
        }

        let mut tag = [0u8; AES_BLOCK_SIZE];

        // Increment encrypt_iv by 1 (little-endian carry propagation)
        for byte in &mut self.encrypt_iv {
            *byte = byte.wrapping_add(1);
            if *byte != 0 {
                break;
            }
        }

        let mut ciphertext = vec![0u8; plaintext.len()];
        ocb_encrypt(
            &self.cipher,
            plaintext,
            &mut ciphertext,
            &self.encrypt_iv,
            &mut tag,
        );

        // Build 4-byte header + ciphertext
        let mut output = Vec::with_capacity(4 + ciphertext.len());
        output.push(self.encrypt_iv[0]);
        output.push(tag[0]);
        output.push(tag[1]);
        output.push(tag[2]);
        output.extend_from_slice(&ciphertext);
        Ok(output)
    }

    fn decrypt(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        if !self.initialized {
            return Err(Error::InvalidState("OCB2 not initialized".into()));
        }

        if data.len() < 4 {
            return Err(Error::InvalidState("UDP packet too short for OCB2 header".into()));
        }

        let iv_byte = data[0];
        let tag_bytes = &data[1..4];
        let ciphertext = &data[4..];

        // Save IV state for rollback on auth failure
        let save_iv = self.decrypt_iv;
        let mut late: i32 = 0;
        let mut lost: i32 = 0;
        let mut restore = false;

        // Determine nonce state from the received IV byte
        if self.decrypt_iv[0].wrapping_add(1) == iv_byte {
            // In-sequence: next expected packet
            if iv_byte > self.decrypt_iv[0] {
                self.decrypt_iv[0] = iv_byte;
            } else if iv_byte < self.decrypt_iv[0] {
                // Overflow from 255 -> 0: carry into higher bytes
                self.decrypt_iv[0] = iv_byte;
                carry_iv_bytes(&mut self.decrypt_iv);
            } else {
                // iv_byte == decrypt_iv[0] should not happen for +1 case
                self.decrypt_iv[0] = iv_byte;
            }
        } else {
            // Out-of-sequence: compute signed distance
            let mut diff = iv_byte as i32 - self.decrypt_iv[0] as i32;
            if diff > 128 {
                diff -= 256;
            } else if diff < -128 {
                diff += 256;
            }

            if (iv_byte < self.decrypt_iv[0]) && (diff > -30) && (diff < 0) {
                // Late packet (arrived after we moved past it)
                late = 1;
                lost = -1;
                self.decrypt_iv[0] = iv_byte;
                restore = true;
            } else if (iv_byte > self.decrypt_iv[0]) && (diff > 0) && (diff < 30) {
                // Skipped some packets
                lost = iv_byte as i32 - self.decrypt_iv[0] as i32 - 1;
                self.decrypt_iv[0] = iv_byte;
            } else {
                // Major desync - way out of range
                self.decrypt_iv = save_iv;
                return Err(Error::InvalidState("OCB2 nonce out of range".into()));
            }
        }

        // Replay detection
        if self.decrypt_history[self.decrypt_iv[0] as usize] == self.decrypt_iv[1] {
            self.decrypt_iv = save_iv;
            return Err(Error::InvalidState("OCB2 replay detected".into()));
        }

        // Decrypt and verify tag
        let mut tag = [0u8; AES_BLOCK_SIZE];
        let mut plaintext = vec![0u8; ciphertext.len()];
        ocb_decrypt(
            &self.cipher,
            ciphertext,
            &mut plaintext,
            &self.decrypt_iv,
            &mut tag,
        );

        // Verify the 3-byte truncated tag
        if tag[0] != tag_bytes[0] || tag[1] != tag_bytes[1] || tag[2] != tag_bytes[2] {
            self.decrypt_iv = save_iv;
            return Err(Error::InvalidState("OCB2 authentication failed".into()));
        }

        // Tag verified - update history and stats
        self.decrypt_history[self.decrypt_iv[0] as usize] = self.decrypt_iv[1];

        if restore {
            // Late packet: restore IV to where it was (don't advance)
            self.decrypt_iv = save_iv;
        }

        // Update stats
        self.stats.good += 1;
        if late > 0 {
            self.stats.late += late as u32;
        }
        if lost > 0 {
            self.stats.lost += lost as u32;
        } else if (self.stats.late as i32) > lost.abs() {
            self.stats.late -= lost.unsigned_abs();
        }

        Ok(plaintext)
    }

    fn is_initialized(&self) -> bool {
        self.initialized
    }
}

// -- OCB2 core ------------------------------------------------------
// GF(2^128) doubling: multiply by x in the polynomial field
// with reduction polynomial x^128 + x^7 + x^2 + x + 1 (0x87).
// Operates on a 16-byte block treated as a big-endian 128-bit integer.

fn s2(block: &mut [u8; AES_BLOCK_SIZE]) {
    let carry = block[0] >> 7; // MSB of byte 0 is the GF carry
    for i in 0..AES_BLOCK_SIZE - 1 {
        block[i] = (block[i] << 1) | (block[i + 1] >> 7);
    }
    block[AES_BLOCK_SIZE - 1] = (block[AES_BLOCK_SIZE - 1] << 1) ^ (carry * 0x87);
}

fn s3(block: &mut [u8; AES_BLOCK_SIZE]) {
    let carry = block[0] >> 7;
    for i in 0..AES_BLOCK_SIZE - 1 {
        block[i] ^= (block[i] << 1) | (block[i + 1] >> 7);
    }
    block[AES_BLOCK_SIZE - 1] ^= (block[AES_BLOCK_SIZE - 1] << 1) ^ (carry * 0x87);
}

fn xor_blocks(dst: &mut [u8; AES_BLOCK_SIZE], a: &[u8; AES_BLOCK_SIZE], b: &[u8; AES_BLOCK_SIZE]) {
    for i in 0..AES_BLOCK_SIZE {
        dst[i] = a[i] ^ b[i];
    }
}

fn xor_in_place(a: &mut [u8; AES_BLOCK_SIZE], b: &[u8; AES_BLOCK_SIZE]) {
    for i in 0..AES_BLOCK_SIZE {
        a[i] ^= b[i];
    }
}

fn aes_encrypt(cipher: &Aes128, input: &[u8; AES_BLOCK_SIZE]) -> [u8; AES_BLOCK_SIZE] {
    let mut block = aes::Block::from(*input);
    cipher.encrypt_block(&mut block);
    block.into()
}

fn aes_decrypt(cipher: &Aes128, input: &[u8; AES_BLOCK_SIZE]) -> [u8; AES_BLOCK_SIZE] {
    let mut block = aes::Block::from(*input);
    cipher.decrypt_block(&mut block);
    block.into()
}

/// OCB2 encrypt: produces ciphertext and a 16-byte tag.
fn ocb_encrypt(
    cipher: &Aes128,
    plain: &[u8],
    encrypted: &mut [u8],
    nonce: &[u8; AES_BLOCK_SIZE],
    tag: &mut [u8; AES_BLOCK_SIZE],
) {
    let mut delta = aes_encrypt(cipher, nonce);
    let mut checksum = [0u8; AES_BLOCK_SIZE];

    let mut offset = 0;
    let mut remaining = plain.len();

    // Process full 16-byte blocks
    while remaining > AES_BLOCK_SIZE {
        s2(&mut delta);

        #[allow(clippy::expect_used, reason = "slice length is guaranteed by while-loop guard")]
        let plain_block: [u8; AES_BLOCK_SIZE] = plain[offset..offset + AES_BLOCK_SIZE]
            .try_into()
            .expect("slice is AES_BLOCK_SIZE");
        let mut tmp = [0u8; AES_BLOCK_SIZE];
        xor_blocks(&mut tmp, &delta, &plain_block);
        tmp = aes_encrypt(cipher, &tmp);
        let mut cipher_block = [0u8; AES_BLOCK_SIZE];
        xor_blocks(&mut cipher_block, &delta, &tmp);
        encrypted[offset..offset + AES_BLOCK_SIZE].copy_from_slice(&cipher_block);

        xor_in_place(&mut checksum, &plain_block);

        offset += AES_BLOCK_SIZE;
        remaining -= AES_BLOCK_SIZE;
    }

    // Final partial block (0..=16 bytes)
    s2(&mut delta);

    // length-dependent pad
    let mut tmp = [0u8; AES_BLOCK_SIZE];
    // Set the last byte to (remaining_bits) as big-endian u32
    // Mumble encodes: tmp[BLOCKSIZE-1] = SWAPPED(len * 8)
    // For byte-level operations, remaining * 8 cast to u8 in the last byte
    // positions. In Mumble's C++ code with subblock-level ops, this is more
    // complex, but for byte-level: we encode the bit count.
    let bit_len = (remaining * 8) as u32;
    tmp[AES_BLOCK_SIZE - 4] = (bit_len >> 24) as u8;
    tmp[AES_BLOCK_SIZE - 3] = (bit_len >> 16) as u8;
    tmp[AES_BLOCK_SIZE - 2] = (bit_len >> 8) as u8;
    tmp[AES_BLOCK_SIZE - 1] = bit_len as u8;
    xor_in_place(&mut tmp, &delta);
    let pad = aes_encrypt(cipher, &tmp);

    // XOR-encrypt the final partial block
    let mut final_plain = [0u8; AES_BLOCK_SIZE];
    final_plain[..remaining].copy_from_slice(&plain[offset..offset + remaining]);
    final_plain[remaining..].copy_from_slice(&pad[remaining..]);

    xor_in_place(&mut checksum, &final_plain);

    let mut cipher_final = [0u8; AES_BLOCK_SIZE];
    xor_blocks(&mut cipher_final, &pad, &final_plain);
    encrypted[offset..offset + remaining].copy_from_slice(&cipher_final[..remaining]);

    // Compute tag
    s3(&mut delta);
    xor_in_place(&mut checksum, &delta);
    *tag = aes_encrypt(cipher, &checksum);
}

/// OCB2 decrypt: produces plaintext and a 16-byte tag for verification.
fn ocb_decrypt(
    cipher: &Aes128,
    encrypted: &[u8],
    plain: &mut [u8],
    nonce: &[u8; AES_BLOCK_SIZE],
    tag: &mut [u8; AES_BLOCK_SIZE],
) {
    let mut delta = aes_encrypt(cipher, nonce);
    let mut checksum = [0u8; AES_BLOCK_SIZE];

    let mut offset = 0;
    let mut remaining = encrypted.len();

    // Process full 16-byte blocks
    while remaining > AES_BLOCK_SIZE {
        s2(&mut delta);

        #[allow(clippy::expect_used, reason = "slice length is guaranteed by while-loop guard")]
        let cipher_block: [u8; AES_BLOCK_SIZE] = encrypted[offset..offset + AES_BLOCK_SIZE]
            .try_into()
            .expect("slice is AES_BLOCK_SIZE");
        let mut tmp = [0u8; AES_BLOCK_SIZE];
        xor_blocks(&mut tmp, &delta, &cipher_block);
        tmp = aes_decrypt(cipher, &tmp);
        let mut plain_block = [0u8; AES_BLOCK_SIZE];
        xor_blocks(&mut plain_block, &delta, &tmp);
        plain[offset..offset + AES_BLOCK_SIZE].copy_from_slice(&plain_block);

        xor_in_place(&mut checksum, &plain_block);

        offset += AES_BLOCK_SIZE;
        remaining -= AES_BLOCK_SIZE;
    }

    // Final partial block
    s2(&mut delta);

    let mut tmp = [0u8; AES_BLOCK_SIZE];
    let bit_len = (remaining * 8) as u32;
    tmp[AES_BLOCK_SIZE - 4] = (bit_len >> 24) as u8;
    tmp[AES_BLOCK_SIZE - 3] = (bit_len >> 16) as u8;
    tmp[AES_BLOCK_SIZE - 2] = (bit_len >> 8) as u8;
    tmp[AES_BLOCK_SIZE - 1] = bit_len as u8;
    xor_in_place(&mut tmp, &delta);
    let pad = aes_encrypt(cipher, &tmp);

    // Decrypt final partial block
    // Build [ciphertext | zeros], XOR with pad → [plaintext | pad_tail]
    // so checksum includes pad_tail (matching encrypt).
    let mut final_cipher = [0u8; AES_BLOCK_SIZE];
    final_cipher[..remaining].copy_from_slice(&encrypted[offset..offset + remaining]);

    let mut final_plain = [0u8; AES_BLOCK_SIZE];
    xor_blocks(&mut final_plain, &final_cipher, &pad);
    plain[offset..offset + remaining].copy_from_slice(&final_plain[..remaining]);

    xor_in_place(&mut checksum, &final_plain);

    // Compute tag
    s3(&mut delta);
    xor_in_place(&mut checksum, &delta);
    *tag = aes_encrypt(cipher, &checksum);
}

fn carry_iv_bytes(iv: &mut [u8]) {
    for byte in iv.iter_mut().skip(1) {
        *byte = byte.wrapping_add(1);
        if *byte != 0 {
            break;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, reason = "unwrap/expect acceptable in test code")]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_single_block() {
        let mut state = Ocb2CryptState::new();
        state
            .set_key(
                &[1u8; 16],
                &[0u8; 16],
                &[0u8; 16],
            )
            .unwrap();

        let plaintext = b"Hello, Mumble!!"; // 15 bytes (partial block)

        // We need a separate decrypt state with matching nonces:
        // encrypt increments client_nonce, so decrypt must use the
        // same nonce (server_nonce for the receiver).
        // Simulate: after encrypt, the encrypt_iv was incremented.
        // The receiver's decrypt_iv starts at [0;16] (= our client_nonce start).
        let encrypted = state.encrypt(plaintext).unwrap();

        // Create a "receiver" that uses our encrypt direction as its decrypt
        let mut receiver = Ocb2CryptState::new();
        // Receiver's server_nonce = sender's client_nonce starting value
        receiver
            .set_key(
                &[1u8; 16],
                &[0u8; 16],  // receiver's encrypt (unused here)
                &[0u8; 16],  // receiver's decrypt = sender's initial client_nonce
            )
            .unwrap();

        let decrypted = receiver.decrypt(&encrypted).unwrap();
        assert_eq!(&decrypted, plaintext);
    }

    #[test]
    fn roundtrip_multi_block() {
        let mut state = Ocb2CryptState::new();
        state.set_key(&[0xAB; 16], &[0; 16], &[0; 16]).unwrap();

        // 48 bytes = 3 full blocks
        let plaintext = vec![0x42u8; 48];
        let encrypted = state.encrypt(&plaintext).unwrap();

        let mut receiver = Ocb2CryptState::new();
        receiver
            .set_key(&[0xAB; 16], &[0; 16], &[0; 16])
            .unwrap();

        let decrypted = receiver.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn roundtrip_multi_block_plus_partial() {
        let mut state = Ocb2CryptState::new();
        state.set_key(&[0xCD; 16], &[0; 16], &[0; 16]).unwrap();

        // 35 bytes = 2 full blocks + 3 bytes partial
        let plaintext: Vec<u8> = (0..35).map(|i| i as u8).collect();
        let encrypted = state.encrypt(&plaintext).unwrap();

        let mut receiver = Ocb2CryptState::new();
        receiver
            .set_key(&[0xCD; 16], &[0; 16], &[0; 16])
            .unwrap();

        let decrypted = receiver.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn sequential_packets() {
        let mut sender = Ocb2CryptState::new();
        sender.set_key(&[0x11; 16], &[0; 16], &[0; 16]).unwrap();

        let mut receiver = Ocb2CryptState::new();
        receiver
            .set_key(&[0x11; 16], &[0; 16], &[0; 16])
            .unwrap();

        for i in 0..10u8 {
            let plain = vec![i; 20];
            let enc = sender.encrypt(&plain).unwrap();
            let dec = receiver.decrypt(&enc).unwrap();
            assert_eq!(dec, plain, "failed on packet {i}");
        }
        assert_eq!(receiver.stats.good, 10);
        assert_eq!(receiver.stats.lost, 0);
        assert_eq!(receiver.stats.late, 0);
    }

    #[test]
    fn tampered_packet_rejected() {
        let mut sender = Ocb2CryptState::new();
        sender.set_key(&[0x22; 16], &[0; 16], &[0; 16]).unwrap();

        let mut receiver = Ocb2CryptState::new();
        receiver
            .set_key(&[0x22; 16], &[0; 16], &[0; 16])
            .unwrap();

        let plain = b"secret audio data";
        let mut enc = sender.encrypt(plain).unwrap();

        // Tamper with ciphertext
        if let Some(last) = enc.last_mut() {
            *last ^= 0xFF;
        }

        assert!(receiver.decrypt(&enc).is_err());
    }

    #[test]
    fn uninitialized_rejects() {
        let mut state = Ocb2CryptState::new();
        assert!(!state.is_initialized());
        assert!(state.encrypt(b"test").is_err());
        assert!(state.decrypt(&[0; 10]).is_err());
    }

    #[test]
    fn empty_plaintext_roundtrip() {
        let mut sender = Ocb2CryptState::new();
        sender.set_key(&[0x33; 16], &[0; 16], &[0; 16]).unwrap();

        let mut receiver = Ocb2CryptState::new();
        receiver
            .set_key(&[0x33; 16], &[0; 16], &[0; 16])
            .unwrap();

        let enc = sender.encrypt(b"").unwrap();
        assert_eq!(enc.len(), 4); // just the header
        let dec = receiver.decrypt(&enc).unwrap();
        assert!(dec.is_empty());
    }

    #[test]
    fn exactly_one_block_roundtrip() {
        let mut sender = Ocb2CryptState::new();
        sender.set_key(&[0x44; 16], &[0; 16], &[0; 16]).unwrap();

        let mut receiver = Ocb2CryptState::new();
        receiver
            .set_key(&[0x44; 16], &[0; 16], &[0; 16])
            .unwrap();

        let plain = [0x55u8; 16]; // exactly one block
        let enc = sender.encrypt(&plain).unwrap();
        let dec = receiver.decrypt(&enc).unwrap();
        assert_eq!(dec, plain);
    }

    #[test]
    fn nonce_overflow_wraps() {
        let mut sender = Ocb2CryptState::new();
        // Start nonce at 0xFE so byte 0 will overflow after 2 packets
        let mut nonce = [0u8; 16];
        nonce[0] = 0xFE;
        sender.set_key(&[0x55; 16], &nonce, &[0; 16]).unwrap();

        let mut receiver = Ocb2CryptState::new();
        receiver
            .set_key(&[0x55; 16], &[0; 16], &nonce)
            .unwrap();

        for i in 0..5u8 {
            let plain = vec![i; 10];
            let enc = sender.encrypt(&plain).unwrap();
            let dec = receiver.decrypt(&enc).unwrap();
            assert_eq!(dec, plain, "failed on packet {i} (nonce overflow test)");
        }
    }

    #[test]
    fn replay_detection() {
        let mut sender = Ocb2CryptState::new();
        sender.set_key(&[0x66; 16], &[0; 16], &[0; 16]).unwrap();

        let mut receiver = Ocb2CryptState::new();
        receiver
            .set_key(&[0x66; 16], &[0; 16], &[0; 16])
            .unwrap();

        let plain = b"audio frame";
        let enc1 = sender.encrypt(plain).unwrap();
        let enc2 = sender.encrypt(plain).unwrap();

        // Receive both in order
        let _ = receiver.decrypt(&enc1).unwrap();
        let _ = receiver.decrypt(&enc2).unwrap();

        // Replay enc1 - should fail (same iv[0]/iv[1] pair seen)
        assert!(receiver.decrypt(&enc1).is_err());
    }
}
