// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The wire contract between `cortex connect` and its clients.
//!
//! `cortex connect` holds one subscribed session and owns the HID interface;
//! everything else talks to it over a unix socket. This module defines what
//! they say to each other, and lives in the crate rather than the CLI so the
//! MCP server and the Tauri backend can speak the same protocol rather than
//! growing their own.
//!
//! ## Why a daemon at all
//!
//! The device grants its HID interface exclusively, so exactly one process
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

use std::path::PathBuf;

/// Where the daemon listens.
///
/// `$XDG_RUNTIME_DIR` is the right home: it is user-owned, `0700`, and
/// cleared on logout, so a stale socket cannot outlive a session. Falls back
/// to a uid-qualified path under the temp directory when it is unset, which
/// is the case on some minimal systems.
#[must_use]
pub fn socket_path() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(dir).join("cortex.sock");
    }
    // Qualify by uid so two users on one machine cannot collide, and so a
    // socket left by another user is not mistaken for ours.
    let uid = std::env::var("UID").unwrap_or_else(|_| "0".into());
    std::env::temp_dir().join(format!("cortex-{uid}.sock"))
}

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
    /// Device firmware and identity.
    Version,
    /// The active scene index.
    ActiveScene,
    /// Switch the active scene. Zero-based.
    SwitchScene {
        /// Scene index, 0-7. Zero-based; the unit labels these A-H.
        scene: u32,
    },
    /// The live grid, including unsaved edits.
    CurrentPreset,
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
    },
    /// The device model catalog.
    Catalog,
    /// The most recent CPU load pushed by the device.
    ///
    /// Only meaningful on a subscribed session, which is the daemon's: a
    /// one-shot `Minimal` session never asks the device to push this.
    CpuLoad,
    /// Ask the daemon to shut down, announcing the disconnect first.
    Shutdown,
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
        /// Intended for a human to read.
        message: String,
    },
}

impl Response {
    /// A success carrying any serialisable payload.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::Decode`] if the payload cannot be serialised,
    /// which would be a bug in the caller rather than a protocol failure.
    pub fn ok<T: serde::Serialize>(value: &T) -> crate::Result<Self> {
        let data = serde_json::to_value(value)
            .map_err(|e| crate::Error::Decode(format!("serialising a response: {e}")))?;
        Ok(Self::Ok { data })
    }

    /// A failure carrying a human-readable message.
    #[must_use]
    pub fn error(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
        }
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
pub struct CacheStatus {
    /// Whether the model catalog is held.
    pub catalog: bool,
    /// Whether a live grid is held.
    pub current_preset: bool,
    /// Setlists whose listing is held.
    pub listed_setlists: Vec<String>,
    /// How many device pushes have been applied since connecting. A cache
    /// kept current by pushes is only as trustworthy as the push stream, so
    /// this is worth surfacing.
    pub pushes_applied: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_unit_request_needs_no_payload() {
        let text = serde_json::to_string(&Request::Status).unwrap();
        assert_eq!(text, r#"{"op":"status"}"#);
    }

    #[test]
    fn responses_distinguish_success_from_failure() {
        let ok = Response::ok(&vec![1, 2, 3]).unwrap();
        let text = serde_json::to_string(&ok).unwrap();
        assert!(text.contains(r#""status":"ok""#), "{text}");

        let err = Response::error("device not found");
        let text = serde_json::to_string(&err).unwrap();
        assert!(text.contains(r#""status":"error""#), "{text}");
        assert!(text.contains("device not found"), "{text}");
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
    fn socket_path_follows_xdg_runtime_dir() {
        // Not asserting the fallback, which depends on the environment; the
        // XDG case is the one that matters and the one we control.
        if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
            let expected = PathBuf::from(dir).join("cortex.sock");
            assert_eq!(socket_path(), expected);
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
