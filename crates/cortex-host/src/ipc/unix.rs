// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Unix domain socket adapter behind the local IPC facade.
//!
//! @see spec/200-cli/spec.md [FR-18]
//! @see spec/200-cli/design.md [DES-CLI]

use std::fmt;
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

/// Platform-specific address of the local daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalEndpoint {
    path: PathBuf,
}

impl LocalEndpoint {
    /// The current user's Cortex daemon endpoint.
    #[must_use]
    pub fn daemon() -> Self {
        Self::for_device(cortex_rs::DeviceKind::QuadCortex)
    }

    /// The current user's daemon endpoint for one Cortex product.
    #[must_use]
    pub fn for_device(device: cortex_rs::DeviceKind) -> Self {
        let socket_name = match device {
            cortex_rs::DeviceKind::QuadCortex => "cortex.sock",
            cortex_rs::DeviceKind::NanoCortex => "cortex-nano.sock",
        };
        let path = if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
            PathBuf::from(dir).join(socket_name)
        } else {
            let uid = std::env::var("UID").unwrap_or_else(|_| {
                let identity =
                    std::env::var_os("HOME").map_or_else(std::env::temp_dir, PathBuf::from);
                format!("h{:016x}", stable_hash(&identity.to_string_lossy()))
            });
            let suffix = match device {
                cortex_rs::DeviceKind::QuadCortex => "",
                cortex_rs::DeviceKind::NanoCortex => "-nano",
            };
            std::env::temp_dir().join(format!("cortex-{uid}{suffix}.sock"))
        };
        Self { path }
    }

    /// Build an endpoint around an explicit Unix socket path.
    #[must_use]
    pub fn at(path: impl AsRef<std::path::Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    /// Place for logs from the detached daemon process.
    #[must_use]
    pub fn log_path(&self) -> PathBuf {
        let mut path = self.path.clone();
        path.set_extension("log");
        path
    }

    pub(crate) fn has_active_claim(&self) -> bool {
        let claim = match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(self.path.with_extension("lock"))
        {
            Ok(claim) => claim,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
            Err(_) => return true,
        };
        match fs4::FileExt::try_lock(&claim) {
            Ok(()) => false,
            Err(_) => true,
        }
    }

    fn cleanup(&self) -> std::io::Result<()> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

fn stable_hash(value: &str) -> u64 {
    value.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

impl fmt::Display for LocalEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.path.display().fmt(formatter)
    }
}

/// Result of claiming the local daemon endpoint.
pub struct BindResult {
    /// Listener holding the endpoint claim.
    pub listener: LocalListener,
    /// Whether an inert socket from a dead process was removed first.
    pub removed_stale_endpoint: bool,
}

/// Process-lifetime ownership claim shared by daemon and direct device access.
pub struct LocalClaim {
    _file: File,
}

impl LocalClaim {
    /// Atomically claim the current user's Cortex device ownership slot.
    ///
    /// # Errors
    ///
    /// Returns [`std::io::ErrorKind::AddrInUse`] when another process owns the
    /// claim, or the underlying lock-file error.
    pub fn acquire(endpoint: &LocalEndpoint) -> std::io::Result<Self> {
        let claim_path = endpoint.path.with_extension("lock");
        let claim = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&claim_path)?;
        fs4::FileExt::try_lock(&claim).map_err(|error| {
            let error: std::io::Error = error.into();
            if error.kind() == std::io::ErrorKind::WouldBlock {
                std::io::Error::new(
                    std::io::ErrorKind::AddrInUse,
                    format!("another process already owns {endpoint}"),
                )
            } else {
                error
            }
        })?;
        Ok(Self { _file: claim })
    }
}

/// Listener for local daemon clients.
pub struct LocalListener {
    inner: UnixListener,
    endpoint: LocalEndpoint,
    _claim: LocalClaim,
}

impl LocalListener {
    /// Claim an endpoint, refusing a live owner and clearing an inert socket.
    ///
    /// # Errors
    ///
    /// Returns [`std::io::ErrorKind::AddrInUse`] for a live owner, or an
    /// underlying filesystem/socket error while cleaning up or binding.
    pub fn bind(endpoint: &LocalEndpoint) -> std::io::Result<BindResult> {
        let claim = LocalClaim::acquire(endpoint)?;

        let mut removed_stale_endpoint = false;
        if endpoint.path.exists() {
            if UnixStream::connect(&endpoint.path).is_ok() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AddrInUse,
                    format!("a cortex daemon is already running on {endpoint}"),
                ));
            }
            endpoint.cleanup()?;
            removed_stale_endpoint = true;
        }
        let inner = UnixListener::bind(&endpoint.path)?;
        std::fs::set_permissions(&endpoint.path, std::fs::Permissions::from_mode(0o600))?;
        Ok(BindResult {
            listener: Self {
                inner,
                endpoint: endpoint.clone(),
                _claim: claim,
            },
            removed_stale_endpoint,
        })
    }

    /// Accept one local client connection.
    ///
    /// # Errors
    ///
    /// Returns the socket accept error.
    pub fn accept(&self) -> std::io::Result<LocalConnection> {
        self.inner
            .accept()
            .map(|(inner, _)| LocalConnection { inner })
    }

    /// Configure whether accept returns immediately when no client is waiting.
    ///
    /// # Errors
    ///
    /// Returns the socket option error.
    pub fn set_nonblocking(&self, nonblocking: bool) -> std::io::Result<()> {
        self.inner.set_nonblocking(nonblocking)
    }

    /// Remove this listener's filesystem endpoint while retaining its atomic claim.
    ///
    /// # Errors
    ///
    /// Returns filesystem errors other than an already-absent endpoint.
    pub fn cleanup_endpoint(&self) -> std::io::Result<()> {
        self.endpoint.cleanup()
    }
}

