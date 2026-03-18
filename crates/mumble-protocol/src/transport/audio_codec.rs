//! Legacy Mumble audio packet codec.
//!
//! Before protocol v2 (Mumble 1.5), audio packets use a compact
//! binary encoding - both over raw UDP and inside `UDPTunnel`.
//!
//! ```text
//! +----------+----------+----------+----------------+------------+
//! | header   | session  | sequence | opus len+term  | opus data  |
//! | 1 byte   | varint   | varint   | varint         | N bytes    |
//! |(type|tgt)|(srv->cli) |          | len| terminator|            |
//! +----------+----------+----------+----------------+------------+
//! ```
//!
//! - **header**: `(type << 5) | target`. Type 4 = Opus.
//! - **session**: only present in server -> client direction.
//! - **sequence**: frame counter.
//! - **opus len**: 13-bit length + 1-bit terminator (MSB) packed
//!   as a varint.

use crate::error::{Error, Result};
use crate::proto::mumble_udp;

// -- Audio type enum ------------------------------------------------

/// Audio packet type encoded in the top 3 bits of the Mumble UDP header byte
/// (`value << 5 | target`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioType {
    /// Protobuf v2 audio format, introduced in Mumble 1.5.
    Protobuf = 0,
    /// Legacy binary Opus format, used by Mumble < 1.5.
    Opus = 4,
}

impl TryFrom<u8> for AudioType {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::Protobuf),
            4 => Ok(Self::Opus),
            other => Err(Error::InvalidState(format!("unsupported audio type {other}"  ))),
        }
    }
}

// -- Varint helpers (Mumble-style) ----------------------------------

/// Mumble variable-length integer codec.
///
/// Mumble varints are *not* the same as protobuf LEB128 varints.
/// They encode 7 bits per byte with the continuation bit as the MSB.
pub struct MumbleVarint;

impl MumbleVarint {
    /// Read a Mumble varint from `buf`, returning `(value, bytes_consumed)`.
    pub fn read(buf: &[u8]) -> Result<(u64, usize)> {
        if buf.is_empty() {
            return Err(Error::InvalidState("varint: empty input".into()));
        }

        let first = buf[0];

        // Single byte encoding (bit 7 clear -> value in bits 0-6).
        if first & 0x80 == 0 {
            return Ok((first as u64, 1));
        }

        // Two byte encoding (bits 6-7 = 10 -> 14-bit value).
        if first & 0xC0 == 0x80 {
            if buf.len() < 2 {
                return Err(Error::InvalidState("varint: truncated 2-byte".into()));
            }
            let val = ((first as u64 & 0x3F) << 8) | buf[1] as u64;
            return Ok((val, 2));
        }

        // Three byte encoding (bits 5-7 = 110 -> 21-bit value).
        if first & 0xE0 == 0xC0 {
            if buf.len() < 3 {
                return Err(Error::InvalidState("varint: truncated 3-byte".into()));
            }
            let val = ((first as u64 & 0x1F) << 16)
                | (buf[1] as u64) << 8
                | buf[2] as u64;
            return Ok((val, 3));
        }

        // Four byte encoding (bits 4-7 = 1110 -> 28-bit value).
        if first & 0xF0 == 0xE0 {
            if buf.len() < 4 {
                return Err(Error::InvalidState("varint: truncated 4-byte".into()));
            }
            let val = ((first as u64 & 0x0F) << 24)
                | (buf[1] as u64) << 16
                | (buf[2] as u64) << 8
                | buf[3] as u64;
            return Ok((val, 4));
        }

        // Prefix 11110000 -> 32-bit value in next 4 bytes (big-endian).
        if first == 0xF0 {
            if buf.len() < 5 {
                return Err(Error::InvalidState("varint: truncated 32-bit".into()));
            }
            let val = (buf[1] as u64) << 24
                | (buf[2] as u64) << 16
                | (buf[3] as u64) << 8
                | buf[4] as u64;
            return Ok((val, 5));
        }

        // Prefix 11110100 -> 64-bit value in next 8 bytes.
        if first == 0xF4 {
            if buf.len() < 9 {
                return Err(Error::InvalidState("varint: truncated 64-bit".into()));
            }
            let val = (buf[1] as u64) << 56
                | (buf[2] as u64) << 48
                | (buf[3] as u64) << 40
                | (buf[4] as u64) << 32
                | (buf[5] as u64) << 24
                | (buf[6] as u64) << 16
                | (buf[7] as u64) << 8
                | buf[8] as u64;
            return Ok((val, 9));
        }

        // Negative values (prefix 11111100 / 11111000) - not used for audio.
        Err(Error::InvalidState(format!(
            "varint: unsupported prefix byte 0x{first:02X}"
        )))
    }

