// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Cortex Control HID report framing.
//!
//! Hardware-verified against a Quad Cortex on `CorOS` 4.0.1 and a Nano Cortex:
//!
//! - Quad reports are 128-byte body + report ID = 129 bytes; Nano reports are
//!   64-byte body + report ID = 65 bytes at the hidapi boundary.
//! - Input report ID `0x01`, output report ID `0x02`.
//! - Frame layout: `[report_id][len][flags][data...]`.
//! - `flags`: `0x40` FIRST, `0x80` LAST, `0xC0` complete, `0x00` middle.
//! - No sequence numbers, no offsets, no total-length field. Reassembly is
//!   purely flag-driven.
//!
//! Reassembly is a tiny state machine: a FIRST frame starts a buffer, middle
//! frames append, a LAST or COMPLETE frame appends and emits the whole
//! message. This module owns that state machine.
//!
//! @see spec/110-framing/spec.md [FR-1] [FR-9] [FR-13]
//! @see spec/110-framing/design.md [DES-FRAME] [DES-REASSEMBLY] [DES-ENCODE]

use serde::{Deserialize, Serialize};

/// Fixed HID report dimensions for one Cortex device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HidReportGeometry {
    body_len: usize,
}

impl HidReportGeometry {
    /// Quad Cortex: 128-byte body, 129 bytes including the report ID.
    pub const QUAD_CORTEX: Self = Self { body_len: 128 };
    /// Nano Cortex: 64-byte body, 65 bytes including the report ID.
    pub const NANO_CORTEX: Self = Self { body_len: 64 };

    /// Report body length, excluding the report ID.
    #[must_use]
    pub const fn body_len(self) -> usize {
        self.body_len
    }

    /// Full report length at the hidapi boundary.
    #[must_use]
    pub const fn report_len(self) -> usize {
        self.body_len + 1
    }

    /// Meaningful frame-data capacity after the `[len][flags]` prefix.
    #[must_use]
    pub const fn data_capacity(self) -> usize {
        self.body_len - 2
    }
}

/// Quad Cortex HID body length in bytes, excluding the report-id byte.
pub const HID_BODY_LEN: usize = HidReportGeometry::QUAD_CORTEX.body_len();

/// Total Quad Cortex report length at the hidapi boundary.
pub const HID_REPORT_LEN: usize = HidReportGeometry::QUAD_CORTEX.report_len();

/// HID report ID. Input is `0x01` (device-to-host), output is `0x02`
/// (host-to-device, sent via `SET_REPORT` on the control pipe).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReportId {
    /// `0x01` - device-to-host.
    Input = 0x01,
    /// `0x02` - host-to-device (via `SET_REPORT`).
    Output = 0x02,
}

impl ReportId {
    /// Parse a raw report-ID byte.
    #[must_use]
    pub const fn from_raw(byte: u8) -> Option<Self> {
        match byte {
            0x01 => Some(Self::Input),
            0x02 => Some(Self::Output),
            _ => None,
        }
    }
}

/// The flag byte on a frame. Encodes where this frame sits in a multi-frame
/// message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Flags(pub u8);

impl Flags {
    /// `0x40` - the first frame of a multi-frame message.
    pub const FIRST: u8 = 0x40;
    /// `0x80` - the last frame of a multi-frame message.
    pub const LAST: u8 = 0x80;
    /// `0xC0` - a complete message in a single frame (FIRST | LAST).
    pub const COMPLETE: u8 = 0xC0;
    /// `0x00` - a middle frame.
    pub const MIDDLE: u8 = 0x00;

    /// True if this frame is the first of a message (or the only one).
    #[must_use]
    pub const fn is_first(self) -> bool {
        self.0 & Self::FIRST != 0
    }

    /// True if this frame is the last of a message (or the only one).
    #[must_use]
    pub const fn is_last(self) -> bool {
        self.0 & Self::LAST != 0
    }

    /// True if this single frame carries a complete message.
    #[must_use]
    pub const fn is_complete(self) -> bool {
        self.0 == Self::COMPLETE
    }

    /// True if this frame is a middle frame of a multi-frame message.
    #[must_use]
    pub const fn is_middle(self) -> bool {
        self.0 == Self::MIDDLE
    }
}

