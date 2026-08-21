// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The wire contract between `cortex session start` and its clients.
//!
//! `cortex session start` holds one subscribed session and owns the HID interface;
//! everything else talks to it over local IPC. This module defines what
//! they say to each other, and lives in the crate rather than the CLI so the
//! MCP server and the Tauri backend can speak the same protocol rather than
//! growing their own.
//!
//! ## Why a daemon at all
//!
//! The protocol requires one effective HID owner, so exactly one process
//! can own it. Until now every CLI command opened its own session, which
//! meant paying a handshake per command - measured at 2-4 s on a rested
//! device, and far worse when a subscribed handshake left the device busy.
//! The work itself is 9 ms.
//!
//! Holding one session also makes a cache possible, which is the larger
//! prize: a subscribed session is told when the PLAYER changes something on
//! the hardware (confirmed - a knob sweep pushes sparse `Grid` updates), so
//! cached state can be kept true rather than merely fast.
//!
//! ## Shape
//!
//! Line-delimited JSON, one request per line, one response per line. Chosen
//! over a binary framing because it is inspectable with `nc` and `jq`, and
//! the volumes here are tiny compared with the USB traffic underneath.
//!
//! @see spec/roadmap.md PROT-008.6
//! @see spec/140-session/spec.md

use cortex_rs::{DeviceKind, RecallConsent};

/// The daemon protocol version, checked by clients to detect skew
/// after an upgrade leaves an old daemon running.
///
/// Bump this whenever a `Request` variant changes shape or a new variant is
/// added. A client that sees a mismatch refuses with an actionable message
/// rather than sending a request the daemon will misparse.
pub const DAEMON_PROTOCOL_VERSION: u32 = 17;

