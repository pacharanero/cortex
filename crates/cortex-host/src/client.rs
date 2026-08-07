// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Synchronous client for the held-session daemon.

use std::io::{BufRead, BufReader, Write};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;

use crate::{DAEMON_PROTOCOL_VERSION, LocalConnection, LocalEndpoint, Request, Response, Status};

const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// A reusable client configuration for short-lived daemon connections.
#[derive(Debug, Clone)]
pub struct DaemonClient {
    endpoint: LocalEndpoint,
    timeout: Duration,
}

impl Default for DaemonClient {
    fn default() -> Self {
        Self::new(LocalEndpoint::daemon())
    }
}

impl DaemonClient {
    /// Create a client for an explicit local IPC endpoint.
    #[must_use]
    pub fn new(endpoint: LocalEndpoint) -> Self {
        Self {
            endpoint,
            timeout: DEFAULT_REQUEST_TIMEOUT,
        }
    }

    /// Override the read and write timeout.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Whether a daemon is listening or has claimed the endpoint while starting.
    #[must_use]
    pub fn is_running(&self) -> bool {
        LocalConnection::connect(&self.endpoint).is_ok() || self.endpoint.has_active_claim()
    }

    /// Require a compatible held session and return its status.
    ///
    /// # Errors
    ///
    /// Returns an error when no daemon is listening, socket I/O fails, the
    /// response is malformed, or the daemon protocol version is incompatible.
    pub fn require_compatible(&self) -> Result<Status> {
        let stream = LocalConnection::connect(&self.endpoint).with_context(|| {
            format!(
                "no held cortex session is listening at {}; start one with `cortex session start`",
                self.endpoint
            )
        })?;
        let (mut writer, mut reader) = self.prepare(stream)?;
        Self::read_compatible_status(&mut writer, &mut reader)
    }

    /// Send one request and return its untyped success payload.
    ///
    /// # Errors
    ///
    /// Returns an error for socket I/O, protocol-version mismatch, a malformed
    /// response, or an error returned by the held session.
    pub fn request_value(&self, request: &Request) -> Result<serde_json::Value> {
        let stream = LocalConnection::connect(&self.endpoint)
            .with_context(|| format!("connecting to cortex session at {}", self.endpoint))?;
        let (mut writer, mut reader) = self.prepare(stream)?;
        Self::read_compatible_status(&mut writer, &mut reader)?;
        write_request(&mut writer, request)?;
        read_response(&mut reader)
    }

    /// Send one request and deserialize its success payload.
    ///
    /// # Errors
    ///
    /// Returns the errors from [`Self::request_value`] and fails when the
    /// success payload does not match `T`.
    pub fn request<T: DeserializeOwned>(&self, request: &Request) -> Result<T> {
        let value = self.request_value(request)?;
        serde_json::from_value(value).context("decoding cortex session response")
    }

    fn prepare(
        &self,
        stream: LocalConnection,
    ) -> Result<(LocalConnection, BufReader<LocalConnection>)> {
        stream.set_read_timeout(Some(self.timeout))?;
        stream.set_write_timeout(Some(self.timeout))?;
        let writer = stream.try_clone()?;
        Ok((writer, BufReader::new(stream)))
    }

    fn read_compatible_status(
        writer: &mut LocalConnection,
        reader: &mut BufReader<LocalConnection>,
    ) -> Result<Status> {
        write_request(writer, &Request::Status)?;
        let value = read_response(reader)?;
        let status: Status = serde_json::from_value(value).context("decoding daemon status")?;
        let daemon_version = status.daemon_version.parse::<u32>().unwrap_or(0);
        if daemon_version != DAEMON_PROTOCOL_VERSION {
            anyhow::bail!(
                "daemon protocol version mismatch: client expects {DAEMON_PROTOCOL_VERSION}, daemon reports {daemon_version}. Run `cortex session stop` to stop the old daemon, then retry."
            );
        }
        Ok(status)
    }
}

fn write_request(writer: &mut LocalConnection, request: &Request) -> Result<()> {
    serde_json::to_writer(&mut *writer, request)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn read_response(reader: &mut BufReader<LocalConnection>) -> Result<serde_json::Value> {
    let mut reply = String::new();
    let read = reader.read_line(&mut reply)?;
    if read == 0 || reply.trim().is_empty() {
        anyhow::bail!("cortex session closed without a response");
    }
    match serde_json::from_str::<Response>(&reply).context("decoding cortex session envelope")? {
        Response::Ok { data } => Ok(data),
        Response::Error { message } => anyhow::bail!(message),
    }
}

/// Whether the default local daemon endpoint accepts a connection.
#[must_use]
pub fn is_running() -> bool {
    DaemonClient::default().is_running()
}

/// Send one request if a daemon is listening, preserving CLI direct fallback.
///
/// The outer `Option` distinguishes an absent daemon from a daemon request
/// that failed after connecting.
#[must_use]
pub fn request(request: &Request) -> Option<Result<serde_json::Value>> {
    let client = DaemonClient::default();
    if !client.is_running() {
        return None;
    }
    Some(client.request_value(request))
}
