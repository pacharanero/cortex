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

/// Which way a report was travelling.
///
/// The two directions are reassembled separately. They interleave on the bus
/// and share no sequence numbering, so a single reassembler would splice one
/// side's FIRST onto the other's LAST and produce plausible nonsense.
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
pub fn decode_stream(reader: impl BufRead, quiet: bool) -> Result<()> {
    let mut rx_in = FrameReassembler::new();
    let mut rx_out = FrameReassembler::new();
    let mut count_in = 0usize;
    let mut count_out = 0usize;
    let mut messages = 0usize;
    let mut skipped = 0usize;

    for line in reader.lines() {
        let line = line?;
        let mut fields = line.split('\t');
        let Some(time) = fields.next().and_then(|t| t.parse::<f64>().ok()) else {
            continue;
        };
        let endpoint = fields.next().unwrap_or_default();
        let capdata = fields.next().unwrap_or_default();
        let fragment = fields.next().unwrap_or_default();

        let hex = if capdata.is_empty() {
            fragment
        } else {
            capdata
        };
        if hex.is_empty() {
            continue;
        }
        let Some(bytes) = parse_hex(hex) else {
            skipped += 1;
            continue;
        };

        // Endpoint 0x81 is the interrupt IN the device pushes on; anything
        // else here is our own control write.
        let direction = if endpoint.trim() == "0x81" {
            Direction::In
        } else {
            Direction::Out
        };
        let (reassembler, count) = match direction {
            Direction::In => (&mut rx_in, &mut count_in),
            Direction::Out => (&mut rx_out, &mut count_out),
        };

        let Ok(frame) = Frame::parse(&bytes) else {
            skipped += 1;
            continue;
        };
        *count += 1;

        match reassembler.feed(&frame) {
            Ok(Some(body)) => {
                let reports = *count;
                *count = 0;
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
                // A partial message interrupted by the start of another. Most
                // often the capture began mid-message.
                reassembler.reset();
                *count = 0;
                skipped += 1;
            }
        }
    }

    eprintln!("decoded {messages} messages ({skipped} reports skipped)");
    Ok(())
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
        // frame that parses but means something else entirely.
        assert_eq!(parse_hex("012"), None);
        assert_eq!(parse_hex(""), None);
    }

    #[test]
    fn an_unrecognised_tag_keeps_its_number() {
        // 9999 is not in the recovered schema. The number is the useful part
        // when reading a capture of a client we do not control.
        assert_eq!(type_name(9999), "<unknown 9999>");
    }
}
