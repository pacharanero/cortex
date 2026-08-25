// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Identifies which Neural DSP device a connection is talking to.
//!
//! The `VersionMessage.DeviceType` enum in the recovered Quad Cortex schema has
//! `QC = 0` and `ATMA = 1`; `ATMA` is the internal codename for the Nano
//! Cortex. Hardware probing established that the Nano shares the HID frame
//! shape but uses 65-byte reports and a Nano-specific four-byte application
//! footer rather than the Quad's 129-byte reports and eight-byte trailer. Nano
//! the separate message codec remains future work - see AGENTS.md.
//!
//! @see spec/130-domain-model/spec.md
//! @see spec/100-transport/spec.md [FR-1]

use serde::{Deserialize, Serialize};

/// The kind of Neural DSP device on the other end of the USB connection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    /// The Neural DSP Quad Cortex. The primary verification target.
    #[default]
    QuadCortex,
    /// The Neural DSP Nano Cortex (internal codename `ATMA`). Its HID framing,
    /// state exchange and selected non-persistent amp/bypass edits are
    /// hardware-verified; FX and wider application support remain provisional.
    NanoCortex,
}

impl DeviceKind {
    /// Returns the USB vendor/product ID pair this client looks for.
    ///
    /// The Quad Cortex presents as `152a:880a` on HID interface 5. The Nano
    /// Cortex's hardware-verified VID:PID is `152a:88e7`.
    #[must_use]
    pub const fn vid_pid(self) -> (u16, u16) {
        match self {
            // Quad Cortex: verified against CorOS 4.0.1 / firmware d14e.
            DeviceKind::QuadCortex => (0x152A, 0x880A),
            DeviceKind::NanoCortex => (0x152A, 0x88E7),
        }
    }

    /// The fixed HID report dimensions measured for this device.
    #[must_use]
    pub const fn report_geometry(self) -> crate::framing::HidReportGeometry {
        match self {
            Self::QuadCortex => crate::framing::HidReportGeometry::QUAD_CORTEX,
            Self::NanoCortex => crate::framing::HidReportGeometry::NANO_CORTEX,
        }
    }

    /// Whether write failures are the device's measured benign status-stage STALL.
    #[must_use]
    pub const fn has_benign_write_stall(self) -> bool {
        matches!(self, Self::QuadCortex)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_profiles_match_hardware_measurements() {
        assert_eq!(DeviceKind::QuadCortex.vid_pid(), (0x152A, 0x880A));
        assert_eq!(DeviceKind::NanoCortex.vid_pid(), (0x152A, 0x88E7));
        assert_eq!(DeviceKind::QuadCortex.report_geometry().report_len(), 129);
        assert_eq!(DeviceKind::NanoCortex.report_geometry().report_len(), 65);
        assert!(DeviceKind::QuadCortex.has_benign_write_stall());
        assert!(!DeviceKind::NanoCortex.has_benign_write_stall());
    }
}