    /// Write a Mumble varint into `buf`.
    pub fn write(buf: &mut Vec<u8>, val: u64) {
        if val < 0x80 {
            // 7-bit - single byte, MSB clear.
            buf.push(val as u8);
        } else if val < 0x4000 {
            // 14-bit - two bytes, prefix 10.
            buf.push(0x80 | ((val >> 8) as u8 & 0x3F));
            buf.push(val as u8);
        } else if val < 0x20_0000 {
            // 21-bit - three bytes, prefix 110.
            buf.push(0xC0 | ((val >> 16) as u8 & 0x1F));
            buf.push((val >> 8) as u8);
            buf.push(val as u8);
        } else if val < 0x1000_0000 {
            // 28-bit - four bytes, prefix 1110.
            buf.push(0xE0 | ((val >> 24) as u8 & 0x0F));
            buf.push((val >> 16) as u8);
            buf.push((val >> 8) as u8);
            buf.push(val as u8);
        } else if val <= u32::MAX as u64 {
            // 32-bit - prefix 0xF0 + 4 big-endian bytes.
            buf.push(0xF0);
            buf.push((val >> 24) as u8);
            buf.push((val >> 16) as u8);
            buf.push((val >> 8) as u8);
            buf.push(val as u8);
        } else {
            // 64-bit - prefix 0xF4 + 8 big-endian bytes.
            buf.push(0xF4);
            for shift in (0..8).rev() {
                buf.push((val >> (shift * 8)) as u8);
            }
        }
    }
}

// -- Codec trait ----------------------------------------------------

/// Common interface for a Mumble UDP audio packet format.
///
/// Each implementation handles one audio type value (`AUDIO_TYPE`) that
/// is encoded in the top 3 bits of the Mumble UDP header byte
/// (`(type << 5) | target`).
pub trait AudioPacketCodec {
    /// Audio type value encoded in the top 3 bits of the header byte.
    const AUDIO_TYPE: AudioType;

    /// Encode `audio` into the wire format, including the 1-byte header.
    fn encode(audio: &mumble_udp::Audio) -> Vec<u8>;

    /// Decode `data` (including the 1-byte header) as received from the server.
    fn decode(data: &[u8]) -> Result<mumble_udp::Audio>;
}

// -- LegacyAudioCodec -----------------------------------------------

/// Legacy binary Opus codec used by Mumble < 1.5.
pub struct LegacyAudioCodec;

impl AudioPacketCodec for LegacyAudioCodec {
    const AUDIO_TYPE: AudioType = AudioType::Opus;

    fn encode(audio: &mumble_udp::Audio) -> Vec<u8> {
        let target = match audio.header {
            Some(mumble_udp::audio::Header::Target(t)) => t as u8 & 0x1F,
            _ => 0,
        };

        let header = (AudioType::Opus as u8) << 5 | target;
        let mut buf = Vec::with_capacity(1 + 9 + 9 + 2 + audio.opus_data.len());
        buf.push(header);

        // Client -> server: no session ID.
        // Sequence number.
        MumbleVarint::write(&mut buf, audio.frame_number);

        // Opus length + terminator.
        let mut len_term = audio.opus_data.len() as u64 & 0x1FFF;
        if audio.is_terminator {
            len_term |= 0x2000;
        }
        MumbleVarint::write(&mut buf, len_term);
        buf.extend_from_slice(&audio.opus_data);
        buf
    }

    fn decode(data: &[u8]) -> Result<mumble_udp::Audio> {
                if data.is_empty() {
            return Err(Error::InvalidState("empty audio packet".into()));
        }

        let header = data[0];
        let audio_type = header >> 5;
        let target = header & 0x1F;

        if audio_type != AudioType::Opus as u8 {
            return Err(Error::InvalidState(format!(
                "unsupported audio type {audio_type} (expected Opus = {})",
                AudioType::Opus as u8
            )));
        }

        let mut pos: usize = 1;

        // Session ID (only in server -> client).
        let sender_session = {
            let (session, n) = MumbleVarint::read(&data[pos..])?;
            pos += n;
            session as u32
        };

        // Sequence number.
        let (sequence, n) = MumbleVarint::read(&data[pos..])?;
        pos += n;

        // Opus payload length + terminator bit.
        // The varint value encodes: `(length << 1) | terminator` for CELT
        // but for Opus it is `length | (terminator << 13)`.
        // Actually per Mumble source: for Opus, the varint holds the raw
        // length in the bottom 13 bits and bit 13 is the terminator flag.
        let (len_term, n) = MumbleVarint::read(&data[pos..])?;
        pos += n;

        let opus_len = (len_term & 0x1FFF) as usize;
        let is_terminator = (len_term & 0x2000) != 0;

        if pos + opus_len > data.len() {
            return Err(Error::InvalidState(format!(
                "opus data truncated: need {opus_len} bytes, have {}",
                data.len() - pos
            )));
        }

        let opus_data = data[pos..pos + opus_len].to_vec();

        Ok(mumble_udp::Audio {
            header: Some(mumble_udp::audio::Header::Target(target as u32)),
            sender_session,
            frame_number: sequence,
            opus_data,
            positional_data: Vec::new(),
            volume_adjustment: 0.0,
            is_terminator,
        })
    }
}