/// A parsed HID frame: the flag byte and the data payload (after the report
/// ID and length byte have been stripped).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// The flag byte.
    pub flags: Flags,
    /// The frame's data payload.
    pub data: Vec<u8>,
}

impl Frame {
    /// Parse a device-sized hidapi report into a `Frame`.
    ///
    /// The first byte is the report ID, the second is `len` (the number of
    /// meaningful bytes that follow, excluding the report ID and length
    /// byte themselves), and the third is the flags byte.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::Framing`] if the report is too short or its
    /// declared length exceeds the available bytes.
    pub fn parse(report: &[u8]) -> crate::Result<Self> {
        if report.len() < 3 {
            return Err(crate::Error::Framing(format!(
                "report too short: {} bytes (need at least 3)",
                report.len()
            )));
        }
        // Byte 0 is the report ID (validated by the caller / transport).
        let len = usize::from(report[1]);
        let flags = Flags(report[2]);
        let data_end = 3 + len;
        if data_end > report.len() {
            return Err(crate::Error::Framing(format!(
                "declared len {len} exceeds available bytes ({})",
                report.len() - 3
            )));
        }
        Ok(Self {
            flags,
            data: report[3..data_end].to_vec(),
        })
    }
}

/// The flag-driven reassembly state machine.
///
/// Feeds `Frame`s in order and emits the reassembled message body when a
/// LAST or COMPLETE frame arrives. There are no sequence numbers and no
/// total-length field on the wire, so a missing first frame or an
/// out-of-order frame is detected only by the flag mismatch.
pub struct FrameReassembler {
    buffer: Vec<u8>,
    in_progress: bool,
}

impl Default for FrameReassembler {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameReassembler {
    /// Create a fresh reassembler.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            buffer: Vec::new(),
            in_progress: false,
        }
    }

    /// Feed one frame. Returns `Some(body)` if this frame completes a
    /// message, or `None` if more frames are expected.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::Framing`] on a middle/last frame without a
    /// preceding first frame, or on an unknown flags byte value.
    pub fn feed(&mut self, frame: &Frame) -> crate::Result<Option<Vec<u8>>> {
        if frame.flags.is_complete() {
            // Single-frame message: no state to carry.
            self.reset();
            return Ok(Some(frame.data.clone()));
        }

        if frame.flags.is_first() {
            // Start a new message.
            self.reset();
            self.buffer.extend_from_slice(&frame.data);
            self.in_progress = true;
            return Ok(None);
        }

        if frame.flags.is_middle() {
            if !self.in_progress {
                return Err(crate::Error::Framing(
                    "middle frame without a preceding first frame".into(),
                ));
            }
            self.buffer.extend_from_slice(&frame.data);
            return Ok(None);
        }

        if frame.flags.is_last() {
            if !self.in_progress {
                return Err(crate::Error::Framing(
                    "last frame without a preceding first frame".into(),
                ));
            }
            self.buffer.extend_from_slice(&frame.data);
            let body = std::mem::take(&mut self.buffer);
            self.in_progress = false;
            return Ok(Some(body));
        }

        // Unknown flag value.
        Err(crate::Error::Framing(format!(
            "unknown flags byte: {:#04x}",
            frame.flags.0
        )))
    }

    /// Reset the state machine (e.g. on a transport-level reconnect).
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.in_progress = false;
    }
}

/// Per-report data capacity: the 128-byte body minus the `[len][flags]`
/// prefix, i.e. 126 bytes of payload per report.
pub const CHUNK_SIZE: usize = HidReportGeometry::QUAD_CORTEX.data_capacity();

