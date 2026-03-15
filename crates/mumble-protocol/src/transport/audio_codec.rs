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

/// Audio codec type encoded in the top 3 bits of the header byte.
const AUDIO_TYPE_OPUS: u8 = 4;
/// Protobuf Audio message type (Mumble 1.5+).
const AUDIO_TYPE_PROTO: u8 = 0;
/// Protobuf Ping message type.
const _PING_TYPE_PROTO: u8 = 1;

// -- Varint helpers (Mumble-style) ----------------------------------

/// Read a Mumble varint from `buf`, returning `(value, bytes_consumed)`.
///
/// Mumble varints are *not* the same as protobuf LEB128 varints.
/// They encode 7 bits per byte with the continuation bit as the MSB.
fn read_varint(buf: &[u8]) -> Result<(u64, usize)> {
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

/// Write a Mumble varint into `buf`, returning bytes written.
fn write_varint(buf: &mut Vec<u8>, val: u64) {
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

// -- Public API -----------------------------------------------------

/// Try to decode a legacy Mumble audio packet from raw bytes.
///
/// `from_server` must be `true` when the packet was received from the
/// server (which includes a session-id varint after the header) and
/// `false` for client -> server packets (which omit the session).
pub fn decode_legacy_audio(data: &[u8], from_server: bool) -> Result<mumble_udp::Audio> {
    if data.is_empty() {
        return Err(Error::InvalidState("empty audio packet".into()));
    }

    let header = data[0];
    let audio_type = header >> 5;
    let target = header & 0x1F;

    if audio_type != AUDIO_TYPE_OPUS {
        return Err(Error::InvalidState(format!(
            "unsupported audio type {audio_type} (expected Opus = {AUDIO_TYPE_OPUS})"
        )));
    }

    let mut pos: usize = 1;

    // Session ID (only in server -> client).
    let sender_session = if from_server {
        let (session, n) = read_varint(&data[pos..])?;
        pos += n;
        session as u32
    } else {
        0
    };

    // Sequence number.
    let (sequence, n) = read_varint(&data[pos..])?;
    pos += n;

    // Opus payload length + terminator bit.
    // The varint value encodes: `(length << 1) | terminator` for CELT
    // but for Opus it is `length | (terminator << 13)`.
    // Actually per Mumble source: for Opus, the varint holds the raw
    // length in the bottom 13 bits and bit 13 is the terminator flag.
    let (len_term, n) = read_varint(&data[pos..])?;
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

/// Encode audio into legacy Mumble binary format (client -> server).
///
/// The resulting bytes are suitable for wrapping in a `UDPTunnel`
/// control message (or sending as a raw UDP packet).
pub fn encode_legacy_audio(audio: &mumble_udp::Audio) -> Vec<u8> {
    let target = match audio.header {
        Some(mumble_udp::audio::Header::Target(t)) => t as u8 & 0x1F,
        _ => 0,
    };

    let header = (AUDIO_TYPE_OPUS << 5) | target;
    let mut buf = Vec::with_capacity(1 + 9 + 9 + 2 + audio.opus_data.len());

    buf.push(header);

    // Client -> server: no session ID.
    // Sequence number.
    write_varint(&mut buf, audio.frame_number);

    // Opus length + terminator.
    let mut len_term = audio.opus_data.len() as u64 & 0x1FFF;
    if audio.is_terminator {
        len_term |= 0x2000;
    }
    write_varint(&mut buf, len_term);

    buf.extend_from_slice(&audio.opus_data);
    buf
}

/// Encode audio into Mumble protobuf v2 format for `UdpTunnel`.
///
/// ```text
/// +----------+--------------------------+
/// | header   | protobuf Audio payload   |
/// | 1 byte   | N bytes                  |
/// |(type|tgt)|                          |
/// +----------+--------------------------+
/// ```
pub fn encode_protobuf_audio(audio: &mumble_udp::Audio) -> Vec<u8> {
    use prost::Message as _;

    let target = match audio.header {
        Some(mumble_udp::audio::Header::Target(t)) => t as u8 & 0x1F,
        _ => 0,
    };

    let header = (AUDIO_TYPE_PROTO << 5) | target;

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

    let header = data[0];
    let audio_type = header >> 5;

    match audio_type {
        AUDIO_TYPE_PROTO => {
            // Protobuf v2: byte 0 is the type/target header,
            // bytes 1..N are the protobuf-encoded Audio message.
            use prost::Message as _;
            let target = (header & 0x1F) as u32;
            let mut audio = mumble_udp::Audio::decode(&data[1..])
                .map_err(|e| Error::InvalidState(format!("protobuf audio decode: {e}")))?;
            // The target is carried in the header byte, not inside the
            // protobuf payload on the wire.  Fill it in if missing.
            if audio.header.is_none() {
                audio.header = Some(mumble_udp::audio::Header::Target(target));
            }
            Ok(audio)
        }
        AUDIO_TYPE_OPUS => {
            // Legacy binary Opus format (server -> client includes session).
            decode_legacy_audio(data, true)
        }
        _ => Err(Error::InvalidState(format!(
            "unsupported audio type {audio_type}"
        ))),
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
            write_varint(&mut buf, val);
            let (decoded, n) = read_varint(&buf).unwrap();
            assert_eq!(decoded, val, "varint roundtrip failed for {val}");
            assert_eq!(n, buf.len(), "varint consumed wrong number of bytes for {val}");
        }
    }

    #[test]
    fn legacy_encode_decode_roundtrip() {
        let audio = mumble_udp::Audio {
            header: Some(mumble_udp::audio::Header::Target(0)),
            sender_session: 0,
            frame_number: 42,
            opus_data: vec![0xDE, 0xAD, 0xBE, 0xEF],
            positional_data: Vec::new(),
            volume_adjustment: 0.0,
            is_terminator: false,
        };

        let encoded = encode_legacy_audio(&audio);
        let decoded = decode_legacy_audio(&encoded, false).unwrap();

        assert_eq!(decoded.frame_number, 42);
        assert_eq!(decoded.opus_data, vec![0xDE, 0xAD, 0xBE, 0xEF]);
        assert!(!decoded.is_terminator);
    }

    #[test]
    fn legacy_decode_with_session() {
        // Build a packet as the server would send it (with session id).
        let mut buf = Vec::new();
        buf.push(AUDIO_TYPE_OPUS << 5); // header: Opus, target 0
        write_varint(&mut buf, 5);   // session = 5
        write_varint(&mut buf, 10);  // sequence = 10
        let opus = vec![1, 2, 3];
        write_varint(&mut buf, opus.len() as u64); // length (no terminator)
        buf.extend_from_slice(&opus);

        let decoded = decode_legacy_audio(&buf, true).unwrap();
        assert_eq!(decoded.sender_session, 5);
        assert_eq!(decoded.frame_number, 10);
        assert_eq!(decoded.opus_data, vec![1, 2, 3]);
        assert!(!decoded.is_terminator);
    }

    #[test]
    fn legacy_terminator_bit() {
        let audio = mumble_udp::Audio {
            header: Some(mumble_udp::audio::Header::Target(0)),
            sender_session: 0,
            frame_number: 1,
            opus_data: vec![0xFF],
            positional_data: Vec::new(),
            volume_adjustment: 0.0,
            is_terminator: true,
        };

        let encoded = encode_legacy_audio(&audio);
        let decoded = decode_legacy_audio(&encoded, false).unwrap();
        assert!(decoded.is_terminator);
        assert_eq!(decoded.opus_data, vec![0xFF]);
    }

    #[test]
    fn decode_tunnel_prefers_legacy_when_protobuf_fails() {
        // Craft a legacy packet - not valid protobuf.
        let mut buf = Vec::new();
        buf.push(AUDIO_TYPE_OPUS << 5);
        write_varint(&mut buf, 7);   // session
        write_varint(&mut buf, 99);  // sequence
        let opus = vec![0xAA, 0xBB];
        write_varint(&mut buf, opus.len() as u64);
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
        buf.push(AUDIO_TYPE_PROTO << 5); // type=0, target=0
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
        buf.push((AUDIO_TYPE_PROTO << 5) | target); // type=0, target=3
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