/// Duplex byte stream to the local daemon.
pub struct LocalConnection {
    inner: UnixStream,
}

impl LocalConnection {
    /// Connect to one endpoint.
    ///
    /// # Errors
    ///
    /// Returns the socket connection error.
    pub fn connect(endpoint: &LocalEndpoint) -> std::io::Result<Self> {
        UnixStream::connect(&endpoint.path).map(|inner| Self { inner })
    }

    /// Clone the stream handle for independent buffered reading and writing.
    ///
    /// # Errors
    ///
    /// Returns the operating-system handle duplication error.
    pub fn try_clone(&self) -> std::io::Result<Self> {
        self.inner.try_clone().map(|inner| Self { inner })
    }

    /// Set the read timeout.
    ///
    /// # Errors
    ///
    /// Returns the socket option error.
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        self.inner.set_read_timeout(timeout)
    }

    /// Set the write timeout.
    ///
    /// # Errors
    ///
    /// Returns the socket option error.
    pub fn set_write_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        self.inner.set_write_timeout(timeout)
    }

    /// Connected pair used by transport-independent protocol tests.
    ///
    /// # Errors
    ///
    /// Returns the socket-pair creation error.
    #[doc(hidden)]
    pub fn pair() -> std::io::Result<(Self, Self)> {
        UnixStream::pair().map(|(left, right)| (Self { inner: left }, Self { inner: right }))
    }
}

impl Read for LocalConnection {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buffer)
    }
}

impl Write for LocalConnection {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.inner.write(buffer)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_endpoint(label: &str) -> LocalEndpoint {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        LocalEndpoint::at(std::env::temp_dir().join(format!(
            "cortex-{label}-{}-{unique}.sock",
            std::process::id()
        )))
    }

    #[test]
    fn daemon_endpoint_follows_xdg_runtime_dir() {
        let endpoint = LocalEndpoint::at(PathBuf::from("/tmp/fictional-runtime/cortex.sock"));
        assert_eq!(endpoint.to_string(), "/tmp/fictional-runtime/cortex.sock");
        assert_eq!(
            endpoint.log_path(),
            PathBuf::from("/tmp/fictional-runtime/cortex.log")
        );
    }

    #[test]
    fn products_have_distinct_endpoints_and_quad_keeps_the_legacy_name() {
        let quad = LocalEndpoint::for_device(cortex_rs::DeviceKind::QuadCortex);
        let nano = LocalEndpoint::for_device(cortex_rs::DeviceKind::NanoCortex);

        assert_eq!(quad, LocalEndpoint::daemon());
        assert_ne!(quad, nano);
        assert!(quad.to_string().ends_with("cortex.sock"));
        assert!(nano.to_string().ends_with("cortex-nano.sock"));
        assert!(nano.log_path().ends_with("cortex-nano.log"));
    }

    #[test]
    fn binding_clears_an_inert_endpoint_but_refuses_a_live_owner() {
        let endpoint = test_endpoint("claim");
        std::fs::write(endpoint.to_string(), b"stale").unwrap();

        let claim = LocalListener::bind(&endpoint).unwrap();
        assert!(claim.removed_stale_endpoint);
        assert!(LocalConnection::connect(&endpoint).is_ok());

        let error = LocalListener::bind(&endpoint).err().unwrap();
        assert_eq!(error.kind(), std::io::ErrorKind::AddrInUse);

        claim.listener.cleanup_endpoint().unwrap();
        drop(claim.listener);
        std::fs::remove_file(endpoint.path.with_extension("lock")).unwrap();
    }

    #[test]
    fn concurrent_stale_cleanup_has_one_winner() {
        let endpoint = test_endpoint("race");
        std::fs::write(endpoint.to_string(), b"stale").unwrap();
        let start = std::sync::Arc::new(std::sync::Barrier::new(3));
        let release =
            std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let (outcome_tx, outcome_rx) = std::sync::mpsc::channel();

        let workers = (0..2)
            .map(|_| {
                let endpoint = endpoint.clone();
                let start = start.clone();
                let release = release.clone();
                let outcome_tx = outcome_tx.clone();
                std::thread::spawn(move || {
                    start.wait();
                    match LocalListener::bind(&endpoint) {
                        Ok(claim) => {
                            outcome_tx.send(Ok(())).unwrap();
                            let (lock, changed) = &*release;
                            let mut done = lock.lock().unwrap();
                            while !*done {
                                done = changed.wait(done).unwrap();
                            }
                            claim.listener.cleanup_endpoint().unwrap();
                        }
                        Err(error) => outcome_tx.send(Err(error.kind())).unwrap(),
                    }
                })
            })
            .collect::<Vec<_>>();
        start.wait();
        let outcomes = [outcome_rx.recv().unwrap(), outcome_rx.recv().unwrap()];
        assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            outcomes
                .iter()
                .filter(|result| matches!(result, Err(std::io::ErrorKind::AddrInUse)))
                .count(),
            1
        );

        let (lock, changed) = &*release;
        *lock.lock().unwrap() = true;
        changed.notify_all();
        for worker in workers {
            worker.join().unwrap();
        }
        std::fs::remove_file(endpoint.path.with_extension("lock")).unwrap();
    }

    #[test]
    fn an_active_claim_counts_before_the_listener_exists() {
        let endpoint = test_endpoint("startup");
        let claim_path = endpoint.path.with_extension("lock");
        let claim = LocalClaim::acquire(&endpoint).unwrap();

        assert!(endpoint.has_active_claim());

        drop(claim);
        assert!(!endpoint.has_active_claim());
        std::fs::remove_file(claim_path).unwrap();
    }
}
