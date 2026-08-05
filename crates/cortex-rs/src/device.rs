// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Identifies which Neural DSP device a connection is talking to.
//!
//! The `VersionMessage.DeviceType` enum in the recovered Quad Cortex schema has
//! `QC = 0` and `ATMA = 1`; `ATMA` is the internal codename for the Nano
//! Cortex. That proves only that this recovered schema names an `ATMA` variant,
//! not that a Nano uses the schema or shares transport compatibility.
//! Third-party observation reports a different HID report size for the Nano,
//! and nobody has shown it speaking this protobuf/trailer protocol. Nano
//! support therefore remains a non-matching placeholder until hardware proves
//! the transport - see AGENTS.md.
//!
//! @see spec/130-domain-model/spec.md
//! @see spec/100-transport/spec.md [FR-1]

use serde::{Deserialize, Serialize};

/// The kind of Neural DSP device on the other end of the USB connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeviceKind {
    /// The Neural DSP Quad Cortex. The primary verification target.
    QuadCortex,
    /// The Neural DSP Nano Cortex (internal codename `ATMA`). Provisional:
    /// neither HID transport compatibility nor Nano-specific messages have
    /// been hardware-verified by this project.
    NanoCortex,
}

impl DeviceKind {
    /// Returns the USB vendor/product ID pair this client looks for.
    ///
    /// The Quad Cortex presents as `152a:880a` on HID interface 5. The Nano
    /// Cortex has a third-party-observed VID:PID of `152a:88e7`, but this method
    /// deliberately returns a non-matching sentinel until the protocol is
    /// verified against real hardware.
    #[must_use]
    pub const fn vid_pid(self) -> (u16, u16) {
        match self {
            // Quad Cortex: verified against CorOS 4.0.1 / firmware d14e.
            DeviceKind::QuadCortex => (0x152A, 0x880A),
            // Nano Cortex: fail closed until this project verifies its transport.
            DeviceKind::NanoCortex => (0x152A, 0xFFFF),
        }
    }
}
