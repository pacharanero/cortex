// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Identifies which Neural DSP device a connection is talking to.
//!
//! The `VersionMessage.DeviceType` enum in the recovered Quad Cortex schema has
//! `QC = 0` and `ATMA = 1`; `ATMA` is the internal codename for the Nano
//! Cortex. Hardware probing established that the Nano shares the HID frame
//! shape but uses 65-byte reports and a Nano-specific four-byte application
//! footer rather than the Quad's 129-byte reports and eight-byte trailer. Nano
//! support therefore remains a non-matching placeholder until transport
//! geometry and the separate message codec land together - see AGENTS.md.
//!
//! @see spec/130-domain-model/spec.md
//! @see spec/100-transport/spec.md [FR-1]

use serde::{Deserialize, Serialize};

/// The kind of Neural DSP device on the other end of the USB connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeviceKind {
    /// The Neural DSP Quad Cortex. The primary verification target.
    QuadCortex,
    /// The Neural DSP Nano Cortex (internal codename `ATMA`). Its HID framing
    /// and read-only state exchange are hardware-verified; runtime support is
    /// provisional until the Nano-specific codec is implemented.
    NanoCortex,
}

impl DeviceKind {
    /// Returns the USB vendor/product ID pair this client looks for.
    ///
    /// The Quad Cortex presents as `152a:880a` on HID interface 5. The Nano
    /// Cortex's hardware-verified VID:PID is `152a:88e7`, but this method
    /// deliberately returns a non-matching sentinel until its 65-byte framing
    /// and message codec are implemented together.
    #[must_use]
    pub const fn vid_pid(self) -> (u16, u16) {
        match self {
            // Quad Cortex: verified against CorOS 4.0.1 / firmware d14e.
            DeviceKind::QuadCortex => (0x152A, 0x880A),
            // Nano Cortex: fail closed until device-dependent framing is implemented.
            DeviceKind::NanoCortex => (0x152A, 0xFFFF),
        }
    }
}
