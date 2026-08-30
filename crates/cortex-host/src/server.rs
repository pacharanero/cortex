// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Concurrent request serving and request-based daemon lifecycle.
//!
//! @see spec/200-cli/spec.md [FR-21] [FR-26] [FR-27] [FR-28]
//! @see spec/200-cli/design.md [DES-CLI]

use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::{LocalConnection, LocalListener, Request, Response};

const ACCEPT_POLL: Duration = Duration::from_millis(10);
const CONNECTION_POLL: Duration = Duration::from_millis(100);

/// Why a held daemon remains alive when it has no requests to serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonLifecycle {
    /// Started by a user and persistent until an explicit shutdown.
    Explicit,
    /// Started on behalf of another host and released after request inactivity.
    AutoManaged {
        /// Full inactivity period measured after each completed request.
        idle_timeout: Duration,
    },
}

impl DaemonLifecycle {
    /// Whether this lifecycle is managed on demand by a host.
    #[must_use]
    pub fn is_auto_managed(self) -> bool {
        matches!(self, Self::AutoManaged { .. })
    }

    /// Configured request-idle timeout, if any.
    #[must_use]
    pub fn idle_timeout(self) -> Option<Duration> {
        match self {
            Self::Explicit => None,
            Self::AutoManaged { idle_timeout } => Some(idle_timeout),
        }
    }
}

/// Why the listener stopped serving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerExit {
    /// A client sent [`Request::Shutdown`].
    ShutdownRequested,
    /// An auto-managed daemon had no request in flight for its timeout.
    IdleTimeout,
}

struct ActivityState {
    in_flight: usize,
    accepted_pending_read: usize,
    last_completed: Instant,
    shutting_down: bool,
}

struct Activity {
    state: Mutex<ActivityState>,
}

impl Activity {
    fn new() -> Self {
        Self {
            state: Mutex::new(ActivityState {
                in_flight: 0,
                accepted_pending_read: 0,
                last_completed: Instant::now(),
                shutting_down: false,
            }),
        }
    }

    fn accepted(self: &Arc<Self>) -> AcceptedGuard {
        self.state.lock().unwrap().accepted_pending_read += 1;
        AcceptedGuard {
            activity: Arc::clone(self),
            active: true,
        }
    }

    fn begin(self: &Arc<Self>) -> Option<RequestGuard> {
        let mut state = self.state.lock().unwrap();
        if state.shutting_down {
            return None;
        }
        state.in_flight += 1;
        Some(RequestGuard {
            activity: Arc::clone(self),
        })
    }

    fn begin_accepted(self: &Arc<Self>, accepted: &mut AcceptedGuard) -> Option<RequestGuard> {
        let mut state = self.state.lock().unwrap();
        state.accepted_pending_read = state.accepted_pending_read.saturating_sub(1);
        accepted.active = false;
        if state.shutting_down {
            return None;
        }
        state.in_flight += 1;
        Some(RequestGuard {
            activity: Arc::clone(self),
        })
    }

    fn begin_shutdown(&self) {
        self.state.lock().unwrap().shutting_down = true;
    }

    fn is_idle_for(&self, timeout: Duration) -> bool {
        let state = self.state.lock().unwrap();
        state.in_flight == 0
            && state.accepted_pending_read == 0
            && state.last_completed.elapsed() >= timeout
    }
}

struct AcceptedGuard {
    activity: Arc<Activity>,
    active: bool,
}

impl Drop for AcceptedGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let mut state = self.activity.state.lock().unwrap();
        state.accepted_pending_read = state.accepted_pending_read.saturating_sub(1);
    }
}

struct RequestGuard {
    activity: Arc<Activity>,
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        let mut state = self.activity.state.lock().unwrap();
        state.in_flight = state.in_flight.saturating_sub(1);
        state.last_completed = Instant::now();
    }
}

