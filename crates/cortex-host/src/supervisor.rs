// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Lifecycle supervision for applications that share the held-session daemon.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::{DaemonClient, Request, Status, client::ensure_compatible};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(120);
const STARTUP_POLL: Duration = Duration::from_millis(200);

/// How a client should handle the device owned by a held session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevicePolicy {
    /// Reuse any compatible session, or start a Quad Cortex session.
    Any,
    /// Require a compatible session for this device, without replacing another.
    Require(cortex_rs::DeviceKind),
    /// Start this device and replace a compatible session for another device.
    Replace(cortex_rs::DeviceKind),
    /// Reuse any compatible session, or try Quad Cortex then Nano Cortex.
    Detect,
}

/// Starts and reuses the one local daemon that owns a Cortex HID interface.
///
/// The sibling `cortex` executable is launched detached with an idle timeout;
/// this object retains no child handle after the session has become ready.
pub struct DaemonSupervisor {
    client: Arc<DaemonClient>,
    probe: DaemonClient,
    startup: Mutex<()>,
    idle_timeout: Duration,
}

impl Default for DaemonSupervisor {
    fn default() -> Self {
        Self::new(Duration::from_secs(60))
    }
}

impl DaemonSupervisor {
    /// Create a supervisor with the auto-managed session idle timeout.
    #[must_use]
    pub fn new(idle_timeout: Duration) -> Self {
        Self {
            client: Arc::new(DaemonClient::default()),
            probe: DaemonClient::default().with_timeout(Duration::from_secs(2)),
            startup: Mutex::new(()),
            idle_timeout,
        }
    }

    /// Return the reusable client for requests after [`Self::ensure`].
    #[must_use]
    pub fn client(&self) -> Arc<DaemonClient> {
        Arc::clone(&self.client)
    }

    /// Ensure a compatible daemon satisfying `policy` is ready.
    ///
    /// # Errors
    ///
    /// Returns an actionable error when the sibling binary is unavailable, a
    /// daemon cannot start, a running daemon uses an incompatible protocol, or
    /// a required device is owned by another session.
    pub fn ensure(&self, policy: DevicePolicy) -> Result<()> {
        let _startup = self
            .startup
            .lock()
            .map_err(|_| anyhow::anyhow!("daemon startup gate is unavailable"))?;

        if let Ok(status) = self.probe.status() {
            ensure_compatible(&status, &Request::Status)?;
            return self.handle_running(&status, policy);
        }
        if self.probe.is_running() {
            return self.wait_for_existing_session();
        }

        match policy {
            DevicePolicy::Detect => {
                match self.start(cortex_rs::DeviceKind::QuadCortex) {
                    Ok(()) => Ok(()),
                    Err(quad_error) => self
                        .start(cortex_rs::DeviceKind::NanoCortex)
                        .map_err(|nano_error| {
                            anyhow::anyhow!(
                                "could not start a session for a connected Cortex device. Quad Cortex: {quad_error}; Nano Cortex: {nano_error}"
                            )
                        }),
                }
            }
            DevicePolicy::Any => self.start(cortex_rs::DeviceKind::QuadCortex),
            DevicePolicy::Require(device) | DevicePolicy::Replace(device) => self.start(device),
        }
    }

    fn handle_running(&self, status: &Status, policy: DevicePolicy) -> Result<()> {
        match policy {
            DevicePolicy::Require(device) if status.device_kind != device => anyhow::bail!(
                "the held session owns {:?}; this operation requires {:?}. Run `cortex session stop`, then retry.",
                status.device_kind,
                device
            ),
            DevicePolicy::Replace(device) if status.device_kind != device => {
                self.client.request::<bool>(&Request::Shutdown)?;
                std::thread::sleep(Duration::from_millis(500));
                self.start(device)
            }
            DevicePolicy::Any
            | DevicePolicy::Detect
            | DevicePolicy::Require(_)
            | DevicePolicy::Replace(_) => Ok(()),
        }
    }

