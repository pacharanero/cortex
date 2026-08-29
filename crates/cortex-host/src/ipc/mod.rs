// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Platform-neutral local IPC facade for the held Cortex daemon.
//!
//! Unix uses a user-owned domain socket. Windows uses a local-only named pipe
//! restricted to the pipe object's owner.
//!
//! @see spec/200-cli/spec.md [FR-18]
//! @see spec/200-cli/design.md [DES-CLI]

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use unix::{BindResult, LocalClaim, LocalConnection, LocalEndpoint, LocalListener};
#[cfg(windows)]
pub use windows::{BindResult, LocalClaim, LocalConnection, LocalEndpoint, LocalListener};

/// Where a backgrounded session writes its log.
#[must_use]
pub fn log_path() -> std::path::PathBuf {
    LocalEndpoint::daemon().log_path()
}
