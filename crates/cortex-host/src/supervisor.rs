// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Lifecycle supervision for applications that share the held-session daemon.
//!
//! @see spec/200-cli/spec.md [FR-18] [FR-19]
//! @see spec/200-cli/design.md [DES-CLI]

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::{DaemonClient, Request, Status, client::ensure_compatible};

// The CLI launcher owns a 120-second startup timeout and cleans up its detached
// child. Leave it time to do that before this outer supervisor gives up.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(125);
const NANO_STARTUP_TIMEOUT: Duration = Duration::from_secs(250);
const STARTUP_POLL: Duration = Duration::from_millis(200);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// How a client should select or replace a product-scoped held session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevicePolicy {
    /// Reuse any compatible session, or start a Quad Cortex session.
    Any,
    /// Require a compatible session for this device.
    Require(cortex_rs::DeviceKind),
    /// Replace an unexpected owner of this device's endpoint, then start it.
    Replace(cortex_rs::DeviceKind),
    /// Reuse any compatible session, or try Quad Cortex then Nano Cortex.
    Detect,
}

struct ProductDaemon {
    client: Arc<DaemonClient>,
    probe: DaemonClient,
    startup: Mutex<()>,
}

impl ProductDaemon {
    fn new(device: cortex_rs::DeviceKind) -> Self {
        Self {
            client: Arc::new(DaemonClient::for_device(device)),
            probe: DaemonClient::for_device(device).with_timeout(Duration::from_secs(2)),
            startup: Mutex::new(()),
        }
    }
}

/// Starts and reuses product-scoped daemons that each own one Cortex HID interface.
///
/// The sibling `cortex` executable is launched detached with an idle timeout;
/// this object retains no child handle after the session has become ready.
pub struct DaemonSupervisor {
    quad: ProductDaemon,
    nano: ProductDaemon,
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
            quad: ProductDaemon::new(cortex_rs::DeviceKind::QuadCortex),
            nano: ProductDaemon::new(cortex_rs::DeviceKind::NanoCortex),
            idle_timeout,
        }
    }

    /// Ensure a compatible daemon satisfying `policy` is ready and return its client.
    ///
    /// # Errors
    ///
    /// Returns an actionable error when the sibling binary is unavailable, a
    /// daemon cannot start, a running daemon uses an incompatible protocol, or
    /// a required device is owned by another session.
    pub fn ensure(&self, policy: DevicePolicy) -> Result<Arc<DaemonClient>> {
        match policy {
            DevicePolicy::Any => self.ensure_any(false),
            DevicePolicy::Detect => self.ensure_any(true),
            DevicePolicy::Require(device) => self.ensure_device(device, false),
            DevicePolicy::Replace(device) => self.ensure_device(device, true),
        }
    }

    fn product(&self, device: cortex_rs::DeviceKind) -> &ProductDaemon {
        match device {
            cortex_rs::DeviceKind::QuadCortex => &self.quad,
            cortex_rs::DeviceKind::NanoCortex => &self.nano,
        }
    }

    fn ensure_any(&self, detect: bool) -> Result<Arc<DaemonClient>> {
        for device in [
            cortex_rs::DeviceKind::QuadCortex,
            cortex_rs::DeviceKind::NanoCortex,
        ] {
            let product = self.product(device);
            if let Ok(status) = product.probe.status() {
                ensure_compatible(&status, &Request::Status, Some(device), None)?;
                if status.device_kind == device {
                    return Ok(Arc::clone(&product.client));
                }
            }
        }

        match self.ensure_device(cortex_rs::DeviceKind::QuadCortex, false) {
            Ok(client) => Ok(client),
            Err(quad_error) if detect => self
                .ensure_device(cortex_rs::DeviceKind::NanoCortex, false)
                .map_err(|nano_error| {
                    anyhow::anyhow!(
                        "could not start a session for a connected Cortex device. Quad Cortex: {quad_error}; Nano Cortex: {nano_error}"
                    )
                }),
            Err(error) => Err(error),
        }
    }

    fn ensure_device(
        &self,
        device: cortex_rs::DeviceKind,
        replace_mismatch: bool,
    ) -> Result<Arc<DaemonClient>> {
        let product = self.product(device);
        let _startup = product
            .startup
            .lock()
            .map_err(|_| anyhow::anyhow!("daemon startup gate is unavailable"))?;

        if let Ok(status) = product.probe.status() {
            ensure_compatible(&status, &Request::Status, Some(device), None)?;
            return self.handle_running(device, product, &status, replace_mismatch);
        }
        if product.probe.is_running() {
            return self.wait_for_existing_session(device, product, replace_mismatch);
        }
        self.start(device, product)?;
        Ok(Arc::clone(&product.client))
    }

    fn handle_running(
        &self,
        device: cortex_rs::DeviceKind,
        product: &ProductDaemon,
        status: &Status,
        replace_mismatch: bool,
    ) -> Result<Arc<DaemonClient>> {
        if status.device_kind == device {
            return Ok(Arc::clone(&product.client));
        }
        if !replace_mismatch {
            anyhow::bail!(
                "the {:?} endpoint reports {:?}; this operation requires {:?}. Run `cortex session stop --device {}`, then retry.",
                device,
                status.device_kind,
                device,
                device_arg(device),
            );
        }
        product
            .client
            .request::<serde_json::Value>(&Request::Shutdown)?;
        Self::wait_for_session_stop(product)?;
        self.start(device, product)?;
        Ok(Arc::clone(&product.client))
    }

    fn wait_for_existing_session(
        &self,
        device: cortex_rs::DeviceKind,
        product: &ProductDaemon,
        replace_mismatch: bool,
    ) -> Result<Arc<DaemonClient>> {
        let timeout = startup_timeout(device);
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Ok(status) = product.probe.status() {
                ensure_compatible(&status, &Request::Status, Some(device), None)?;
                return self.handle_running(device, product, &status, replace_mismatch);
            }
            if !product.probe.is_running() {
                anyhow::bail!("the held cortex session stopped before it became ready");
            }
            std::thread::sleep(STARTUP_POLL);
        }
        anyhow::bail!(
            "the held cortex session did not become ready within {}s",
            timeout.as_secs()
        )
    }

    fn wait_for_session_stop(product: &ProductDaemon) -> Result<()> {
        let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
        while Instant::now() < deadline {
            if !product.probe.is_running() {
                return Ok(());
            }
            std::thread::sleep(STARTUP_POLL);
        }
        anyhow::bail!(
            "the previous cortex session did not release its endpoint within {}s",
            SHUTDOWN_TIMEOUT.as_secs()
        )
    }

    fn start(&self, device: cortex_rs::DeviceKind, product: &ProductDaemon) -> Result<()> {
        let sibling = sibling_cortex_binary()?;
        let idle_seconds = self.idle_timeout.as_secs().to_string();
        let mut command = Command::new(&sibling);
        command.args(sibling_start_args(device, &idle_seconds));
        let mut child = command
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("starting sibling cortex binary at {}", sibling.display()))?;
        let timeout = startup_timeout(device);
        let deadline = Instant::now() + timeout;

        while Instant::now() < deadline {
            if let Ok(status) = product.probe.status() {
                ensure_compatible(&status, &Request::Status, Some(device), None)?;
                if status.device_kind != device {
                    let _ = child.kill();
                    let _ = child.wait();
                    anyhow::bail!(
                        "the held session became ready for {:?} while starting {:?}",
                        status.device_kind,
                        device
                    );
                }
                // The child is the short-lived detached-session launcher. Reap
                // it after it exits without delaying readiness for the daemon.
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
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
            timeout.as_secs(),
            read_child_stderr(&mut child).trim()
        )
    }
}

