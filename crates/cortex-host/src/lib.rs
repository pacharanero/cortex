// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Shared host boundary for applications that use a held Cortex session.
//!
//! This crate owns the local daemon contract and synchronous local IPC client.
//! It intentionally has no HID feature and cannot open a device. Unix sockets
//! are the current adapter; Windows named pipes fit behind the same facade.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(clippy::all, clippy::pedantic)]

mod client;
mod ipc;
mod protocol;
mod server;

pub use client::{DaemonClient, DaemonError, is_running, request, request_with_timeout};
pub use ipc::{BindResult, LocalClaim, LocalConnection, LocalEndpoint, LocalListener, log_path};
pub use protocol::{
    CacheStatus, DAEMON_PROTOCOL_VERSION, DaemonErrorCode, DeviceHealth, PrepareSaveResult,
    Request, Response, Status,
};
pub use server::{DaemonLifecycle, ServerExit, serve_listener};