/// A request from a client to the daemon.
///
/// Deliberately mirrors the `QuadCortex` API rather than the CLI's command
/// surface: the CLI is one client of several, and a protocol shaped around
/// its arguments would not fit the MCP server or the GUI.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Is the daemon alive, is the device healthy, what is it holding.
    Status,
    /// Interrupt reconnect backoff and make the next connection attempt now.
    ReconnectNow,
    /// Read the Nano Cortex's complete fixed-chain state.
    NanoState,
    /// Set one Nano amp control and verify it through a fresh state read.
    NanoSetAmp {
        /// Typed amplifier control.
        control: cortex_rs::nano::NanoAmpControl,
        /// Raw device value, 0-255.
        value: u8,
    },
    /// Set one Nano Gate/FX bypass and verify it through a fresh state read.
    NanoSetBypass {
        /// Typed bypass target (Gate, Pre FX 1-2, Post FX 1-3).
        target: cortex_rs::nano::NanoBypassTarget,
        /// `true` to bypass, `false` to engage.
        bypassed: bool,
    },
    /// Read FX parameter values for one editable Nano slot.
    NanoReadFxParams {
        /// Typed FX slot (Pre FX 1-2, Post FX 1-3).
        slot: cortex_rs::nano::NanoFxSlot,
    },
    /// Set one Nano FX parameter and verify it through a fresh read.
    NanoSetFxParam {
        /// Typed FX slot (Pre FX 1-2, Post FX 1-3).
        slot: cortex_rs::nano::NanoFxSlot,
        /// Parameter index within the slot's model.
        param_index: u8,
        /// Normalized 0.0-1.0 value.
        value: f32,
    },
    /// Device firmware and identity.
    Version,
    /// The active scene index.
    ActiveScene,
    /// Switch the active scene. Zero-based.
    SwitchScene {
        /// Scene index, 0-7. Zero-based; the unit labels these A-H.
        scene: u32,
    },
    /// Set or clear a scene label on the unsaved working copy.
    SetSceneLabel {
        /// Scene index, 0-7.
        scene: u32,
        /// Label text, or `None` for the unit's unlabelled value.
        label: Option<String>,
    },
    /// Set a scene's ARGB colour on the unsaved working copy.
    SetSceneColor {
        /// Scene index, 0-7.
        scene: u32,
        /// ARGB colour as a `u32`.
        color: u32,
    },
    /// Copy or swap two scenes on the unsaved working copy.
    CopyScene {
        /// Source scene index, 0-7.
        from_scene: u32,
        /// Destination scene index, 0-7.
        to_scene: u32,
        /// Exchange both scenes rather than overwriting the destination.
        swap: bool,
    },
    /// The live grid, including unsaved edits.
    CurrentPreset {
        /// Include each block's stored parameter values.
        with_params: bool,
        /// Maximum wait for the live preset push.
        timeout_seconds: u64,
    },
    /// Recall and read a stored preset. Changes what is heard.
    ReadPreset {
        /// Absolute device path of the setlist.
        setlist: String,
        /// Slot name, e.g. `28C`.
        slot: String,
        /// Whether the setlist is the read-only factory library.
        factory: bool,
        /// Include each block's stored parameter values.
        with_params: bool,
        /// Maximum wait for the preset push.
        timeout_seconds: u64,
    },
    /// Recall a stored preset. Changes what is heard.
    RecallPreset {
        /// Absolute device path of the setlist.
        setlist: String,
        /// Slot name, e.g. `28C`.
        slot: String,
        /// Whether the setlist is the read-only factory library.
        factory: bool,
    },
    /// List a setlist.
    ListPresets {
        /// Absolute device path of the setlist.
        setlist: String,
        /// Include empty slots, so a free one can be found.
        include_empty: bool,
        /// Maximum wait for the requested listing.
        timeout_seconds: u64,
    },
    /// Enumerate every folder announced by the device.
    ListFolders {
        /// Collection window; folder announcements arrive as a flood.
        window_seconds: u64,
    },
    /// The device model catalog.
    Catalog {
        /// Maximum wait when the handshake did not cache the catalog.
        timeout_seconds: u64,
    },
    /// List existing Neural Captures from the device library.
    ListCaptures {
        /// Maximum wait for the correlated library listing.
        timeout_seconds: u64,
    },
    /// List existing impulse responses from one device library folder.
    ListIrs {
        /// Device-returned library folder, or `None` for the default IR library.
        folder: Option<String>,
        /// Maximum wait for the correlated library listing.
        timeout_seconds: u64,
    },
    /// Select one exact device-returned Neural Capture in the working grid.
    SetCapture {
        /// Zero-based wire row.
        row: u32,
        /// Zero-based column.
        column: u32,
        /// Exact device-returned capture identity.
        capture: cortex_rs::LibraryEntry,
        /// Place the default Capture block before selection when supplied.
        model: Option<u32>,
        /// Maximum wait for placement and library reads.
        timeout_seconds: u64,
    },
    /// Select one exact device-returned impulse response in the working grid.
    SetIr {
        /// Zero-based wire row.
        row: u32,
        /// Zero-based column.
        column: u32,
        /// Exact device-returned IR identity.
        ir: cortex_rs::LibraryEntry,
        /// IR Loader slot, 0 or 1.
        slot: u32,
        /// Place a compatible IR Loader model before selection when supplied.
        model: Option<u32>,
        /// Device-returned library folder, or `None` for the default IR library.
        folder: Option<String>,
        /// Maximum wait for placement and library reads.
        timeout_seconds: u64,
    },
    /// Write a parameter value.
    ///
    /// Rows travel as a plain zero-based WIRE index, not a [`cortex_rs::Row`].
    /// That type exists to keep wire rows and the 1-4 shown on screen from
    /// being interchangeable, and deriving `Deserialize` on it would let any
    /// integer off a socket become one - defeating the guard on a mistake
    /// that succeeds silently and edits the wrong row.
    SetParam {
        /// Zero-based wire row.
        row: u32,
        /// Zero-based column.
        column: u32,
        /// Parameter index or display name.
        target: cortex_rs::client::ParameterTarget,
        /// Normalised, real-unit, or string input.
        input: cortex_rs::client::ParameterInput,
        /// Write into this scene rather than the active one.
        scene: Option<u32>,
        /// Make the parameter follow scenes first.
        promote: bool,
        /// Maximum wait for any grid/catalog reads needed for resolution.
        timeout_seconds: u64,
    },
    /// Place or replace a block in one grid cell.
    SetBlock {
        /// Zero-based wire row.
        row: u32,
        /// Zero-based column.
        column: u32,
        /// Model catalog id.
        model: u32,
        /// Verify the placement by echo or read-back.
        verify: bool,
        /// Maximum wait for confirmation.
        timeout_seconds: u64,
    },
    /// Bypass or enable a block and verify it by complete live-grid read-back.
    SetBypass {
        /// Zero-based wire row.
        row: u32,
        /// Zero-based column.
        column: u32,
        /// Whether the block should be bypassed.
        bypass: bool,
    },
    /// Remove a block and verify the cell is empty by complete read-back.
    RemoveBlock {
        /// Zero-based wire row.
        row: u32,
        /// Zero-based column.
        column: u32,
    },
    /// Move one occupied grid cell to an empty destination with read-back.
    MoveBlock {
        /// Zero-based source wire row.
        from_row: u32,
        /// Zero-based source column.
        from_column: u32,
        /// Zero-based destination wire row.
        to_row: u32,
        /// Zero-based destination column.
        to_column: u32,
        /// Maximum wait for each live-grid read.
        timeout_seconds: u64,
    },
    /// Set and verify a row's split and mix columns.
    SetSplit {
        /// Zero-based wire row.
        row: u32,
        /// Column at which the row branches.
        split: i32,
        /// Column at which it rejoins, or negative for never.
        mix: i32,
    },
    /// Set and verify a row's typed input or output port.
    SetRouting {
        /// Zero-based wire row.
        row: u32,
        /// The input port, if setting the input.
        input: Option<cortex_rs::GridInputPort>,
        /// The output port, if setting the output.
        output: Option<cortex_rs::GridOutputPort>,
    },
    /// Prepare a save destination before editing. **Not destructive.**
    ///
    /// The daemon holds the resulting `SavePreparation` under an opaque token
    /// and returns a `SavePreparationView`. The raw backup blob never crosses
    /// the socket.
    PrepareSave {
        /// Absolute device path of the setlist.
        setlist: String,
        /// Target slot, e.g. `2B`.
        slot: String,
        /// Whether the host accepts discarding the current working grid to
        /// back up an occupied (or apparently-empty) target.
        recall_consent: RecallConsent,
        /// Maximum wait for listing and recall, in seconds.
        timeout_seconds: u64,
    },
    /// Commit a previously prepared save. **Destructive.**
    ///
    /// Consumes the preparation token; a token cannot be used twice. Requires
    /// explicit confirmation.
    CommitSave {
        /// Opaque token returned by `PrepareSave`.
        token: String,
        /// Explicit confirmation from the host. Must be `true`.
        confirmed: bool,
        /// New name, or `None` to keep the slot's existing name.
        name: Option<String>,
        /// Preferred-instrument metadata written with the preset.
        instrument: cortex_rs::Instrument,
        /// Maximum wait for the re-list and save, in seconds.
        timeout_seconds: u64,
    },
    /// Delete a preset by name. **Destructive.**
    DeletePreset {
        /// Absolute device path of the setlist.
        setlist: String,
        /// The preset's stored name, as the device reports it.
        name: String,
    },
    /// Move a preset to an empty slot in the same setlist. **Destructive.**
    MovePreset {
        /// Absolute device path of the user setlist.
        setlist: String,
        /// Occupied source slot, e.g. `7A`.
        from_slot: String,
        /// Empty destination slot, e.g. `7B`.
        to_slot: String,
        /// Explicit confirmation from the host. Must be `true`.
        confirmed: bool,
    },
    /// Copy a preset by preparing the destination, recalling the source, and saving. **Destructive.**
    CopyPreset {
        /// Source USER setlist path.
        from_setlist: String,
        /// Source slot.
        from_slot: String,
        /// Destination USER setlist path.
        to_setlist: String,
        /// Destination slot.
        to_slot: String,
        /// Optional destination name.
        name: Option<String>,
        /// Preferred-instrument metadata.
        instrument: cortex_rs::Instrument,
        /// Explicit confirmation from the host.
        confirmed: bool,
    },
    /// Create one USER setlist. **Destructive.**
    CreateSetlist {
        /// Single safe destination name.
        name: String,
        /// Explicit confirmation from the host.
        confirmed: bool,
    },
    /// Delete one USER setlist and all of its presets. **Destructive.**
    DeleteSetlist {
        /// Single safe setlist name.
        name: String,
        /// Explicit confirmation from the host.
        confirmed: bool,
    },
    /// Duplicate one USER setlist through create plus recall/save. **Destructive.**
    DuplicateSetlist {
        /// Single safe source name.
        source_name: String,
        /// Single safe destination name.
        destination_name: String,
        /// Optional maximum occupied source entries to copy.
        limit: Option<usize>,
        /// Explicit confirmation from the host.
        confirmed: bool,
    },
    /// The most recent CPU load pushed by the device.
    ///
    /// Only meaningful on a subscribed session, which is the daemon's: a
    /// one-shot `Minimal` session never asks the device to push this.
    CpuLoad,
    /// Explain the latest CPU push against the current live grid without
    /// changing the device.
    AnalyzeCpuFit,
    /// Ask the daemon to shut down, announcing the disconnect first.
    Shutdown,
}

