// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Identifies which Neural DSP device a connection is talking to.
//!
//! The `VersionMessage.DeviceType` enum in the recovered schema has `QC = 0`
//! and `ATMA = 1`; `ATMA` is the internal codename for the Nano Cortex. The
//! Quad Cortex and Nano Cortex share the same Cortex Control protocol shape
//! (USB HID framing + protobuf-in-trailer), so `cortex-rs` targets both from
//! the start. Device-specific behaviour that is not hardware-verified is
//! labelled provisional - see AGENTS.md.
//!
//! @see spec/130-domain-model/spec.md
//! @see spec/100-transport/spec.md [FR-1]

use serde::{Deserialize, Serialize};

/// The kind of Neural DSP device on the other end of the USB connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeviceKind {
    /// The Neural DSP Quad Cortex. The primary verification target.
    QuadCortex,
    /// The Neural DSP Nano Cortex (internal codename `ATMA`). Provisional -
    /// protocol shape is shared with the Quad Cortex, but Nano-specific
    /// messages and BLE behaviour are not yet hardware-verified by this
    /// project.
    NanoCortex,
}

impl DeviceKind {
    /// Returns the USB vendor/product ID pair this client looks for.
    ///
    /// The Quad Cortex presents as `152a:880a` on HID interface 5. The Nano
    /// Cortex is expected to share the same vendor ID; its product ID will be
    /// recorded here once verified against real hardware.
    #[must_use]
    pub const fn vid_pid(self) -> (u16, u16) {
        match self {
            // Quad Cortex: verified against CorOS 4.0.1 / firmware d14e.
            DeviceKind::QuadCortex => (0x152A, 0x880A),
            // Nano Cortex: TODO record the verified product ID here.
            DeviceKind::NanoCortex => (0x152A, 0xFFFF),
        }
    }
}