/// Serve local clients concurrently until shutdown or request inactivity.
///
/// A newly accepted connection receives one read poll of admission grace so a
/// queued request cannot lose a race with idle expiry. After that, activity
/// begins only when a complete line parses as a [`Request`]. The request
/// remains in flight through response writing, and its completion restarts an
/// auto-managed daemon's full idle timeout. Blank lines, malformed input, and
/// connections left open after the admission poll do not keep it alive.
///
/// # Errors
///
/// Returns listener configuration or non-transient accept errors.
pub fn serve_listener<F>(
    listener: &LocalListener,
    lifecycle: DaemonLifecycle,
    handler: &F,
) -> std::io::Result<ServerExit>
where
    F: Fn(Request) -> Response + Sync,
{
    if matches!(lifecycle, DaemonLifecycle::AutoManaged { idle_timeout } if idle_timeout.is_zero())
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "auto-managed idle timeout must be greater than zero",
        ));
    }

    listener.set_nonblocking(true)?;
    let stopping = Arc::new(AtomicBool::new(false));
    let explicit_shutdown = Arc::new(AtomicBool::new(false));
    let activity = Arc::new(Activity::new());

    std::thread::scope(|scope| -> std::io::Result<ServerExit> {
        let mut idle_candidate: Option<Instant> = None;
        loop {
            if stopping.load(Ordering::Acquire) {
                break;
            }

            match listener.accept() {
                Ok(connection) => {
                    idle_candidate = None;
                    let accepted = activity.accepted();
                    let activity = Arc::clone(&activity);
                    let stopping = Arc::clone(&stopping);
                    let explicit_shutdown = Arc::clone(&explicit_shutdown);
                    scope.spawn(move || {
                        serve_connection(
                            connection,
                            handler,
                            &activity,
                            &stopping,
                            &explicit_shutdown,
                            accepted,
                        );
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if lifecycle
                        .idle_timeout()
                        .is_some_and(|timeout| activity.is_idle_for(timeout))
                    {
                        // Keep polling for one connection-admission window after
                        // the first idle observation. A client can enter the
                        // listen backlog between a WouldBlock result and the
                        // clock check; this admits it before teardown wins.
                        if let Some(since) = idle_candidate {
                            if since.elapsed() >= CONNECTION_POLL {
                                stopping.store(true, Ordering::Release);
                                break;
                            }
                        } else {
                            idle_candidate = Some(Instant::now());
                        }
                    } else {
                        idle_candidate = None;
                    }
                    std::thread::sleep(ACCEPT_POLL);
                }
                Err(error) => return Err(error),
            }
        }

        Ok(if explicit_shutdown.load(Ordering::Acquire) {
            ServerExit::ShutdownRequested
        } else {
            ServerExit::IdleTimeout
        })
    })
}

fn serve_connection<F>(
    stream: LocalConnection,
    handler: &F,
    activity: &Arc<Activity>,
    stopping: &AtomicBool,
    explicit_shutdown: &AtomicBool,
    accepted: AcceptedGuard,
) where
    F: Fn(Request) -> Response,
{
    if stream.set_read_timeout(Some(CONNECTION_POLL)).is_err() {
        return;
    }
    let Ok(peer) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(peer);
    let mut writer = stream;
    let mut line = String::new();
    let mut accepted = Some(accepted);

    while !stopping.load(Ordering::Acquire) {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return,
            Ok(_) if line.trim().is_empty() => {
                accepted.take();
                continue;
            }
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                accepted.take();
                continue;
            }
            Err(_) => return,
        }

        let request = match serde_json::from_str::<Request>(&line) {
            Ok(request) => request,
            Err(error) => {
                accepted.take();
                if write_response(
                    &mut writer,
                    &Response::error(format!("could not parse request: {error}")),
                )
                .is_err()
                {
                    return;
                }
                continue;
            }
        };
        let shutdown = matches!(request, Request::Shutdown);
        if shutdown {
            accepted.take();
            activity.begin_shutdown();
            explicit_shutdown.store(true, Ordering::Release);
            stopping.store(true, Ordering::Release);
        }
        let request_guard = if shutdown {
            None
        } else if let Some(mut accepted) = accepted.take() {
            let Some(guard) = activity.begin_accepted(&mut accepted) else {
                return;
            };
            Some(guard)
        } else {
            let Some(guard) = activity.begin() else {
                return;
            };
            Some(guard)
        };
        let response = handler(request);
        let wrote_response = write_response(&mut writer, &response).is_ok();
        drop(request_guard);

        if shutdown {
            return;
        }
        if !wrote_response {
            return;
        }
    }
}

fn write_response(writer: &mut LocalConnection, response: &Response) -> std::io::Result<()> {
    serde_json::to_writer(&mut *writer, response)?;
    writer.write_all(b"\n")?;
    writer.flush()
}
