// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Turn a USB capture into Cortex Control messages.
//!
//! Reads the field output of `tshark` (see `s/usb-decode`, which drives this)
//! and prints one line per reassembled message, in the same shape as
//! `CORTEX_TRACE`.
//!
//! The framing, trailer and gzip handling all come from the crate rather than
//! being reimplemented here. That is the point: a decoder that parsed the
//! wire its own way could disagree with the client and would then be worse
//! than useless, because it would be trusted while wrong.
//!
//! @see spec/roadmap.md ENG-005.2

use std::io::BufRead;

use anyhow::Result;
use cortex_rs::framing::{Frame, FrameReassembler};
use cortex_rs::message::Message;
use cortex_rs::proto::cortex_message_type::Enum as MessageType;

/// One HID report: the report ID plus a 128-byte body.
///
/// Fixed by the protocol, which is what lets a capture be cut into reports
/// without trusting how the transfers were chopped up.
const HID_REPORT_LEN: usize = 129;

/// Which way a report was travelling.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    /// Device to host: interrupt IN on endpoint 0x81.
    In,
    /// Host to device: a `SET_REPORT` on the control endpoint.
    Out,
}

impl Direction {
    fn label(self) -> &'static str {
        match self {
            Self::In => "IN ",
            Self::Out => "OUT",
        }
    }
}

/// Reassembly state for one direction.
///
/// The two directions are kept apart deliberately. They interleave on the bus
/// and share no sequence numbering, so a single reassembler would splice one
/// side's FIRST onto the other's LAST and produce plausible nonsense.
#[derive(Default)]
struct Stream {
    /// Bytes seen but not yet consumed as whole reports.
    buffer: Vec<u8>,
    /// Reports fed into the current message, for the report count.
    reports: usize,
}

