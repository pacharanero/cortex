// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The reassembled Cortex Control message envelope.
//!
//! A reassembled message is `protobuf ++ 8-byte trailer`. The trailer carries
//! the message-type tag as a little-endian `uint16` `CortexMessageType.Enum`
//! (defined in `ProductionAutomation.proto`). The remaining 6 bytes of the
//! trailer are not part of the protobuf message body and are stripped before
//! decoding.
//!
//! @see spec/110-framing/spec.md [FR-13]
//! @see spec/120-proto-schema/spec.md
//! @see spec/130-domain-model/spec.md [FR-4] [FR-5] [FR-6] [FR-7]

use bytes::Bytes;

/// The 8-byte trailer length on a reassembled message.
pub const TRAILER_LEN: usize = 8;

/// The largest decompressed body [`Message::decode`] will retain from a
/// frame-level gzip payload, comfortably above the 150,008-byte largest body
/// reassembled from `CorOS` 4.0.1 hardware. Without this bound, a malformed
/// or hostile gzip stream inside an otherwise-reassembled message could
/// inflate to an unbounded size before `Message::decode` returns.
pub const MAX_DECOMPRESSED_MESSAGE_LEN: usize = 8 * 1024 * 1024;

/// A reassembled Cortex Control message: the protobuf body and the
/// message-type tag read from the trailer.
#[derive(Debug, Clone)]
pub struct Message {
    /// The little-endian `uint16` message-type tag from the trailer. This is
    /// a value of `CortexMessageType.Enum` (see the generated `proto` module).
    pub message_type: u16,
    /// The protobuf message body (trailer already stripped).
    pub body: Bytes,
}

impl Message {
    /// Parse a reassembled message buffer (protobuf body + 8-byte trailer)
    /// into a [`Message`].
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::Trailer`] if the buffer is shorter than the
    /// 8-byte trailer.
    pub fn parse(reassembled: &[u8]) -> crate::Result<Self> {
        if reassembled.len() < TRAILER_LEN {
            return Err(crate::Error::Trailer(format!(
                "reassembled message too short: {} bytes (need at least {TRAILER_LEN})",
                reassembled.len()
            )));
        }
        let split = reassembled.len() - TRAILER_LEN;
        let body = Bytes::copy_from_slice(&reassembled[..split]);
        // The message-type tag is the first two bytes of the trailer,
        // little-endian. The remaining 6 bytes are currently unused by this
        // client (the recovered schema does not document them).
        let message_type = u16::from_le_bytes([reassembled[split], reassembled[split + 1]]);
        Ok(Self { message_type, body })
    }

    /// Returns the message-type tag as a `CortexMessageType.Enum` value, if
    /// it is in the documented range (0..=71 against `CorOS` 4.0.1).
    #[must_use]
    pub fn message_type_value(&self) -> u16 {
        self.message_type
    }

    /// Parse a reassembled buffer and decompress a frame-level gzip body.
    ///
    /// Returns the message and whether its body arrived gzipped, which is
    /// worth surfacing: it distinguishes a large message from an expensive
    /// one.
    ///
    /// The order matters and is easy to get wrong. The 8-byte trailer is
    /// stripped FIRST, because the type tag sits outside the compression; a
    /// decoder that gunzips before reading the trailer finds neither.
    ///
    /// Exists so the live RX path and any offline decoder share one
    /// implementation. They previously could not disagree because there was
    /// only one; this keeps that true now there is more than one caller.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::Trailer`] if the buffer is too short, or
    /// [`crate::Error::Decode`] if a gzip body will not inflate or its
    /// decompressed size exceeds [`MAX_DECOMPRESSED_MESSAGE_LEN`].
    pub fn decode(reassembled: &[u8]) -> crate::Result<(Self, bool)> {
        let mut msg = Self::parse(reassembled)?;
        let gzipped = msg.body.starts_with(&[0x1f, 0x8b]);
        if gzipped {
            let decompressed =
                bounded_gunzip(&msg.body[..], MAX_DECOMPRESSED_MESSAGE_LEN, "message body")?;
            msg.body = Bytes::from(decompressed);
        }
        Ok((msg, gzipped))
    }
}

