// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The seam between the session and the wire.
//!
//! [`Session`](crate::Session) needs exactly two operations from the device -
//! write a report, read a report - so that is all this trait exposes. Keeping
//! it that narrow is what makes it substitutable: a fake has to be honest
//! about two things rather than about a USB stack.
//!
//! Why it exists: everything above the transport was untestable without
//! hardware, which left the RX loop, the handshake, the writer gate and the
//! whole daemon uncovered. Those are precisely where this project's expensive
//! bugs have lived - a reader starving a writer, a keepalive too slow to keep
//! the device pushing - and none of them are visible from the outside until
//! something has already gone wrong on real hardware.
//!
//! @see spec/roadmap.md ENG-001.y

/// A bidirectional HID report channel.
///
/// `Send` but not `Sync`: `hidapi::HidDevice` is `!Sync`, so the session
/// shares one behind a mutex and this mirrors that constraint rather than
/// hiding it.
pub trait HidLink: Send {
    /// Write one report. The caller has already framed it.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::Hid`] on a transport failure. The Quad session
    /// ignores this because that device deliberately stalls every write;
    /// device-neutral callers must apply their selected device's write policy.
    fn write(&self, report: &[u8]) -> crate::Result<usize>;

    /// Read one report, returning 0 on timeout.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::Hid`] on a transport failure. A timeout is
    /// `Ok(0)`, not an error - it is the ordinary state of a quiet device.
    fn read_timeout(&self, buf: &mut [u8], timeout_ms: i32) -> crate::Result<usize>;
}

/// Read and reassemble one complete logical message from a HID report channel.
///
/// This stops at the framing boundary. Quad callers decode the eight-byte
/// trailer with [`crate::Message`]; Nano callers retain their distinct
/// command-specific four-byte footer codec.
///
/// # Errors
///
/// Returns a transport or framing error, or [`crate::Error::ReadTimeout`] when
/// no complete message arrives before `timeout`.
pub(crate) fn read_message(
    link: &impl HidLink,
    geometry: crate::framing::HidReportGeometry,
    timeout: std::time::Duration,
) -> crate::Result<Vec<u8>> {
    let deadline = std::time::Instant::now() + timeout;
    let mut reassembler = crate::framing::FrameReassembler::new();
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(crate::Error::ReadTimeout(timeout));
        }
        let mut report = vec![0; geometry.report_len()];
        let timeout_ms =
            i32::try_from(remaining.as_millis().min(i32::MAX as u128)).unwrap_or(i32::MAX);
        let read = link.read_timeout(&mut report, timeout_ms)?;
        if read == 0 {
            continue;
        }
        report.truncate(read);
        if let Some(message) = reassembler.feed(&crate::framing::Frame::parse(&report)?)? {
            return Ok(message);
        }
    }
}

#[cfg(feature = "hid")]
impl HidLink for hidapi::HidDevice {
    fn write(&self, report: &[u8]) -> crate::Result<usize> {
        hidapi::HidDevice::write(self, report).map_err(|e| crate::Error::Hid(e.to_string()))
    }

    fn read_timeout(&self, buf: &mut [u8], timeout_ms: i32) -> crate::Result<usize> {
        hidapi::HidDevice::read_timeout(self, buf, timeout_ms)
            .map_err(|e| crate::Error::Hid(e.to_string()))
    }
}

#[cfg(any(test, feature = "test-support"))]
pub use fake::FakeLink;

#[cfg(any(test, feature = "test-support"))]
pub mod fake {
    //! A [`HidLink`](super::HidLink) backed by queues instead of a device.

    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// A scriptable stand-in for the device.
    ///
    /// Reads drain a queue the test fills; writes are recorded for the test to
    /// assert on. A read finds the queue empty exactly as often as a real
    /// device is quiet, which is the behaviour that matters: the RX loop
    /// spends most of its life timing out, and code that only ever saw data
    /// would not exercise the paths where the bugs are.
    #[derive(Clone, Default)]
    pub struct FakeLink {
        inbound: Arc<Mutex<VecDeque<Vec<u8>>>>,
        outbound: Arc<Mutex<Vec<Vec<u8>>>>,
        /// How long a read blocks before reporting a timeout, so a test can
        /// keep the RX thread genuinely busy rather than spinning.
        read_delay: Duration,
        /// When set, every read returns this immediately and the queue is
        /// never consulted.
        saturate: Option<Arc<Vec<u8>>>,
    }