/// The result of a `PrepareSave` request: an opaque token and a safe view.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PrepareSaveResult {
    /// Opaque token; pass to `CommitSave` to commit. The daemon holds the
    /// actual `SavePreparation` under this key.
    pub token: String,
    /// Safe serialisable view of the preparation.
    pub view: cortex_rs::safety::SavePreparationView,
}

/// A response from the daemon.
///
/// `Ok` carries an arbitrary JSON value rather than a typed variant per
/// request: the payloads are already defined by the client API's own types,
/// and duplicating them here would create two shapes to keep in step.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Response {
    /// The request succeeded.
    Ok {
        /// The payload, shaped by the request.
        data: serde_json::Value,
    },
    /// The request failed.
    Error {
        /// Stable machine-readable failure category.
        code: DaemonErrorCode,
        /// Intended for a human to read.
        message: String,
    },
}

/// Stable machine-readable categories for daemon failures.
///
/// These are deliberately broader than [`cortex_rs::Error`]: hosts need to
/// decide whether an agent can correct a request, should retry later, or must
/// repair the session without parsing human-readable diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonErrorCode {
    /// The request envelope or argument combination was malformed.
    InvalidRequest,
    /// A preset slot name was malformed.
    InvalidSlot,
    /// A requested entity was not found.
    NotFound,
    /// The device silently refused a block because DSP capacity was exhausted.
    DspRefused,
    /// A block move was malformed or unsafe.
    InvalidBlockMove,
    /// Read-back could not prove the final state of a write.
    OutcomeUnconfirmed,
    /// A row index or row topology was invalid.
    InvalidRow,
    /// A scene index was invalid.
    InvalidScene,
    /// A parameter selector or value was invalid.
    InvalidParameter,
    /// A persistent operation did not satisfy its safety policy.
    SafetyRefused,
    /// The requested device reply did not arrive before its deadline.
    DeviceTimeout,
    /// The device or its session is unavailable.
    DeviceUnavailable,
    /// Another transport owns the device's exclusive editor channel.
    DeviceBusy,
    /// The daemon is reconnecting and the request may be retried later.
    Reconnecting,
    /// Another exclusive device operation is currently running.
    Busy,
    /// The daemon has stopped admitting device work.
    ShuttingDown,
    /// A subscribed value has not arrived yet.
    NotReady,
    /// Device traffic was malformed or could not be decoded.
    Protocol,
    /// The daemon failed internally rather than rejecting the model's request.
    Internal,
}

impl DaemonErrorCode {
    /// Classify one leaf-crate error without inspecting its display text.
    #[must_use]
    pub fn from_cortex_error(error: &cortex_rs::Error) -> Self {
        #[allow(unreachable_patterns)]
        match error {
            cortex_rs::Error::Framing(_)
            | cortex_rs::Error::Decode(_)
            | cortex_rs::Error::Trailer(_)
            | cortex_rs::Error::UnsupportedDeviceOperation { .. } => Self::Protocol,
            cortex_rs::Error::ReadTimeout(_) => Self::DeviceTimeout,
            cortex_rs::Error::DeviceBusy { .. } => Self::DeviceBusy,
            cortex_rs::Error::DeviceSilent(_) | cortex_rs::Error::Session(_) => {
                Self::DeviceUnavailable
            }
            cortex_rs::Error::InvalidSlot(_) => Self::InvalidSlot,
            cortex_rs::Error::NotFound(_) => Self::NotFound,
            cortex_rs::Error::BlockRefused(_) => Self::DspRefused,
            cortex_rs::Error::InvalidBlockMove(_) => Self::InvalidBlockMove,
            cortex_rs::Error::BlockMoveUnconfirmed(_)
            | cortex_rs::Error::GridWriteUnconfirmed(_)
            | cortex_rs::Error::MoveUnconfirmed(_)
            | cortex_rs::Error::SetlistUnconfirmed(_) => Self::OutcomeUnconfirmed,
            cortex_rs::Error::InvalidRow(_) => Self::InvalidRow,
            cortex_rs::Error::InvalidScene(_) => Self::InvalidScene,
            cortex_rs::Error::InvalidParameter(_) => Self::InvalidParameter,
            cortex_rs::Error::UnsafeSave(_) | cortex_rs::Error::UnsafeMove(_) => {
                Self::SafetyRefused
            }
            // HID-only variants are absent when cortex-host's leaf dependency
            // is built without its default feature.
            _ => Self::DeviceUnavailable,
        }
    }
}