/// Wrap an already-formed application body in device-sized HID reports.
///
/// This function does not add or interpret a Quad or Nano application
/// envelope. Callers select report geometry and provide the complete logical
/// body, including any device-specific trailer or footer.
#[must_use]
pub fn encode_reports(geometry: HidReportGeometry, body: &[u8]) -> Vec<Vec<u8>> {
    let chunk_size = geometry.data_capacity();
    let chunk_count = body.len().div_ceil(chunk_size).max(1);
    let mut reports = Vec::with_capacity(chunk_count);

    for i in 0..chunk_count {
        let start = i * chunk_size;
        let end = (start + chunk_size).min(body.len());
        let chunk = &body[start..end];
        let is_first = i == 0;
        let is_last = i == chunk_count - 1;
        let flags = if is_first && is_last {
            Flags::COMPLETE
        } else if is_first {
            Flags::FIRST
        } else if is_last {
            Flags::LAST
        } else {
            Flags::MIDDLE
        };

        let mut report = vec![0u8; geometry.report_len()];
        report[0] = ReportId::Output as u8;
        #[allow(clippy::cast_possible_truncation)]
        let len = chunk.len() as u8; // Both closed geometries have capacity < 256.
        report[1] = len;
        report[2] = flags;
        report[3..3 + chunk.len()].copy_from_slice(chunk);
        reports.push(report);
    }

    reports
}

