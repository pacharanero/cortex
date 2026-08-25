// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Windows named-pipe adapter placeholder.
//!
//! The host contract compiles independently of Unix socket types. Runtime IPC
//! remains unsupported until a reviewed safe named-pipe dependency supplies
//! current-user ACLs and duplex byte-stream semantics.
//!
//! @see spec/roadmap.md [CLI-004.12]
//! @see spec/200-cli/spec.md [FR-18]
//! @see spec/200-cli/design.md [DES-LIMITS]

use std::fmt;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

fn unsupported() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Windows local IPC is not implemented yet; the planned adapter uses a current-user named pipe",
    )
}

/// Platform-specific address of the local daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalEndpoint {
    name: String,
}

impl LocalEndpoint {
    /// The current user's future Cortex named-pipe endpoint.
    #[must_use]
    pub fn daemon() -> Self {
        Self::for_device(cortex_rs::DeviceKind::QuadCortex)
    }

    /// The current user's future named-pipe endpoint for one Cortex product.
    #[must_use]
    pub fn for_device(device: cortex_rs::DeviceKind) -> Self {
        Self {
            name: match device {
                cortex_rs::DeviceKind::QuadCortex => r"\\.\pipe\cortex",
                cortex_rs::DeviceKind::NanoCortex => r"\\.\pipe\cortex-nano",
            }
            .into(),
        }
    }

    /// Place for logs from the detached daemon process.
    #[must_use]
    pub fn log_path(&self) -> PathBuf {
        let name = if self.name.ends_with("cortex-nano") {
            "cortex-nano.log"
        } else {
            "cortex.log"
        };
        std::env::temp_dir().join(name)
    }

    pub(crate) fn has_active_claim(&self) -> bool {
        let _ = &self.name;
        false
    }
}

impl fmt::Display for LocalEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.name.fmt(formatter)
    }
}

/// Result of claiming the local daemon endpoint.
pub struct BindResult {
    /// Listener holding the endpoint claim.
    pub listener: LocalListener,
    /// Named pipes have no stale filesystem endpoint.
    pub removed_stale_endpoint: bool,
}

/// Future Windows process-lifetime device ownership claim.
pub struct LocalClaim;

impl LocalClaim {
    /// Claim acquisition is unavailable until the named-pipe adapter lands.
    ///
    /// # Errors
    ///
    /// Always returns [`std::io::ErrorKind::Unsupported`] in this placeholder.
    pub fn acquire(_endpoint: &LocalEndpoint) -> std::io::Result<Self> {
        Err(unsupported())
    }
}

/// Future Windows named-pipe listener.
pub struct LocalListener;

impl LocalListener {
    /// Bind is unavailable until the named-pipe adapter lands.
    ///
    /// # Errors
    ///
    /// Always returns [`std::io::ErrorKind::Unsupported`] in this placeholder.
    pub fn bind(_endpoint: &LocalEndpoint) -> std::io::Result<BindResult> {
        Err(unsupported())
    }

    /// Accept is unavailable until the named-pipe adapter lands.
    ///
    /// # Errors
    ///
    /// Always returns [`std::io::ErrorKind::Unsupported`] in this placeholder.
    pub fn accept(&self) -> std::io::Result<LocalConnection> {
        Err(unsupported())
    }

    /// Nonblocking mode is unavailable until the named-pipe adapter lands.
    ///
    /// # Errors
    ///
    /// Always returns [`std::io::ErrorKind::Unsupported`] in this placeholder.
    pub fn set_nonblocking(&self, _nonblocking: bool) -> std::io::Result<()> {
        Err(unsupported())
    }

    /// Named pipes leave no filesystem endpoint to remove.
    ///
    /// # Errors
    ///
    /// Reserved for errors from the future named-pipe adapter.
    pub fn cleanup_endpoint(&self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Future Windows named-pipe byte stream.
pub struct LocalConnection;

impl LocalConnection {
    /// Connect is unavailable until the named-pipe adapter lands.
    ///
    /// # Errors
    ///
    /// Always returns [`std::io::ErrorKind::Unsupported`] in this placeholder.
    pub fn connect(_endpoint: &LocalEndpoint) -> std::io::Result<Self> {
        Err(unsupported())
    }

    /// Clone is unavailable until the named-pipe adapter lands.
    ///
    /// # Errors
    ///
    /// Always returns [`std::io::ErrorKind::Unsupported`] in this placeholder.
    pub fn try_clone(&self) -> std::io::Result<Self> {
        Err(unsupported())
    }

    /// Set the read timeout.
    ///
    /// # Errors
    ///
    /// Always returns [`std::io::ErrorKind::Unsupported`] in this placeholder.
    pub fn set_read_timeout(&self, _timeout: Option<Duration>) -> std::io::Result<()> {
        Err(unsupported())
    }

    /// Set the write timeout.
    ///
    /// # Errors
    ///
    /// Always returns [`std::io::ErrorKind::Unsupported`] in this placeholder.
    pub fn set_write_timeout(&self, _timeout: Option<Duration>) -> std::io::Result<()> {
        Err(unsupported())
    }

    /// Half-close is unavailable until the named-pipe adapter lands.
    ///
    /// # Errors
    ///
    /// Always returns [`std::io::ErrorKind::Unsupported`] in this placeholder.
    pub fn shutdown_write(&self) -> std::io::Result<()> {
        Err(unsupported())
    }

    /// Test pairs are unavailable until the named-pipe adapter lands.
    ///
    /// # Errors
    ///
    /// Always returns [`std::io::ErrorKind::Unsupported`] in this placeholder.
    #[doc(hidden)]
    pub fn pair() -> std::io::Result<(Self, Self)> {
        Err(unsupported())
    }
}

impl Read for LocalConnection {
    fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
        Err(unsupported())
    }
}

impl Write for LocalConnection {
    fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
        Err(unsupported())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Err(unsupported())
    }
}