/// Gunzip `reader`, rejecting a decompressed size over `limit` bytes.
///
/// Reads through [`Read::take`] set to `limit + 1`, so at most one byte over
/// the limit is ever pulled through the decoder before this returns - a
/// malformed or hostile gzip stream cannot allocate memory in proportion to
/// its (attacker-controlled) claimed size.
///
/// `context` names the field being decompressed, for the error message.
///
/// # Errors
///
/// Returns [`crate::Error::Decode`] if the stream is not valid gzip or its
/// decompressed size exceeds `limit`.
pub(crate) fn bounded_gunzip(
    reader: impl std::io::Read,
    limit: usize,
    context: &str,
) -> crate::Result<Vec<u8>> {
    use std::io::Read as _;

    let mut decoder = flate2::read::GzDecoder::new(reader)
        .take(u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1));
    let mut decompressed = Vec::new();
    decoder
        .read_to_end(&mut decompressed)
        .map_err(|e| crate::Error::Decode(format!("{context}: gzip: {e}")))?;
    if decompressed.len() > limit {
        return Err(crate::Error::Decode(format!(
            "{context}: decompressed size exceeds {limit}-byte limit"
        )));
    }
    Ok(decompressed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_body_and_trailer() {
        let body = b"protobuf bytes";
        let mut msg = body.to_vec();
        // 8-byte trailer: little-endian u16 type (e.g. Version = 10), then
        // 6 bytes of padding.
        msg.extend_from_slice(&10u16.to_le_bytes());
        msg.extend_from_slice(&[0u8; 6]);

        let parsed = Message::parse(&msg).unwrap();
        assert_eq!(parsed.message_type, 10);
        assert_eq!(parsed.body.as_ref(), body);
    }

    #[test]
    fn rejects_too_short() {
        let err = Message::parse(b"short").unwrap_err();
        assert!(matches!(err, crate::Error::Trailer(_)));
    }

    fn gzip(plain: &[u8]) -> Vec<u8> {
        use std::io::Write as _;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(plain).expect("fixture compresses");
        encoder.finish().expect("fixture finishes")
    }

    // Exercised directly against the parameterized private helper so the
    // boundary is proven without allocating an 8 MiB vector per test.
    #[test]
    fn bounded_gunzip_accepts_exactly_the_limit() {
        let payload = gzip(&[7u8; 10]);
        let decompressed = bounded_gunzip(&payload[..], 10, "test").unwrap();
        assert_eq!(decompressed, vec![7u8; 10]);
    }

    #[test]
    fn bounded_gunzip_rejects_one_byte_over_the_limit() {
        let payload = gzip(&[7u8; 11]);
        let err = bounded_gunzip(&payload[..], 10, "test").unwrap_err();
        assert!(matches!(err, crate::Error::Decode(_)));
    }

    #[test]
    fn bounded_gunzip_rejects_malformed_gzip() {
        let err = bounded_gunzip(&b"not gzip at all"[..], 1024, "test").unwrap_err();
        assert!(matches!(err, crate::Error::Decode(_)));
    }

    #[test]
    fn decode_gunzips_a_frame_level_gzip_body_under_the_real_limit() {
        let body = b"protobuf bytes";
        let mut reassembled = gzip(body);
        reassembled.extend_from_slice(&10u16.to_le_bytes());
        reassembled.extend_from_slice(&[0u8; 6]);

        let (parsed, gzipped) = Message::decode(&reassembled).unwrap();
        assert!(gzipped);
        assert_eq!(parsed.body.as_ref(), body);
    }

    #[test]
    fn decode_rejects_a_frame_level_gzip_body_over_the_real_limit() {
        let mut reassembled = gzip(&vec![0u8; MAX_DECOMPRESSED_MESSAGE_LEN + 1]);
        reassembled.extend_from_slice(&10u16.to_le_bytes());
        reassembled.extend_from_slice(&[0u8; 6]);

        let err = Message::decode(&reassembled).unwrap_err();
        assert!(matches!(err, crate::Error::Decode(_)));
    }
}
