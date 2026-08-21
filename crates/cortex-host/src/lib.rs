// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Shared host boundary for applications that use a held Cortex session.
//!
//! This crate owns the local daemon contract and synchronous local IPC client.
//! It intentionally has no HID feature and cannot open a device. Unix sockets
//! are the current adapter; Windows named pipes fit behind the same facade.
//!
//! @see spec/200-cli/spec.md [FR-18] [FR-19]
//! @see spec/200-cli/design.md [DES-CLI]

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(clippy::all, clippy::pedantic)]

mod client;
mod cpu_fit;
mod ipc;
mod protocol;
mod server;
mod supervisor;
pub mod tool_registry;

pub use client::{DaemonClient, DaemonError, is_running, request, request_with_timeout};
pub use cpu_fit::{CpuFitAnalysis, CpuFitCell, CpuFitCore};
pub use ipc::{BindResult, LocalClaim, LocalConnection, LocalEndpoint, LocalListener, log_path};
pub use protocol::{
    CacheStatus, DAEMON_PROTOCOL_VERSION, DaemonErrorCode, DeviceHealth, PrepareSaveResult,
    Request, Response, Status,
};
pub use server::{DaemonLifecycle, ServerExit, serve_listener};
pub use supervisor::{DaemonSupervisor, DevicePolicy};
