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

use bytes::Bytes;

/// The 8-byte trailer length on a reassembled message.
pub const TRAILER_LEN: usize = 8;

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
    /// [`crate::Error::Decode`] if a gzip body will not inflate.
    pub fn decode(reassembled: &[u8]) -> crate::Result<(Self, bool)> {
        let mut msg = Self::parse(reassembled)?;
        let gzipped = msg.body.starts_with(&[0x1f, 0x8b]);
        if gzipped {
            use std::io::Read;
            let mut decoder = flate2::read::GzDecoder::new(&msg.body[..]);
            let mut decompressed = Vec::new();
            decoder
                .read_to_end(&mut decompressed)
                .map_err(|e| crate::Error::Decode(format!("gzip: {e}")))?;
            msg.body = Bytes::from(decompressed);
        }
        Ok((msg, gzipped))
    }
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
}