/// Decode a stream of `tshark` field lines.
///
/// Each line is tab-separated: relative time, endpoint address, then the two
/// candidate payload fields. Inbound interrupt transfers carry their bytes in
/// `usb.capdata`; outbound control transfers carry theirs in
/// `usb.data_fragment`. Only one is ever populated, so the decoder takes
/// whichever it finds rather than caring which kind of transfer it was.
///
/// # Errors
///
/// Returns an error only if the input cannot be read. Malformed lines are
/// skipped: a capture is a recording of reality, and reality includes noise
/// from other devices and reports truncated by the end of the capture.
pub fn decode_stream(reader: impl BufRead, quiet: bool, verbose: bool) -> Result<()> {
    let mut reassemblers = [FrameReassembler::new(), FrameReassembler::new()];
    let mut streams = [Stream::default(), Stream::default()];
    let mut messages = 0usize;
    let mut skipped = 0usize;
    let mut resyncs = 0usize;

    for line in reader.lines() {
        let line = line?;
        let mut fields = line.split('\t');
        let Some(time) = fields.next().and_then(|t| t.parse::<f64>().ok()) else {
            continue;
        };
        let endpoint = fields.next().unwrap_or_default();

        // Three candidate payload fields, because which one holds the bytes
        // depends on how much Wireshark worked out about the device:
        //
        // - `usb.data_fragment` for control transfers (the SET_REPORT writes)
        // - `usbhid.data` once it has seen the descriptors and knows the
        //   interface is HID, which it does when the capture includes the
        //   device enumerating
        // - `usb.capdata` when it does not, and the payload stays generic
        //
        // The same unit yields different fields in different captures, so
        // taking whichever is populated is the only thing that reads both.
        // Asking for one field and getting nothing is indistinguishable from
        // a device that said nothing - which is exactly how a whole capture
        // of the official client first appeared to be one-sided.
        let hex = fields
            .by_ref()
            .take(3)
            .find(|f| !f.is_empty())
            .unwrap_or_default();
        if hex.is_empty() {
            continue;
        }
        let Some(bytes) = parse_hex(hex) else {
            skipped += 1;
            continue;
        };

        // Endpoint 0x81 is the interrupt IN the device pushes on; anything
        // else here is a control write from the host.
        let direction = if endpoint.trim() == "0x81" {
            Direction::In
        } else {
            Direction::Out
        };
        let index = usize::from(direction == Direction::Out);
        let stream = &mut streams[index];
        let reassembler = &mut reassemblers[index];

        stream.buffer.extend_from_slice(&bytes);

        // Cut the stream into fixed-size reports rather than trusting one
        // transfer to hold exactly one report.
        //
        // It does when the host owns the device directly, but not when the
        // unit is passed through to a VM: QEMU splits every 129-byte report
        // into a 128-byte transfer plus a 1-byte transfer, so a per-transfer
        // parser sees a truncated report followed by a stray byte and
        // reassembles nothing at all. Buffering makes the split invisible and
        // handles both shapes without having to know which produced the
        // capture.
        while stream.buffer.len() >= HID_REPORT_LEN {
            // Resync if the stream is not on a report boundary, which happens
            // whenever a capture starts mid-report. Dropping a byte at a time
            // recovers alignment; assuming alignment would yield frames that
            // parse cleanly and mean something else entirely.
            if !matches!(stream.buffer[0], 0x01 | 0x02) {
                stream.buffer.remove(0);
                resyncs += 1;
                continue;
            }
            let report: Vec<u8> = stream.buffer.drain(..HID_REPORT_LEN).collect();

            let Ok(frame) = Frame::parse(&report) else {
                skipped += 1;
                continue;
            };
            stream.reports += 1;

            match reassembler.feed(&frame) {
                Ok(Some(body)) => {
                    let reports = std::mem::take(&mut stream.reports);
                    match Message::decode(&body) {
                        Ok((msg, gzipped)) => {
                            messages += 1;
                            if !quiet {
                                println!(
                                    "  {:9.3}  {}  {:<24} {:>7} B{}  {} report{}",
                                    time,
                                    direction.label(),
                                    type_name(msg.message_type),
                                    msg.body.len(),
                                    if gzipped { "  gzip" } else { "      " },
                                    reports,
                                    if reports == 1 { "" } else { "s" }
                                );
                                if verbose {
                                    for field in dump_fields(&msg.body) {
                                        println!("                {field}");
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            skipped += 1;
                            if !quiet {
                                println!("  {time:9.3}  {}  <undecodable: {e}>", direction.label());
                            }
                        }
                    }
                }
                Ok(None) => {}
                Err(_) => {
                    // A partial message interrupted by the start of another,
                    // most often because the capture began mid-message.
                    reassembler.reset();
                    stream.reports = 0;
                    skipped += 1;
                }
            }
        }
    }

    eprintln!("decoded {messages} messages ({skipped} skipped, {resyncs} resync bytes)");
    Ok(())
}

/// Describe a protobuf body field by field, without knowing its schema.
///
/// Generic on purpose. A per-type match over the 70-odd message types would
/// decode more prettily and would go blank on exactly the messages worth
/// looking at - the ones the official client sends that we do not model. The
/// wire format carries field numbers and types regardless, which is enough to
/// compare two clients' requests byte for byte.
///
/// Values are shown, not just counted, because the question this answers is
/// usually "how does their request differ from ours".
fn dump_fields(body: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < body.len() {
        let Some((key, used)) = varint(&body[i..]) else {
            out.push(format!("<undecodable at byte {i}>"));
            break;
        };
        i += used;
        let field = key >> 3;
        let wire = key & 7;
        match wire {
            0 => {
                let Some((v, used)) = varint(&body[i..]) else {
                    out.push(format!("field {field}: <truncated varint>"));
                    break;
                };
                i += used;
                out.push(format!("field {field}: varint {v}"));
            }
            1 => {
                if i + 8 > body.len() {
                    out.push(format!("field {field}: <truncated 64-bit>"));
                    break;
                }
                let bytes: [u8; 8] = body[i..i + 8].try_into().unwrap();
                i += 8;
                out.push(format!(
                    "field {field}: 64-bit {} (f64 {})",
                    u64::from_le_bytes(bytes),
                    f64::from_le_bytes(bytes)
                ));
            }
            2 => {
                let Some((len, used)) = varint(&body[i..]) else {
                    out.push(format!("field {field}: <truncated length>"));
                    break;
                };
                i += used;
                let len = usize::try_from(len).unwrap_or(usize::MAX);
                if i + len > body.len() {
                    out.push(format!("field {field}: <truncated {len}-byte value>"));
                    break;
                }
                let value = &body[i..i + len];
                i += len;
                // Show text as text. Most length-delimited fields here are
                // names, keys and paths, and reading those as hex would hide
                // the most useful thing in the message.
                match std::str::from_utf8(value) {
                    Ok(t) if t.chars().all(|c| !c.is_control()) && !t.is_empty() => {
                        out.push(format!("field {field}: \"{t}\""));
                    }
                    _ => out.push(format!("field {field}: {len} bytes (nested or binary)")),
                }
            }
            5 => {
                if i + 4 > body.len() {
                    out.push(format!("field {field}: <truncated 32-bit>"));
                    break;
                }
                let bytes: [u8; 4] = body[i..i + 4].try_into().unwrap();
                i += 4;
                out.push(format!(
                    "field {field}: 32-bit {} (f32 {})",
                    u32::from_le_bytes(bytes),
                    f32::from_le_bytes(bytes)
                ));
            }
            other => {
                // 3 and 4 are the deprecated group markers; anything else
                // means the stream is not what we think it is, and guessing
                // further would invent structure.
                out.push(format!("field {field}: unsupported wire type {other}"));
                break;
            }
        }
    }
    out
}

/// Read one protobuf varint, returning its value and the bytes consumed.
fn varint(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut value = 0u64;
    for (i, b) in bytes.iter().take(10).enumerate() {
        value |= u64::from(b & 0x7f) << (7 * i);
        if b & 0x80 == 0 {
            return Some((value, i + 1));
        }
    }
    None
}

/// The `CortexMessageType` name for a trailer tag.
///
/// Unknown tags are shown numerically rather than as `Undefined`: a tag we do
/// not recognise is exactly the interesting case when reading a capture of
/// the official client, and collapsing every one of them onto the same name
/// would hide the thing worth noticing.
fn type_name(tag: u16) -> String {
    MessageType::try_from(i32::from(tag)).map_or_else(
        |_| format!("<unknown {tag}>"),
        |mt| {
            if mt == MessageType::Undefined {
                format!("<unknown {tag}>")
            } else {
                format!("{mt:?}")
            }
        },
    )
}

/// Parse a hex string, tolerating the colon separators some `tshark` builds
/// emit.
fn parse_hex(s: &str) -> Option<Vec<u8>> {
    let clean: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if clean.is_empty() || clean.len() % 2 != 0 {
        return None;
    }
    (0..clean.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&clean[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parses_with_and_without_separators() {
        assert_eq!(parse_hex("012ac0"), Some(vec![0x01, 0x2a, 0xc0]));
        assert_eq!(parse_hex("01:2a:c0"), Some(vec![0x01, 0x2a, 0xc0]));
    }

    #[test]
    fn odd_length_hex_is_rejected_rather_than_truncated() {
        // Truncating would silently shift every subsequent byte, producing a
        // frame that parses but means something else.
        assert_eq!(parse_hex("012"), None);
        assert_eq!(parse_hex(""), None);
    }

    #[test]
    fn an_unrecognised_tag_keeps_its_number() {
        // 9999 is not in the recovered schema. The number is the useful part
        // when reading a capture of a client we do not control.
        assert_eq!(type_name(9999), "<unknown 9999>");
    }

    #[test]
    fn fields_are_described_without_a_schema() {
        // field 1 varint 2; field 2 string "hi"
        let body = [0x08, 0x02, 0x12, 0x02, b'h', b'i'];
        let got = dump_fields(&body);
        assert_eq!(got[0], "field 1: varint 2");
        assert_eq!(got[1], "field 2: \"hi\"");
    }

    #[test]
    fn a_truncated_field_is_reported_not_guessed() {
        // Claims a 9-byte string but supplies 2. Inventing the rest would be
        // worse than saying so.
        let body = [0x12, 0x09, b'h', b'i'];
        assert!(dump_fields(&body)[0].contains("truncated"));
    }

    #[test]
    fn a_report_split_across_transfers_still_decodes() {
        // The case that matters for a VM capture: QEMU delivers a 129-byte
        // report as 128 bytes plus 1 byte. Split across lines, the decoder
        // must still see one report.
        let mut report = [0u8; HID_REPORT_LEN];
        report[0] = 0x01; // report ID
        report[2] = 0xC0; // FIRST | LAST
        // Payload is an 8-byte trailer and no body, so the type tag is the
        // two zero bytes at its start.
        report[1] = 8;

        let first = hex(&report[..128]);
        let rest = hex(&report[128..]);
        let input = format!("1.0\t0x81\t{first}\t\n1.1\t0x81\t{rest}\t\n");

        // Decoding must not error, and must consume both halves as one report.
        decode_stream(input.as_bytes(), true, false).unwrap();
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}
