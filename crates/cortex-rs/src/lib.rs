// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! # cortex-rs
//!
//! Low-level Rust crate for the Neural DSP **Quad Cortex** (and, in time, the
//! **Nano Cortex**) over the Cortex Control USB HID protocol. A port of the
//! protocol behaviour established by the MIT-licensed
//! [`stokes-audio/pyquadcortex`](https://github.com/stokes-audio/pyquadcortex)
//! Python library, re-verified against a real Quad Cortex on Linux.
//!
//! **Unofficial.** This project is not affiliated with or endorsed by Neural
//! DSP. "Neural DSP", "Quad Cortex", and "Nano Cortex" are trademarks of
//! Neural DSP Technologies. See `NOTICE` and the README for the full trademark
//! and reverse-engineering-for-interoperability statement.
//!
//! ## Leaf crate
//!
//! `cortex-rs` is a **leaf**: it depends only on what it needs to encode the
//! protocol and the typed domain model. It never depends on a host application
//! or an async runtime, so the same crate can drive:
//!
//! - the `cortex-cli` command-line surface (this workspace),
//! - the `cortex-mcp` MCP server for agentic patch editing (this workspace),
//! - a Tauri desktop GUI backend (planned, see `gui/`),
//! - and any third-party consumer via crates.io.
//!
//! Building with `default-features = false` drops the `hidapi` transport,
//! leaving only the protocol/domain decode surface - useful for tests,
//! analysis tools, and schema introspection without a device present.
//!
//! ## Feature flags
//!
//! - `default = ["hid"]` builds the USB HID transport.
//! - `default-features = false` builds only the protocol/domain surface (no
//!   `hidapi`, no device access).
//!
//! ## Protocol invariants
//!
//! See `AGENTS.md` in the repo root for the authoritative list. The core
//! gotchas this crate encodes:
//!
//! - 128-byte HID body + report ID = 129 bytes at the hidapi boundary.
//! - Flag-driven reassembly (`0x40` FIRST / `0x80` LAST / `0xC0` complete).
//! - The message-type tag is a little-endian `uint16` in the **trailer**,
//!   not a header.
//! - The benign write STALL: `hid_write()` returns `-1` on a write that
//!   worked. Swallow write errors; detect a dead device via read timeouts.
//! - No version field on the wire: a `CorOS` update can silently break things.
//!
//! @see spec/001-overview/spec.md
//! @see spec/001-overview/design.md [DES-ARCH]

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(clippy::all, clippy::pedantic)]

pub mod daemon;
pub mod device;
pub mod framing;
pub mod grid;
pub mod link;
pub mod message;

#[cfg(feature = "hid")]
pub mod catalog;
pub mod client;
#[cfg(feature = "hid")]
pub mod session;
#[cfg(feature = "hid")]
pub mod transport;

/// Typed Rust types generated from the recovered Cortex Control protobuf
/// schema by `prost` at build time.
///
/// The schema was recovered from Cortex Control by
/// `stokes-audio/pyquadcortex` (MIT). See `THIRD-PARTY-NOTICES.md`.
#[allow(missing_docs, clippy::all, clippy::pedantic)]
pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/cortex_protobuf_v2.rs"));
}

#[cfg(feature = "hid")]
pub use catalog::{Catalog, Model, Parameter, ParameterKind};
pub use client::{Placement, QuadCortex};
pub use daemon::{Request, Response};
pub use device::DeviceKind;
pub use framing::{Flags, Frame, FrameReassembler, ReportId};
pub use grid::{Row, Value};
pub use message::Message;
pub use session::ConnectMode;
#[cfg(feature = "hid")]
pub use session::{InboundMessage, Session};
#[cfg(feature = "hid")]
pub use transport::Transport;

/// Crate-level error type. Library errors are a typed enum so callers
/// (including the MCP safety surface) can match on them; the CLI and MCP
/// binaries use `anyhow` for top-level orchestration.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A frame was malformed or the reassembly state machine got into an
    /// unexpected state (e.g. a middle frame without a preceding first).
    #[error("framing error: {0}")]
    Framing(String),

    /// A reassembled message could not be decoded as a Cortex Control
    /// protobuf message.
    #[error("protobuf decode error: {0}")]
    Decode(String),

    /// The 8-byte trailer was missing or did not contain a recognised
    /// `CortexMessageType` tag.
    #[error("trailer error: {0}")]
    Trailer(String),

    /// The device read timed out. Because every write is deliberately
    /// stalled at the USB status stage, a read timeout is the only
    /// reliable signal of a dead or unresponsive device.
    #[error("read timeout after {0:?}")]
    ReadTimeout(std::time::Duration),

    /// Nothing at all has arrived from the device for long enough that the
    /// link must be considered down.
    ///
    /// Distinct from [`Error::ReadTimeout`], and worth the separate variant:
    /// a read timeout means "the answer I wanted did not come", which a busy
    /// but healthy device can produce. This means "the device has stopped
    /// talking altogether", which a kept-alive session never does - measured
    /// at 0 s of silence across a 90 s idle, and 0.11 s for Cortex Control
    /// over the same test.
    ///
    /// That distinction is only true while keepalives are frequent enough.
    /// At a 5 s interval the device stops pushing and healthy sessions do
    /// fall silent, which is how this check came to be built, withdrawn, and
    /// rebuilt. See [`crate::session`] and roadmap PROT-008.6.4.
    ///
    /// Reported in seconds rather than a `Duration` because the source is a
    /// second-resolution counter; anything finer would be false precision.
    #[error("device silent for {0}s (a kept-alive session is never quiet)")]
    DeviceSilent(u64),

    /// A slot name was malformed. Slots are a bank number 1-32 followed by
    /// a letter A-H, e.g. `"28C"`.
    #[error("invalid slot name: {0}")]
    InvalidSlot(String),

    /// A lookup found nothing - e.g. no preset of that name in the setlist.
    /// Distinct from [`Error::ReadTimeout`]: the device answered, and the
    /// answer was "no such thing".
    #[error("not found: {0}")]
    NotFound(String),

    /// A USB HID transport error from the underlying `hidapi` crate.
    #[cfg(feature = "hid")]
    #[error("hid error: {0}")]
    Hid(String),

    /// The device did not accept a block placement.
    ///
    /// The known cause is the preset having no DSP capacity left for that
    /// model. Nothing on the wire says so - every host write is stalled and
    /// there is no per-block error - so this is inferred from the absence of
    /// the device's `Grid` echo naming the cell.
    #[error("block refused: {0}")]
    BlockRefused(String),

    /// A row was addressed that cannot hold what was asked of it - an odd
    /// row for a splitter, or screen row 0, which does not exist.
    #[error("invalid row: {0}")]
    InvalidRow(String),

    /// The device was not found on the USB bus. On Linux, check the udev
    /// rule and that the device is powered on (see README -> Setup).
    #[cfg(feature = "hid")]
    #[error("device not found: {0}")]
    DeviceNotFound(String),
}

/// Crate-level result alias.
pub type Result<T, E = Error> = std::result::Result<T, E>;