impl Response {
    /// A success carrying any serialisable payload.
    ///
    /// # Errors
    ///
    /// Returns [`cortex_rs::Error::Decode`] if the payload cannot be serialised,
    /// which would be a bug in the caller rather than a protocol failure.
    pub fn ok<T: serde::Serialize>(value: &T) -> cortex_rs::Result<Self> {
        let data = serde_json::to_value(value)
            .map_err(|e| cortex_rs::Error::Decode(format!("serialising a response: {e}")))?;
        Ok(Self::Ok { data })
    }

    /// An internal failure carrying a human-readable message.
    #[must_use]
    pub fn error(message: impl Into<String>) -> Self {
        Self::coded_error(DaemonErrorCode::Internal, message)
    }

    /// A failure carrying a stable code and a human-readable message.
    #[must_use]
    pub fn coded_error(code: DaemonErrorCode, message: impl Into<String>) -> Self {
        Self::Error {
            code,
            message: message.into(),
        }
    }

    /// Classify and expose one leaf-crate error without parsing its message.
    #[must_use]
    pub fn cortex_error(error: &cortex_rs::Error) -> Self {
        Self::coded_error(DaemonErrorCode::from_cortex_error(error), error.to_string())
    }
}

/// What the daemon is currently doing, reported by [`Request::Status`].
///
/// Health is reported rather than inferred from whether a call hangs. A
/// client that cannot tell "the device went away" from "this is just slow"
/// has no way to give the user a useful message.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Status {
    /// The daemon's own version, so a client can detect skew after an
    /// upgrade left an old daemon running.
    pub daemon_version: String,
    /// Seconds since the daemon started.
    pub uptime_seconds: u64,
    /// Whether another host started this daemon on demand.
    #[serde(default)]
    pub auto_managed: bool,
    /// Request-idle shutdown bound for an auto-managed daemon.
    #[serde(default)]
    pub idle_timeout_seconds: Option<u64>,
    /// Product whose USB interface this daemon owns.
    #[serde(default)]
    pub device_kind: DeviceKind,
    /// Whether the device is currently answering.
    pub device: DeviceHealth,
    /// What the daemon has cached, and how fresh it is.
    pub cache: CacheStatus,
}

/// Whether the device is answering, and what it is.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DeviceHealth {
    /// Connected and answering.
    Connected {
        /// The unit's serial number, when known.
        serial: Option<String>,
        /// The `CorOS` version, when known.
        coros_version: Option<String>,
        /// Seconds since anything was last received.
        ///
        /// A healthy session reads 0 here even when idle, because the
        /// device pushes continuously while it is being kept alive. A large
        /// value means something is wrong. See roadmap PROT-008.6.4.
        last_message_seconds: u64,
    },
    /// The connection dropped and the daemon is trying to re-establish it.
    Reconnecting {
        /// How many reconnection attempts have been made.
        attempts: u32,
        /// Why the last attempt failed.
        last_error: String,
    },
    /// The daemon has given up.
    Failed {
        /// Why.
        error: String,
    },
}

