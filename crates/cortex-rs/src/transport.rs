// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The hardware-verified USB HID transport substrate for Cortex devices.
//!
//! Nano Cortex shares the HID frame shape but uses 65-byte reports and a
//! different application envelope. Raw framed I/O supports both geometries;
//! the synchronous Quad-envelope request path remains explicitly Quad-only.
//!
//! Encapsulates the two non-obvious behaviours documented in
//! `quad-cortex-linux-editor-and-protocol.md`:
//!
//! - **The benign write STALL.** Every host-to-device `SET_REPORT` is acted
//!   upon and *then* deliberately stalled at the USB status stage, so
//!   `hid_write()` returns `-1` on a write that worked. [`Transport::write`]
//!   swallows these errors; a dead device is detected via a read timeout
//!   instead.
//! - **Exclusive HID access.** One owning process per device, not one
//!   connection per call. [`Transport::open`] takes the interface
//!   exclusively; the MCP server especially must hold a single connection
//!   for its lifetime.
//!
//! @see spec/100-transport/spec.md [FR-1] [FR-5] [FR-6]
//! @see spec/100-transport/design.md [DES-STALL] [DES-EXCLUSIVE]

use std::time::Duration;

use crate::device::DeviceKind;
pub use crate::framing::{HID_BODY_LEN, HID_REPORT_LEN};

/// Default read timeout. The write STALL means this - not a write error -
/// is the signal of a dead or unresponsive device.
pub const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(2);

/// Wraps a single owning connection to a Neural DSP device over USB HID.
///
/// Open with [`Transport::open`], then [`Transport::write`] host-to-device
/// messages and [`Transport::read`] device-to-host frames. The connection
/// holds the HID interface exclusively; drop the [`Transport`] to release
/// it.
pub struct Transport {
    device: hidapi::HidDevice,
    kind: DeviceKind,
}

impl Transport {
    /// Open the first matching Neural DSP device on the USB bus.
    ///
    /// On Linux this requires a udev rule granting the locally logged-in user
    /// access to `/dev/hidraw*`; otherwise it will return
    /// [`crate::Error::DeviceNotFound`]. See the README setup section.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::DeviceNotFound`] if no matching device is on
    /// the bus, or [`crate::Error::Hid`] if `hidapi` fails to enumerate or
    /// open the device.
    pub fn open(kind: DeviceKind) -> crate::Result<Self> {
        let api = hidapi::HidApi::new().map_err(|e| crate::Error::Hid(e.to_string()))?;
        let (vid, pid) = kind.vid_pid();
        let device = api
            .device_list()
            .find(|info| info.vendor_id() == vid && info.product_id() == pid)
            .ok_or_else(|| {
                crate::Error::DeviceNotFound(format!(
                    "no Neural DSP device with VID:PID {vid:04x}:{pid:04x} on the bus"
                ))
            })?;
        let device = api
            .open_path(device.path())
            .map_err(|e| crate::Error::Hid(e.to_string()))?;
        Ok(Self { device, kind })
    }

    /// Consume the transport and return the underlying `HidDevice`. Used by
    /// the session layer to share the device handle between the RX thread
    /// and the foreground via `Arc<HidDevice>`.
    #[must_use]
    pub fn into_device(self) -> hidapi::HidDevice {
        self.device
    }

    /// Device profile retained by this connection.
    #[must_use]
    pub const fn device_kind(&self) -> DeviceKind {
        self.kind
    }

    /// Send an already-formed host-to-device body using this device's report
    /// geometry and the shared FIRST/LAST/COMPLETE/MIDDLE flags.
    ///
    /// Quad write errors are swallowed because its benign status-stage STALL
    /// makes failure the expected success result. Nano writes return normally,
    /// so their errors propagate. Neither path retries a report.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::Hid`] for a Nano write failure. Quad write
    /// failures are swallowed because its status-stage STALL is benign.
    pub fn write(&self, message: &[u8]) -> crate::Result<()> {
        for report in crate::framing::encode_reports(self.kind.report_geometry(), message) {
            validate_write_result(
                self.kind,
                report.len(),
                self.device
                    .write(&report)
                    .map_err(|error| error.to_string()),
            )?;
        }
        Ok(())
    }