// -- ProtobufAudioCodec ---------------------------------------------

/// Protobuf v2 audio codec introduced in Mumble 1.5.
pub struct ProtobufAudioCodec;

impl AudioPacketCodec for ProtobufAudioCodec {
    const AUDIO_TYPE: AudioType = AudioType::Protobuf;

    fn encode(audio: &mumble_udp::Audio) -> Vec<u8> {
        use prost::Message as _;

        let target = match audio.header {
            Some(mumble_udp::audio::Header::Target(t)) => t as u8 & 0x1F,
            _ => 0,
        };

        let header = (AudioType::Protobuf as u8) << 5 | target;

        // Encode the protobuf payload (without the header/target field,
        // since that's carried in byte 0).
        let mut wire_audio = audio.clone();
        wire_audio.header = None; // target goes in the header byte
        let proto_bytes = wire_audio.encode_to_vec();

        let mut buf = Vec::with_capacity(1 + proto_bytes.len());
        buf.push(header);
        buf.extend_from_slice(&proto_bytes);
        buf
    }

    fn decode(data: &[u8]) -> Result<mumble_udp::Audio> {
        use prost::Message as _;
        // Byte 0 is the type/target header; bytes 1..N are the protobuf payload.
        let target = (data[0] & 0x1F) as u32;
        let mut audio = mumble_udp::Audio::decode(&data[1..])
            .map_err(|e| Error::InvalidState(format!("protobuf audio decode: {e}")))?;
        // The target is carried in the header byte, not inside the
        // protobuf payload on the wire.  Fill it in if missing.
        if audio.header.is_none() {
            audio.header = Some(mumble_udp::audio::Header::Target(target));
        }
        Ok(audio)
    }
}

// -- Public API -----------------------------------------------------

/// Try to decode a `UdpTunnel` payload as audio.
///
/// Both legacy and protobuf v2 formats share the same header byte
/// layout: `(type << 5) | target`.
///
/// - `type == 0` -> protobuf v2 Audio  (Mumble 1.5+)
/// - `type == 4` -> legacy Opus binary
/// - Other types -> unsupported (CELT, Speex, etc.)
pub fn decode_tunnel_audio(data: &[u8]) -> Result<mumble_udp::Audio> {
    if data.is_empty() {
        return Err(Error::InvalidState("empty UdpTunnel payload".into()));
    }

    match AudioType::try_from(data[0] >> 5)? {
        AudioType::Protobuf => ProtobufAudioCodec::decode(data),
        AudioType::Opus => LegacyAudioCodec::decode(data),
    }
}