    fn wait_for_existing_session(&self) -> Result<()> {
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        while Instant::now() < deadline {
            if let Ok(status) = self.probe.status() {
                return ensure_compatible(&status, &Request::Status);
            }
            if !self.probe.is_running() {
                anyhow::bail!("the held cortex session stopped before it became ready");
            }
            std::thread::sleep(STARTUP_POLL);
        }
        anyhow::bail!(
            "the held cortex session did not become ready within {}s",
            STARTUP_TIMEOUT.as_secs()
        )
    }

    fn start(&self, device: cortex_rs::DeviceKind) -> Result<()> {
        let sibling = sibling_cortex_binary()?;
        let idle_seconds = self.idle_timeout.as_secs().to_string();
        let mut command = Command::new(&sibling);
        command.args(sibling_start_args(device, &idle_seconds));
        let mut child = command
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("starting sibling cortex binary at {}", sibling.display()))?;
        let deadline = Instant::now() + STARTUP_TIMEOUT;

        while Instant::now() < deadline {
            if let Ok(status) = self.probe.status() {
                ensure_compatible(&status, &Request::Status)?;
                return Ok(());
            }
            if let Some(status) = child.try_wait().context("waiting for sibling cortex")? {
                let stderr = read_child_stderr(&mut child);
                if !status.success() {
                    anyhow::bail!(
                        "sibling cortex could not start an auto-managed session ({status}): {}",
                        stderr.trim()
                    );
                }
            }
            std::thread::sleep(STARTUP_POLL);
        }

        let _ = child.kill();
        let _ = child.wait();
        anyhow::bail!(
            "sibling cortex did not finish startup within {}s: {}",
            STARTUP_TIMEOUT.as_secs(),
            read_child_stderr(&mut child).trim()
        )
    }
}

fn sibling_start_args(device: cortex_rs::DeviceKind, idle_seconds: &str) -> Vec<String> {
    let mut args = vec![
        "session".into(),
        "start".into(),
        "--auto-managed".into(),
        "--idle-timeout-seconds".into(),
        idle_seconds.into(),
    ];
    if device == cortex_rs::DeviceKind::NanoCortex {
        args.extend(["--device".into(), "nano".into()]);
    }
    args
}

fn sibling_cortex_binary() -> Result<PathBuf> {
    let path = if let Some(path) = std::env::var_os("CORTEX_CLI_PATH") {
        PathBuf::from(path)
    } else {
        sibling_path(&std::env::current_exe().context("locating client executable")?)
    };
    if !path.is_file() {
        anyhow::bail!(
            "could not find the sibling cortex binary at {}; install cortex with this client, or set CORTEX_CLI_PATH",
            path.display()
        );
    }
    Ok(path)
}

fn sibling_path(current: &Path) -> PathBuf {
    current.with_file_name(format!("cortex{}", std::env::consts::EXE_SUFFIX))
}

fn read_child_stderr(child: &mut std::process::Child) -> String {
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    stderr
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nano_start_selects_the_nano_device() {
        assert_eq!(
            sibling_start_args(cortex_rs::DeviceKind::NanoCortex, "60"),
            [
                "session",
                "start",
                "--auto-managed",
                "--idle-timeout-seconds",
                "60",
                "--device",
                "nano",
            ]
        );
        assert!(
            !sibling_start_args(cortex_rs::DeviceKind::QuadCortex, "60")
                .iter()
                .any(|argument| argument == "--device")
        );
    }

    #[test]
    fn sibling_binary_uses_the_clients_directory() {
        let current = PathBuf::from(format!(
            "/opt/cortex/bin/cortex-mcp{}",
            std::env::consts::EXE_SUFFIX
        ));
        assert_eq!(
            sibling_path(&current),
            PathBuf::from(format!(
                "/opt/cortex/bin/cortex{}",
                std::env::consts::EXE_SUFFIX
            ))
        );
    }
}