    /// Read the next device-sized input report. Returns the raw report
    /// (report ID + body) so the caller can feed it to
    /// [`crate::framing::Frame::parse`].
    ///
    /// A timeout is the reliable signal of a dead or unresponsive device,
    /// because writes are deliberately stalled.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::Hid`] on an `hidapi` read error, or
    /// [`crate::Error::ReadTimeout`] if no report arrives within `timeout`
    /// (the canonical dead-device signal).
    pub fn read(&self, timeout: Duration) -> crate::Result<Vec<u8>> {
        let mut buf = vec![0u8; self.kind.report_geometry().report_len()];
        // hidapi takes an `i32` timeout in milliseconds. The duration is
        // bounded by `DEFAULT_READ_TIMEOUT` in practice; clamp to `i32::MAX`
        // to satisfy clippy's cast truncation check.
        let timeout_ms =
            i32::try_from(timeout.as_millis().min(i32::MAX as u128)).unwrap_or(i32::MAX);
        let n = self
            .device
            .read_timeout(&mut buf, timeout_ms)
            .map_err(|e| crate::Error::Hid(e.to_string()))?;
        if n == 0 {
            return Err(crate::Error::ReadTimeout(timeout));
        }
        buf.truncate(n);
        Ok(buf)
    }

    /// Send a message (protobuf payload + message-type tag) and read back the
    /// reassembled reply, decoding the full transport stack: encode -> write
    /// (swallowing the STALL) -> read frames -> reassemble -> strip trailer ->
    /// gzip-decompress if the payload starts with the gzip magic.
    ///
    /// This is the synchronous request/response path used by the CLI's
    /// `version` command. It does not correlate by `request_id` yet (READ
    /// replies carry none); it simply returns the first reassembled message.
    /// The background-RX transport that correlates and dispatches is a later
    /// concern (needed for the full connect handshake and the MCP server).
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::ReadTimeout`] if no reply arrives within
    /// `timeout`, or a framing/trailer error if the reassembled message is
    /// malformed.
    pub fn request(
        &self,
        message_type: u16,
        payload: &[u8],
        timeout: Duration,
    ) -> crate::Result<crate::message::Message> {
        if self.kind != DeviceKind::QuadCortex {
            return Err(crate::Error::UnsupportedDeviceOperation {
                device: self.kind,
                operation: "Quad Cortex message request",
            });
        }
        // Encode and send.
        let reports = crate::framing::encode_message(message_type, payload);
        for report in &reports {
            validate_write_result(
                self.kind,
                report.len(),
                self.device.write(report).map_err(|error| error.to_string()),
            )?;
        }

        // Read and reassemble. A FIRST frame starts a new message; middle
        // frames append; a LAST or COMPLETE frame closes it. A new FIRST
        // arriving mid-partial drops the stale buffer (the device interleaves
        // pushes, so this is routine).
        let mut reassembler = crate::framing::FrameReassembler::new();
        let deadline = std::time::Instant::now() + timeout;

        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Err(crate::Error::ReadTimeout(timeout));
            }

            let report = self.read(remaining)?;
            let frame = crate::framing::Frame::parse(&report)?;

            if frame.flags.is_first() {
                reassembler.reset();
            }

            if let Some(body) = reassembler.feed(&frame)? {
                let mut msg = crate::message::Message::parse(&body)?;

                // Frame-level gzip: the device compresses some payloads
                // (e.g. RecallPreset pushes carrying a full BinaryPreset).
                // The decompressed bytes are the ordinary protobuf message.
                if msg.body.starts_with(&[0x1f, 0x8b]) {
                    use std::io::Read;
                    let mut decoder = flate2::read::GzDecoder::new(&msg.body[..]);
                    let mut decompressed = Vec::new();
                    decoder
                        .read_to_end(&mut decompressed)
                        .map_err(|e| crate::Error::Decode(format!("gzip: {e}")))?;
                    msg.body = bytes::Bytes::from(decompressed);
                }

                return Ok(msg);
            }
        }
    }
}

fn validate_write_result(
    kind: DeviceKind,
    report_len: usize,
    result: Result<usize, String>,
) -> crate::Result<()> {
    match result {
        Ok(written) if written != report_len => Err(crate::Error::Hid(format!(
            "short HID write: wrote {written} of {report_len} bytes"
        ))),
        Ok(_) => Ok(()),
        Err(_) if kind.has_benign_write_stall() => Ok(()),
        Err(error) => Err(crate::Error::Hid(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_results_follow_device_policy_without_retrying() {
        assert!(validate_write_result(DeviceKind::QuadCortex, 129, Err("stall".into())).is_ok());
        assert!(validate_write_result(DeviceKind::NanoCortex, 65, Ok(65)).is_ok());
        assert!(matches!(
            validate_write_result(DeviceKind::NanoCortex, 65, Err("failed".into())),
            Err(crate::Error::Hid(_))
        ));
        assert!(matches!(
            validate_write_result(DeviceKind::NanoCortex, 65, Ok(64)),
            Err(crate::Error::Hid(_))
        ));
        assert!(matches!(
            validate_write_result(DeviceKind::QuadCortex, 129, Ok(128)),
            Err(crate::Error::Hid(_))
        ));
    }
}