/// Encode a logical message (protobuf payload + message-type tag) into one or
/// more 129-byte HID output reports ready for `hidapi::HidDevice::write`.
///
/// Appends the 8-byte trailer (`[message_type u16 LE]` + six zero bytes) to
/// `payload`, splits the result into 126-byte chunks, and wraps each chunk as
/// `[OUT_REPORT_ID][len][flags][chunk, zero-padded to 126]`. The first chunk's
/// flags carry `FIRST`, the last `LAST` (a single-report message carries both
/// as `COMPLETE`).
#[must_use]
pub fn encode_message(message_type: u16, payload: &[u8]) -> Vec<Vec<u8>> {
    // Build the full body: protobuf payload ++ 8-byte trailer.
    let mut body = Vec::with_capacity(payload.len() + crate::message::TRAILER_LEN);
    body.extend_from_slice(payload);
    body.extend_from_slice(&message_type.to_le_bytes());
    body.extend_from_slice(&[0u8; crate::message::TRAILER_LEN - 2]);

    encode_reports(HidReportGeometry::QUAD_CORTEX, &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(flags: u8, data: &[u8]) -> Frame {
        Frame {
            flags: Flags(flags),
            data: data.to_vec(),
        }
    }

    #[test]
    fn complete_frame_round_trips() {
        let mut r = FrameReassembler::new();
        let body = r.feed(&frame(Flags::COMPLETE, b"hello")).unwrap();
        assert_eq!(body.as_deref(), Some(b"hello".as_slice()));
    }

    #[test]
    fn multi_frame_assembles_in_order() {
        let mut r = FrameReassembler::new();
        assert!(r.feed(&frame(Flags::FIRST, b"hel")).unwrap().is_none());
        assert!(r.feed(&frame(Flags::MIDDLE, b"lo ")).unwrap().is_none());
        assert_eq!(
            r.feed(&frame(Flags::LAST, b"world")).unwrap().as_deref(),
            Some(b"hello world".as_slice())
        );
    }

    #[test]
    fn middle_without_first_errors() {
        let mut r = FrameReassembler::new();
        let err = r.feed(&frame(Flags::MIDDLE, b"oops")).unwrap_err();
        assert!(matches!(err, crate::Error::Framing(_)));
    }

    #[test]
    fn last_without_first_errors() {
        let mut r = FrameReassembler::new();
        let err = r.feed(&frame(Flags::LAST, b"oops")).unwrap_err();
        assert!(matches!(err, crate::Error::Framing(_)));
    }

    #[test]
    fn flags_decode_correctly() {
        assert!(Flags(Flags::FIRST).is_first());
        assert!(Flags(Flags::LAST).is_last());
        assert!(Flags(Flags::COMPLETE).is_complete());
        assert!(Flags(Flags::MIDDLE).is_middle());
    }

    #[test]
    fn encode_then_decode_round_trips() {
        let payload = b"hello world this is a test payload";
        let reports = encode_message(10, payload);
        assert!(!reports.is_empty());
        // Reassemble by feeding each report through Frame::parse + reassembler.
        let mut r = FrameReassembler::new();
        let mut body = None;
        for report in &reports {
            let frame = Frame::parse(report).unwrap();
            if frame.flags.is_first() {
                r.reset();
            }
            if let Some(complete) = r.feed(&frame).unwrap() {
                body = Some(complete);
            }
        }
        let body = body.expect("a complete message was produced");
        let msg = crate::message::Message::parse(&body).unwrap();
        assert_eq!(msg.message_type, 10);
        assert_eq!(msg.body.as_ref(), payload);
    }

    #[test]
    fn encode_single_frame_is_complete() {
        let reports = encode_message(10, b"short");
        assert_eq!(reports.len(), 1);
        let frame = Frame::parse(&reports[0]).unwrap();
        assert!(frame.flags.is_complete());
    }

    #[test]
    fn encode_multi_frame_sets_flags() {
        // A payload larger than CHUNK_SIZE forces multiple frames.
        let payload = vec![0xAB; CHUNK_SIZE + 10];
        let reports = encode_message(10, &payload);
        assert!(reports.len() > 1);
        let first = Frame::parse(&reports[0]).unwrap();
        let last = Frame::parse(&reports[reports.len() - 1]).unwrap();
        assert!(first.flags.is_first());
        assert!(!first.flags.is_last());
        assert!(last.flags.is_last());
        assert!(!last.flags.is_first());
    }

    #[test]
    fn legacy_constants_remain_quad_geometry() {
        assert_eq!(HID_BODY_LEN, 128);
        assert_eq!(HID_REPORT_LEN, 129);
        assert_eq!(CHUNK_SIZE, 126);
        assert!(
            encode_message(10, b"short")
                .iter()
                .all(|report| report.len() == HID_REPORT_LEN)
        );
    }

    #[test]
    fn closed_geometries_have_measured_dimensions() {
        assert_eq!(HidReportGeometry::QUAD_CORTEX.body_len(), 128);
        assert_eq!(HidReportGeometry::QUAD_CORTEX.report_len(), 129);
        assert_eq!(HidReportGeometry::QUAD_CORTEX.data_capacity(), 126);
        assert_eq!(HidReportGeometry::NANO_CORTEX.body_len(), 64);
        assert_eq!(HidReportGeometry::NANO_CORTEX.report_len(), 65);
        assert_eq!(HidReportGeometry::NANO_CORTEX.data_capacity(), 62);
    }

    #[test]
    fn raw_report_boundaries_follow_selected_geometry() {
        let empty = encode_reports(HidReportGeometry::NANO_CORTEX, &[]);
        assert_eq!(empty.len(), 1);
        assert_eq!(empty[0].len(), 65);
        assert_eq!(empty[0][1], 0);
        assert_eq!(empty[0][2], Flags::COMPLETE);
        assert_eq!(Frame::parse(&empty[0]).unwrap().data, Vec::<u8>::new());
        assert_eq!(
            encode_reports(HidReportGeometry::QUAD_CORTEX, &[0; 126]).len(),
            1
        );
        assert_eq!(
            encode_reports(HidReportGeometry::QUAD_CORTEX, &[0; 127]).len(),
            2
        );
        assert_eq!(
            encode_reports(HidReportGeometry::NANO_CORTEX, &[0; 62]).len(),
            1
        );
        assert_eq!(
            encode_reports(HidReportGeometry::NANO_CORTEX, &[0; 63]).len(),
            2
        );
    }

    #[test]
    fn nano_state_sized_body_reassembles_from_nine_reports() {
        let body: Vec<u8> = (0u16..546)
            .map(|i| u8::try_from(i % 251).unwrap())
            .collect();
        let reports = encode_reports(HidReportGeometry::NANO_CORTEX, &body);
        assert_eq!(reports.len(), 9);
        assert!(reports.iter().all(|report| report.len() == 65));
        assert_eq!(reports[0][1], 62);
        assert_eq!(reports[8][1], 50);

        let frames: Vec<Frame> = reports
            .iter()
            .map(|report| Frame::parse(report).unwrap())
            .collect();
        assert_eq!(frames[0].flags, Flags(Flags::FIRST));
        assert!(
            frames[1..8]
                .iter()
                .all(|frame| frame.flags == Flags(Flags::MIDDLE))
        );
        assert_eq!(frames[8].flags, Flags(Flags::LAST));

        let mut decoder = FrameReassembler::new();
        let mut decoded_body = None;
        for frame in &frames {
            if let Some(complete) = decoder.feed(frame).unwrap() {
                decoded_body = Some(complete);
            }
        }
        assert_eq!(decoded_body.as_deref(), Some(body.as_slice()));
    }
}