/// What the daemon holds, and whether it can be trusted.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct CacheStatus {
    /// Physical-session generation that owns the values.
    pub generation: u64,
    /// Monotonic state revision for coalescing updates.
    pub revision: u64,
    /// Stored-preset mutation epoch in this generation.
    pub storage_revision: u64,
    /// Overall cache readiness.
    pub phase: cortex_rs::CachePhase,
    /// Whether the model catalog is held.
    pub catalog: bool,
    /// Whether a live grid is held.
    pub current_preset: bool,
    /// Whether the active scene is held.
    pub active_scene: bool,
    /// Whether working-copy dirty state is held.
    pub preset_dirty: bool,
    /// Whether the selected setlist and slot are held.
    pub preset_location: bool,
    /// Setlists whose listing is held.
    pub listed_setlists: Vec<String>,
    /// How many device pushes have been applied since connecting. A cache
    /// kept current by pushes is only as trustworthy as the push stream, so
    /// this is worth surfacing.
    pub pushes_applied: u64,
    /// State-bearing messages observed in this generation.
    pub messages_seen: u64,
    /// Messages rejected because applying them would require guessing.
    pub messages_rejected: u64,
    /// Broken frame sequences that invalidated continuity.
    pub stream_gaps: u64,
    /// Why the most recent state message was rejected.
    pub last_rejection: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::too_many_lines)]
    fn canonical_request_fixtures() -> Vec<(Request, serde_json::Value)> {
        let capture = cortex_rs::LibraryEntry {
            key: "a".repeat(64),
            name: "Fictional Capture".into(),
        };
        let ir = cortex_rs::LibraryEntry {
            key: "fictional-ir".into(),
            name: "Fictional IR".into(),
        };
        vec![
            (Request::Status, serde_json::json!({ "op": "status" })),
            (
                Request::ReconnectNow,
                serde_json::json!({ "op": "reconnect_now" }),
            ),
            (
                Request::NanoState,
                serde_json::json!({ "op": "nano_state" }),
            ),
            (
                Request::NanoSetAmp {
                    control: cortex_rs::nano::NanoAmpControl::Gain,
                    value: 127,
                },
                serde_json::json!({ "op": "nano_set_amp", "control": "gain", "value": 127 }),
            ),
            (
                Request::NanoSetBypass {
                    target: cortex_rs::nano::NanoBypassTarget::PostFx3,
                    bypassed: false,
                },
                serde_json::json!({ "op": "nano_set_bypass", "target": "post_fx3", "bypassed": false }),
            ),
            (
                Request::NanoReadFxParams {
                    slot: cortex_rs::nano::NanoFxSlot::PreFx2,
                },
                serde_json::json!({ "op": "nano_read_fx_params", "slot": "pre_fx2" }),
            ),
            (
                Request::NanoSetFxParam {
                    slot: cortex_rs::nano::NanoFxSlot::PostFx1,
                    param_index: 3,
                    value: 0.5,
                },
                serde_json::json!({ "op": "nano_set_fx_param", "slot": "post_fx1", "param_index": 3, "value": 0.5 }),
            ),
            (Request::Version, serde_json::json!({ "op": "version" })),
            (
                Request::ActiveScene,
                serde_json::json!({ "op": "active_scene" }),
            ),
            (
                Request::SwitchScene { scene: 3 },
                serde_json::json!({ "op": "switch_scene", "scene": 3 }),
            ),
            (
                Request::SetSceneLabel {
                    scene: 2,
                    label: None,
                },
                serde_json::json!({ "op": "set_scene_label", "scene": 2, "label": null }),
            ),
            (
                Request::SetSceneColor {
                    scene: 4,
                    color: 0xff12_3456,
                },
                serde_json::json!({ "op": "set_scene_color", "scene": 4, "color": 4_279_383_126_u32 }),
            ),
            (
                Request::CopyScene {
                    from_scene: 1,
                    to_scene: 6,
                    swap: true,
                },
                serde_json::json!({ "op": "copy_scene", "from_scene": 1, "to_scene": 6, "swap": true }),
            ),
            (
                Request::CurrentPreset {
                    with_params: true,
                    timeout_seconds: 15,
                },
                serde_json::json!({ "op": "current_preset", "with_params": true, "timeout_seconds": 15 }),
            ),
            (
                Request::ReadPreset {
                    setlist: "/media/p4/Presets/Fictional".into(),
                    slot: "2B".into(),
                    factory: false,
                    with_params: true,
                    timeout_seconds: 15,
                },
                serde_json::json!({
                    "op": "read_preset", "setlist": "/media/p4/Presets/Fictional", "slot": "2B",
                    "factory": false, "with_params": true, "timeout_seconds": 15
                }),
            ),
            (
                Request::RecallPreset {
                    setlist: "/media/p4/Presets/Fictional".into(),
                    slot: "2B".into(),
                    factory: false,
                },
                serde_json::json!({ "op": "recall_preset", "setlist": "/media/p4/Presets/Fictional", "slot": "2B", "factory": false }),
            ),
            (
                Request::ListPresets {
                    setlist: "/media/p4/Presets/Fictional".into(),
                    include_empty: true,
                    timeout_seconds: 15,
                },
                serde_json::json!({ "op": "list_presets", "setlist": "/media/p4/Presets/Fictional", "include_empty": true, "timeout_seconds": 15 }),
            ),
            (
                Request::ListFolders { window_seconds: 3 },
                serde_json::json!({ "op": "list_folders", "window_seconds": 3 }),
            ),
            (
                Request::Catalog {
                    timeout_seconds: 15,
                },
                serde_json::json!({ "op": "catalog", "timeout_seconds": 15 }),
            ),
            (
                Request::ListCaptures {
                    timeout_seconds: 15,
                },
                serde_json::json!({ "op": "list_captures", "timeout_seconds": 15 }),
            ),
            (
                Request::ListIrs {
                    folder: None,
                    timeout_seconds: 15,
                },
                serde_json::json!({ "op": "list_irs", "folder": null, "timeout_seconds": 15 }),
            ),
            (
                Request::SetCapture {
                    row: 0,
                    column: 2,
                    capture: capture.clone(),
                    model: Some(1001),
                    timeout_seconds: 15,
                },
                serde_json::json!({
                    "op": "set_capture", "row": 0, "column": 2,
                    "capture": { "key": "a".repeat(64), "name": "Fictional Capture" },
                    "model": 1001, "timeout_seconds": 15
                }),
            ),
            (
                Request::SetIr {
                    row: 0,
                    column: 3,
                    ir: ir.clone(),
                    slot: 1,
                    model: None,
                    folder: Some("Fictional Folder".into()),
                    timeout_seconds: 15,
                },
                serde_json::json!({
                    "op": "set_ir", "row": 0, "column": 3,
                    "ir": { "key": "fictional-ir", "name": "Fictional IR" }, "slot": 1,
                    "model": null, "folder": "Fictional Folder", "timeout_seconds": 15
                }),
            ),
            (
                Request::SetParam {
                    row: 1,
                    column: 2,
                    target: cortex_rs::ParameterTarget::Name("GAIN".into()),
                    input: cortex_rs::ParameterInput::Real(7.5),
                    scene: Some(3),
                    promote: true,
                    timeout_seconds: 15,
                },
                serde_json::json!({
                    "op": "set_param", "row": 1, "column": 2,
                    "target": { "by": "name", "value": "GAIN" },
                    "input": { "kind": "real", "value": 7.5 }, "scene": 3,
                    "promote": true, "timeout_seconds": 15
                }),
            ),
            (
                Request::SetBlock {
                    row: 0,
                    column: 2,
                    model: 1001,
                    verify: true,
                    timeout_seconds: 15,
                },
                serde_json::json!({ "op": "set_block", "row": 0, "column": 2, "model": 1001, "verify": true, "timeout_seconds": 15 }),
            ),
            (
                Request::SetBypass {
                    row: 0,
                    column: 2,
                    bypass: true,
                },
                serde_json::json!({ "op": "set_bypass", "row": 0, "column": 2, "bypass": true }),
            ),
            (
                Request::RemoveBlock { row: 0, column: 2 },
                serde_json::json!({ "op": "remove_block", "row": 0, "column": 2 }),
            ),
            (
                Request::MoveBlock {
                    from_row: 0,
                    from_column: 2,
                    to_row: 1,
                    to_column: 6,
                    timeout_seconds: 15,
                },
                serde_json::json!({
                    "op": "move_block", "from_row": 0, "from_column": 2,
                    "to_row": 1, "to_column": 6, "timeout_seconds": 15
                }),
            ),
            (
                Request::SetSplit {
                    row: 0,
                    split: 3,
                    mix: 6,
                },
                serde_json::json!({ "op": "set_split", "row": 0, "split": 3, "mix": 6 }),
            ),
            (
                Request::SetRouting {
                    row: 0,
                    input: Some(cortex_rs::GridInputPort::PreviousRow),
                    output: None,
                },
                serde_json::json!({ "op": "set_routing", "row": 0, "input": "previous_row", "output": null }),
            ),
            (
                Request::PrepareSave {
                    setlist: "/media/p4/Presets/Fictional".into(),
                    slot: "2B".into(),
                    recall_consent: RecallConsent::DiscardWorkingCopy,
                    timeout_seconds: 40,
                },
                serde_json::json!({
                    "op": "prepare_save", "setlist": "/media/p4/Presets/Fictional", "slot": "2B",
                    "recall_consent": "discard_working_copy", "timeout_seconds": 40
                }),
            ),
            (
                Request::CommitSave {
                    token: "save-fictional".into(),
                    confirmed: true,
                    name: Some("Fictional".into()),
                    instrument: cortex_rs::Instrument::Vocal,
                    timeout_seconds: 40,
                },
                serde_json::json!({
                    "op": "commit_save", "token": "save-fictional", "confirmed": true,
                    "name": "Fictional", "instrument": "vocal", "timeout_seconds": 40
                }),
            ),
            (
                Request::DeletePreset {
                    setlist: "/media/p4/Presets/Fictional".into(),
                    name: "Fictional".into(),
                },
                serde_json::json!({ "op": "delete_preset", "setlist": "/media/p4/Presets/Fictional", "name": "Fictional" }),
            ),
            (
                Request::MovePreset {
                    setlist: "/media/p4/Presets/Fictional".into(),
                    from_slot: "2A".into(),
                    to_slot: "2B".into(),
                    confirmed: true,
                },
                serde_json::json!({
                    "op": "move_preset", "setlist": "/media/p4/Presets/Fictional",
                    "from_slot": "2A", "to_slot": "2B", "confirmed": true
                }),
            ),
            (
                Request::CopyPreset {
                    from_setlist: "/media/p4/Presets/Fictional".into(),
                    from_slot: "2A".into(),
                    to_setlist: "/media/p4/Presets/Fictional Copy".into(),
                    to_slot: "2B".into(),
                    name: None,
                    instrument: cortex_rs::Instrument::Bass,
                    confirmed: true,
                },
                serde_json::json!({
                    "op": "copy_preset", "from_setlist": "/media/p4/Presets/Fictional", "from_slot": "2A",
                    "to_setlist": "/media/p4/Presets/Fictional Copy", "to_slot": "2B", "name": null,
                    "instrument": "bass", "confirmed": true
                }),
            ),
            (
                Request::CreateSetlist {
                    name: "Fictional Setlist".into(),
                    confirmed: true,
                },
                serde_json::json!({ "op": "create_setlist", "name": "Fictional Setlist", "confirmed": true }),
            ),
            (
                Request::DeleteSetlist {
                    name: "Fictional Setlist".into(),
                    confirmed: true,
                },
                serde_json::json!({ "op": "delete_setlist", "name": "Fictional Setlist", "confirmed": true }),
            ),
            (
                Request::DuplicateSetlist {
                    source_name: "My Presets".into(),
                    destination_name: "Fictional Duplicate".into(),
                    limit: Some(2),
                    confirmed: true,
                },
                serde_json::json!({
                    "op": "duplicate_setlist", "source_name": "My Presets",
                    "destination_name": "Fictional Duplicate", "limit": 2, "confirmed": true
                }),
            ),
            (Request::CpuLoad, serde_json::json!({ "op": "cpu_load" })),
            (
                Request::AnalyzeCpuFit,
                serde_json::json!({ "op": "analyze_cpu_fit" }),
            ),
            (Request::Shutdown, serde_json::json!({ "op": "shutdown" })),
        ]
    }

    fn request_operation(request: &Request) -> &'static str {
        match request {
            Request::Status => "status",
            Request::ReconnectNow => "reconnect_now",
            Request::NanoState => "nano_state",
            Request::NanoSetAmp { .. } => "nano_set_amp",
            Request::NanoSetBypass { .. } => "nano_set_bypass",
            Request::NanoReadFxParams { .. } => "nano_read_fx_params",
            Request::NanoSetFxParam { .. } => "nano_set_fx_param",
            Request::Version => "version",
            Request::ActiveScene => "active_scene",
            Request::SwitchScene { .. } => "switch_scene",
            Request::SetSceneLabel { .. } => "set_scene_label",
            Request::SetSceneColor { .. } => "set_scene_color",
            Request::CopyScene { .. } => "copy_scene",
            Request::CurrentPreset { .. } => "current_preset",
            Request::ReadPreset { .. } => "read_preset",
            Request::RecallPreset { .. } => "recall_preset",
            Request::ListPresets { .. } => "list_presets",
            Request::ListFolders { .. } => "list_folders",
            Request::Catalog { .. } => "catalog",
            Request::ListCaptures { .. } => "list_captures",
            Request::ListIrs { .. } => "list_irs",
            Request::SetCapture { .. } => "set_capture",
            Request::SetIr { .. } => "set_ir",
            Request::SetParam { .. } => "set_param",
            Request::SetBlock { .. } => "set_block",
            Request::SetBypass { .. } => "set_bypass",
            Request::RemoveBlock { .. } => "remove_block",
            Request::MoveBlock { .. } => "move_block",
            Request::SetSplit { .. } => "set_split",
            Request::SetRouting { .. } => "set_routing",
            Request::PrepareSave { .. } => "prepare_save",
            Request::CommitSave { .. } => "commit_save",
            Request::DeletePreset { .. } => "delete_preset",
            Request::MovePreset { .. } => "move_preset",
            Request::CopyPreset { .. } => "copy_preset",
            Request::CreateSetlist { .. } => "create_setlist",
            Request::DeleteSetlist { .. } => "delete_setlist",
            Request::DuplicateSetlist { .. } => "duplicate_setlist",
            Request::CpuLoad => "cpu_load",
            Request::AnalyzeCpuFit => "analyze_cpu_fit",
            Request::Shutdown => "shutdown",
        }
    }

    #[test]
    fn every_request_variant_has_a_canonical_json_fixture() {
        let fixtures = canonical_request_fixtures();
        assert_eq!(fixtures.len(), 41);
        let mut operations = std::collections::BTreeSet::new();

        for (request, expected) in fixtures {
            let operation = request_operation(&request);
            assert!(
                operations.insert(operation),
                "duplicate fixture for {operation}"
            );
            assert_eq!(expected["op"], operation);
            assert_eq!(serde_json::to_value(&request).unwrap(), expected);
            let decoded: Request = serde_json::from_value(expected.clone()).unwrap();
            assert_eq!(request_operation(&decoded), operation);
            assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
        }
    }

    #[test]
    fn requests_round_trip_as_tagged_json() {
        // The tag makes the wire form readable with `jq` and stable against
        // reordering the enum.
        let request = Request::SwitchScene { scene: 3 };
        let text = serde_json::to_string(&request).unwrap();
        assert!(text.contains(r#""op":"switch_scene""#), "{text}");
        assert!(text.contains(r#""scene":3"#), "{text}");
        let back: Request = serde_json::from_str(&text).unwrap();
        assert!(matches!(back, Request::SwitchScene { scene: 3 }));
    }

    #[test]
    fn scene_management_requests_round_trip_without_losing_argb_or_null() {
        let requests = [
            Request::SetSceneLabel {
                scene: 2,
                label: Some("Wide Lead".into()),
            },
            Request::SetSceneLabel {
                scene: 3,
                label: None,
            },
            Request::SetSceneColor {
                scene: 4,
                color: u32::MAX,
            },
            Request::CopyScene {
                from_scene: 1,
                to_scene: 6,
                swap: true,
            },
        ];
        for request in requests {
            let text = serde_json::to_string(&request).unwrap();
            let back: Request = serde_json::from_str(&text).unwrap();
            assert_eq!(
                serde_json::to_value(back).unwrap(),
                serde_json::to_value(request).unwrap()
            );
        }
    }

    #[test]
    fn a_block_move_round_trips_both_cells() {
        let request = Request::MoveBlock {
            from_row: 0,
            from_column: 2,
            to_row: 1,
            to_column: 6,
            timeout_seconds: 15,
        };
        let text = serde_json::to_string(&request).unwrap();
        let back: Request = serde_json::from_str(&text).unwrap();
        assert_eq!(
            serde_json::to_value(back).unwrap(),
            serde_json::to_value(request).unwrap()
        );
    }

    #[test]
    fn typed_grid_routes_round_trip_by_name() {
        let requests = [
            Request::SetRouting {
                row: 0,
                input: Some(cortex_rs::GridInputPort::PreviousRow),
                output: None,
            },
            Request::SetRouting {
                row: 2,
                input: None,
                output: Some(cortex_rs::GridOutputPort::NextRow34),
            },
        ];
        for request in requests {
            let text = serde_json::to_string(&request).unwrap();
            assert!(!text.contains("portid"), "{text}");
            let back: Request = serde_json::from_str(&text).unwrap();
            assert_eq!(
                serde_json::to_value(back).unwrap(),
                serde_json::to_value(request).unwrap()
            );
        }
    }

    #[test]
    fn a_unit_request_needs_no_payload() {
        let text = serde_json::to_string(&Request::Status).unwrap();
        assert_eq!(text, r#"{"op":"status"}"#);
        let text = serde_json::to_string(&Request::ReconnectNow).unwrap();
        assert_eq!(text, r#"{"op":"reconnect_now"}"#);
        let text = serde_json::to_string(&Request::NanoState).unwrap();
        assert_eq!(text, r#"{"op":"nano_state"}"#);
        let text = serde_json::to_string(&Request::NanoSetBypass {
            target: cortex_rs::nano::NanoBypassTarget::PreFx1,
            bypassed: true,
        })
        .unwrap();
        assert_eq!(
            text,
            r#"{"op":"nano_set_bypass","target":"pre_fx1","bypassed":true}"#
        );
        let text = serde_json::to_string(&Request::NanoReadFxParams {
            slot: cortex_rs::nano::NanoFxSlot::PostFx1,
        })
        .unwrap();
        assert_eq!(text, r#"{"op":"nano_read_fx_params","slot":"post_fx1"}"#);
    }

    #[test]
    fn every_nano_request_has_an_exact_typed_json_shape() {
        let requests = [
            (
                Request::NanoState,
                serde_json::json!({ "op": "nano_state" }),
            ),
            (
                Request::NanoSetAmp {
                    control: cortex_rs::nano::NanoAmpControl::Gain,
                    value: 127,
                },
                serde_json::json!({
                    "op": "nano_set_amp",
                    "control": "gain",
                    "value": 127
                }),
            ),
            (
                Request::NanoSetBypass {
                    target: cortex_rs::nano::NanoBypassTarget::PostFx3,
                    bypassed: false,
                },
                serde_json::json!({
                    "op": "nano_set_bypass",
                    "target": "post_fx3",
                    "bypassed": false
                }),
            ),
            (
                Request::NanoReadFxParams {
                    slot: cortex_rs::nano::NanoFxSlot::PreFx2,
                },
                serde_json::json!({
                    "op": "nano_read_fx_params",
                    "slot": "pre_fx2"
                }),
            ),
            (
                Request::NanoSetFxParam {
                    slot: cortex_rs::nano::NanoFxSlot::PostFx1,
                    param_index: 3,
                    value: 0.5,
                },
                serde_json::json!({
                    "op": "nano_set_fx_param",
                    "slot": "post_fx1",
                    "param_index": 3,
                    "value": 0.5
                }),
            ),
        ];

        for (request, expected) in requests {
            let actual = serde_json::to_value(&request).unwrap();
            assert_eq!(actual, expected);
            assert_eq!(
                serde_json::to_value(serde_json::from_value::<Request>(actual).unwrap()).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn responses_distinguish_success_from_failure() {
        let ok = Response::ok(&vec![1, 2, 3]).unwrap();
        let text = serde_json::to_string(&ok).unwrap();
        assert!(text.contains(r#""status":"ok""#), "{text}");

        let err = Response::coded_error(DaemonErrorCode::DeviceUnavailable, "device not found");
        let text = serde_json::to_string(&err).unwrap();
        assert!(text.contains(r#""status":"error""#), "{text}");
        assert!(text.contains(r#""code":"device_unavailable""#), "{text}");
        assert!(text.contains("device not found"), "{text}");
    }

    #[test]
    fn leaf_errors_have_stable_codes() {
        assert_eq!(
            DaemonErrorCode::from_cortex_error(&cortex_rs::Error::InvalidRow("4".into())),
            DaemonErrorCode::InvalidRow
        );
        assert_eq!(
            DaemonErrorCode::from_cortex_error(&cortex_rs::Error::BlockRefused("full".into())),
            DaemonErrorCode::DspRefused
        );
    }

    #[test]
    fn a_request_is_one_line() {
        // The framing is line-delimited, so a serialised request must never
        // contain a newline or the stream desynchronises.
        let request = Request::RecallPreset {
            setlist: "/media/p4/Presets/My Presets".into(),
            slot: "28C".into(),
            factory: false,
        };
        let text = serde_json::to_string(&request).unwrap();
        assert!(!text.contains('\n'), "a request must serialise to one line");
    }

    #[test]
    fn a_high_level_parameter_request_round_trips() {
        let request = Request::SetParam {
            row: 1,
            column: 2,
            target: cortex_rs::ParameterTarget::Name("GAIN".into()),
            input: cortex_rs::ParameterInput::Real(7.5),
            scene: Some(3),
            promote: true,
            timeout_seconds: 15,
        };
        let text = serde_json::to_string(&request).unwrap();
        let back: Request = serde_json::from_str(&text).unwrap();
        assert!(matches!(
            back,
            Request::SetParam {
                row: 1,
                column: 2,
                target: cortex_rs::ParameterTarget::Name(name),
                input: cortex_rs::ParameterInput::Real(7.5),
                scene: Some(3),
                promote: true,
                timeout_seconds: 15,
            } if name == "GAIN"
        ));
    }

    #[test]
    fn a_confirmed_preset_move_round_trips() {
        let request = Request::MovePreset {
            setlist: cortex_rs::client::USER_SETLIST.into(),
            from_slot: "2A".into(),
            to_slot: "2B".into(),
            confirmed: true,
        };
        let text = serde_json::to_string(&request).unwrap();
        let back: Request = serde_json::from_str(&text).unwrap();
        assert!(matches!(
            back,
            Request::MovePreset {
                from_slot,
                to_slot,
                confirmed: true,
                ..
            } if from_slot == "2A" && to_slot == "2B"
        ));
    }

    #[test]
    fn file_operation_requests_preserve_instrument_and_partial_limit() {
        let requests = [
            Request::CommitSave {
                token: "save-fictional".into(),
                confirmed: true,
                name: Some("Fictional".into()),
                instrument: cortex_rs::Instrument::Vocal,
                timeout_seconds: 40,
            },
            Request::CopyPreset {
                from_setlist: cortex_rs::client::USER_SETLIST.into(),
                from_slot: "6A".into(),
                to_setlist: "/media/p4/Presets/Fictional Destination".into(),
                to_slot: "1A".into(),
                name: None,
                instrument: cortex_rs::Instrument::Bass,
                confirmed: true,
            },
            Request::DuplicateSetlist {
                source_name: "My Presets".into(),
                destination_name: "Fictional Duplicate".into(),
                limit: Some(2),
                confirmed: true,
            },
            Request::CreateSetlist {
                name: "Fictional Setlist".into(),
                confirmed: true,
            },
        ];
        for request in requests {
            let value = serde_json::to_value(&request).unwrap();
            let back: Request = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(back).unwrap(), value);
        }
    }

    #[test]
    fn device_health_states_are_distinguishable_on_the_wire() {
        // A client must be able to tell "gone" from "slow", which is the
        // whole point of reporting health rather than letting calls hang.
        let connected = DeviceHealth::Connected {
            serial: Some("QA00AB123".into()),
            coros_version: Some("4.0.1".into()),
            last_message_seconds: 0,
        };
        let text = serde_json::to_string(&connected).unwrap();
        assert!(text.contains(r#""state":"connected""#), "{text}");

        let reconnecting = DeviceHealth::Reconnecting {
            attempts: 2,
            last_error: "read timeout".into(),
        };
        let text = serde_json::to_string(&reconnecting).unwrap();
        assert!(text.contains(r#""state":"reconnecting""#), "{text}");
    }
}