fn device_arg(device: cortex_rs::DeviceKind) -> &'static str {
    match device {
        cortex_rs::DeviceKind::QuadCortex => "quad",
        cortex_rs::DeviceKind::NanoCortex => "nano",
    }
}

fn startup_timeout(device: cortex_rs::DeviceKind) -> Duration {
    match device {
        cortex_rs::DeviceKind::QuadCortex => STARTUP_TIMEOUT,
        // A new Nano daemon may first wait for a legacy endpoint owner to
        // identify itself, then perform its own hardware handshake.
        cortex_rs::DeviceKind::NanoCortex => NANO_STARTUP_TIMEOUT,
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

    fn status(device_kind: cortex_rs::DeviceKind) -> Status {
        Status {
            daemon_version: crate::DAEMON_PROTOCOL_VERSION.to_string(),
            uptime_seconds: 0,
            auto_managed: false,
            idle_timeout_seconds: None,
            device_kind,
            device: crate::DeviceHealth::Failed {
                error: "fixture".into(),
            },
            cache: crate::CacheStatus::default(),
        }
    }

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
    fn nano_startup_allows_for_legacy_owner_identification() {
        assert_eq!(
            startup_timeout(cortex_rs::DeviceKind::QuadCortex),
            Duration::from_secs(125)
        );
        assert_eq!(
            startup_timeout(cortex_rs::DeviceKind::NanoCortex),
            Duration::from_secs(250)
        );
    }

    #[test]
    fn running_products_return_their_product_scoped_clients() {
        let supervisor = DaemonSupervisor::default();

        let quad = supervisor
            .handle_running(
                cortex_rs::DeviceKind::QuadCortex,
                &supervisor.quad,
                &status(cortex_rs::DeviceKind::QuadCortex),
                false,
            )
            .unwrap();
        let nano = supervisor
            .handle_running(
                cortex_rs::DeviceKind::NanoCortex,
                &supervisor.nano,
                &status(cortex_rs::DeviceKind::NanoCortex),
                false,
            )
            .unwrap();

        assert!(Arc::ptr_eq(&quad, &supervisor.quad.client));
        assert!(Arc::ptr_eq(&nano, &supervisor.nano.client));
        assert!(!Arc::ptr_eq(&quad, &nano));
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