// --- Tests ---------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_roundtrip_values() {
        for &val in &[0u64, 1, 0x7F, 0x80, 0x3FFF, 0x4000, 0x1F_FFFF, 0x0FFF_FFFF, 0xFFFF_FFFF, 0x1_0000_0000u64] {
            let mut buf = Vec::new();
            MumbleVarint::write(&mut buf, val);
            let (decoded, n) = MumbleVarint::read(&buf).unwrap();
            assert_eq!(decoded, val, "varint roundtrip failed for {val}");
            assert_eq!(n, buf.len(), "varint consumed wrong number of bytes for {val}");
        }
    }

    #[test]
    fn legacy_encode_decode_roundtrip() {
        // Build a server-format packet (with session id) and verify decode.
        let mut buf = Vec::new();
        buf.push((AudioType::Opus as u8) << 5); // header: Opus, target 0
        MumbleVarint::write(&mut buf, 7);  // session = 7
        MumbleVarint::write(&mut buf, 42); // sequence = 42
        let opus = vec![0xDE, 0xAD, 0xBE, 0xEF];
        MumbleVarint::write(&mut buf, opus.len() as u64);
        buf.extend_from_slice(&opus);

        let decoded = LegacyAudioCodec::decode(&buf).unwrap();
        assert_eq!(decoded.sender_session, 7);
        assert_eq!(decoded.frame_number, 42);
        assert_eq!(decoded.opus_data, vec![0xDE, 0xAD, 0xBE, 0xEF]);
        assert!(!decoded.is_terminator);
    }

    #[test]
    fn legacy_decode_with_session() {
        // Build a packet as the server would send it (with session id).
        let mut buf = Vec::new();
        buf.push((AudioType::Opus as u8) << 5); // header: Opus, target 0
        MumbleVarint::write(&mut buf, 5);   // session = 5
        MumbleVarint::write(&mut buf, 10);  // sequence = 10
        let opus = vec![1, 2, 3];
        MumbleVarint::write(&mut buf, opus.len() as u64); // length (no terminator)
        buf.extend_from_slice(&opus);

        let decoded = LegacyAudioCodec::decode(&buf).unwrap();
        assert_eq!(decoded.sender_session, 5);
        assert_eq!(decoded.frame_number, 10);
        assert_eq!(decoded.opus_data, vec![1, 2, 3]);
        assert!(!decoded.is_terminator);
    }

    #[test]
    fn legacy_terminator_bit() {
        // Build a server-format packet (with session id) that has the terminator bit set.
        let mut buf = Vec::new();
        buf.push((AudioType::Opus as u8) << 5); // header: Opus, target 0
        MumbleVarint::write(&mut buf, 0);  // session = 0
        MumbleVarint::write(&mut buf, 1);  // sequence = 1
        let opus = vec![0xFF];
        let len_term = opus.len() as u64 | 0x2000; // set terminator bit
        MumbleVarint::write(&mut buf, len_term);
        buf.extend_from_slice(&opus);

        let decoded = LegacyAudioCodec::decode(&buf).unwrap();
        assert!(decoded.is_terminator);
        assert_eq!(decoded.opus_data, vec![0xFF]);
    }

    #[test]
    fn decode_tunnel_prefers_legacy_when_protobuf_fails() {
        // Craft a legacy packet - not valid protobuf.
        let mut buf = Vec::new();
        buf.push((AudioType::Opus as u8) << 5);
        MumbleVarint::write(&mut buf, 7);   // session
        MumbleVarint::write(&mut buf, 99);  // sequence
        let opus = vec![0xAA, 0xBB];
        MumbleVarint::write(&mut buf, opus.len() as u64);
        buf.extend_from_slice(&opus);

        let decoded = decode_tunnel_audio(&buf).unwrap();
        assert_eq!(decoded.sender_session, 7);
        assert_eq!(decoded.frame_number, 99);
        assert_eq!(decoded.opus_data, vec![0xAA, 0xBB]);
    }

    #[test]
    fn decode_tunnel_protobuf_v2_format() {
        use prost::Message as _;
        // Build a protobuf v2 packet: 1 byte type header + protobuf payload.
        let audio = mumble_udp::Audio {
            header: None, // target comes from type byte
            sender_session: 42,
            frame_number: 100,
            opus_data: vec![0x01, 0x02, 0x03],
            positional_data: Vec::new(),
            volume_adjustment: 0.0,
            is_terminator: false,
        };
        let mut buf = Vec::new();
        buf.push((AudioType::Protobuf as u8) << 5); // type=0, target=0
        buf.extend_from_slice(&audio.encode_to_vec());

        let decoded = decode_tunnel_audio(&buf).unwrap();
        assert_eq!(decoded.sender_session, 42);
        assert_eq!(decoded.frame_number, 100);
        assert_eq!(decoded.opus_data, vec![0x01, 0x02, 0x03]);
    }

    #[test]
    fn decode_tunnel_protobuf_v2_with_target() {
        use prost::Message as _;
        let audio = mumble_udp::Audio {
            header: None,
            sender_session: 10,
            frame_number: 50,
            opus_data: vec![0xCC],
            positional_data: Vec::new(),
            volume_adjustment: 0.0,
            is_terminator: false,
        };
        let mut buf = Vec::new();
        let target: u8 = 3;
        buf.push(((AudioType::Protobuf as u8) << 5) | target); // type=0, target=3
        buf.extend_from_slice(&audio.encode_to_vec());

        let decoded = decode_tunnel_audio(&buf).unwrap();
        assert_eq!(decoded.sender_session, 10);
        // Target should be filled from the header byte
        match decoded.header {
            Some(mumble_udp::audio::Header::Target(t)) => assert_eq!(t, 3),
            other => panic!("expected Target(3), got {other:?}"),
        }
    }
}