    impl FakeLink {
        /// A fake with nothing queued and a short read delay.
        #[must_use]
        pub fn new() -> Self {
            Self {
                read_delay: Duration::from_millis(1),
                ..Self::default()
            }
        }

        /// Return this report from every read, without ever going quiet.
        ///
        /// Reproduces a device with a backlog, which is the only condition
        /// under which a reader starves a writer: reads complete instantly,
        /// so the RX loop reacquires the device lock the moment it releases
        /// it. A fake that ever went quiet would let the writer through by
        /// luck and the test would pass against the bug.
        #[must_use]
        pub fn saturated(mut self, report: Vec<u8>) -> Self {
            self.saturate = Some(Arc::new(report));
            self
        }

        /// Make every read take this long before finding nothing.
        ///
        /// Used to reproduce a busy reader: the writer-starvation bug only
        /// appears when reads return promptly and the loop reacquires the
        /// device lock immediately, so a test for it needs control here.
        #[must_use]
        pub fn with_read_delay(mut self, delay: Duration) -> Self {
            self.read_delay = delay;
            self
        }

        /// Queue a report for the session to read.
        /// # Panics
        ///
        /// If a previous test thread panicked while holding the queue.
        pub fn push_inbound(&self, report: Vec<u8>) {
            self.inbound.lock().unwrap().push_back(report);
        }

        /// Every report the session has written so far.
        /// # Panics
        ///
        /// If a previous test thread panicked while holding the log.
        #[must_use]
        pub fn written(&self) -> Vec<Vec<u8>> {
            self.outbound.lock().unwrap().clone()
        }

        /// How many reports the session has written.
        /// # Panics
        ///
        /// If a previous test thread panicked while holding the log.
        #[must_use]
        pub fn write_count(&self) -> usize {
            self.outbound.lock().unwrap().len()
        }

        /// How many reports are still waiting to be read.
        /// # Panics
        ///
        /// If a previous test thread panicked while holding the queue.
        #[must_use]
        pub fn pending_inbound(&self) -> usize {
            self.inbound.lock().unwrap().len()
        }
    }

    impl super::HidLink for FakeLink {
        fn write(&self, report: &[u8]) -> crate::Result<usize> {
            self.outbound.lock().unwrap().push(report.to_vec());
            Ok(report.len())
        }

        fn read_timeout(&self, buf: &mut [u8], _timeout_ms: i32) -> crate::Result<usize> {
            if let Some(report) = &self.saturate {
                let n = report.len().min(buf.len());
                buf[..n].copy_from_slice(&report[..n]);
                return Ok(n);
            }
            if let Some(report) = self.inbound.lock().unwrap().pop_front() {
                let n = report.len().min(buf.len());
                buf[..n].copy_from_slice(&report[..n]);
                return Ok(n);
            }
            // Quiet, not broken.
            std::thread::sleep(self.read_delay);
            Ok(0)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{FakeLink, read_message};
    use crate::framing::{HidReportGeometry, encode_reports};

    #[test]
    fn reads_one_complete_multi_report_message() {
        let link = FakeLink::new();
        let body = vec![0x5a; HidReportGeometry::NANO_CORTEX.data_capacity() + 1];
        for report in encode_reports(HidReportGeometry::NANO_CORTEX, &body) {
            link.push_inbound(report);
        }

        assert_eq!(
            read_message(
                &link,
                HidReportGeometry::NANO_CORTEX,
                Duration::from_secs(1)
            )
            .unwrap(),
            body
        );
    }

    #[test]
    fn times_out_without_a_complete_message() {
        let link = FakeLink::new().with_read_delay(Duration::from_millis(1));
        let timeout = Duration::from_millis(2);

        assert!(matches!(
            read_message(&link, HidReportGeometry::QUAD_CORTEX, timeout),
            Err(crate::Error::ReadTimeout(actual)) if actual == timeout
        ));
    }
}
