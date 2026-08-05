// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! `cortex session start`: one held session, served over a unix socket.
//!
//! The device grants its HID interface exclusively, so exactly one process
//! can own it. This is that process. Everything else talks to it.
//!
//! @see spec/roadmap.md PROT-008.6
//! @see spec/140-session/spec.md

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use cortex_rs::daemon::{CacheStatus, DeviceHealth, Request, Response, Status, socket_path};
use cortex_rs::{DeviceKind, QuadCortex, Session};

/// How long a request may take before the daemon gives up on the device.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// How long teardown gets before the process exits regardless.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(3);

/// How often the held session's measured liveness is checked.
const HEALTH_POLL: Duration = Duration::from_secs(1);

/// Reconnect starts quickly, then backs off to this ceiling while unplugged.
const MAX_RECONNECT_BACKOFF: Duration = Duration::from_secs(30);

#[derive(Clone, Copy)]
struct ReconnectTimings {
    poll: Duration,
    initial_backoff: Duration,
    max_backoff: Duration,
}

const RECONNECT_TIMINGS: ReconnectTimings = ReconnectTimings {
    poll: HEALTH_POLL,
    initial_backoff: Duration::from_secs(1),
    max_backoff: MAX_RECONNECT_BACKOFF,
};

#[derive(Clone)]
enum RuntimeHealth {
    Connected,
    Reconnecting { attempts: u32, last_error: String },
    Failed { error: String },
}

/// State the daemon holds for the life of its session.
struct Daemon {
    /// Swapped atomically at the host layer after a successful reconnect.
    session: Arc<Mutex<Arc<Session>>>,
    state: cortex_rs::DeviceStateCache,
    /// Parsed lazily and keyed by the cached payload's generation/revision.
    catalog: Mutex<Option<(u64, u64, cortex_rs::Catalog)>>,
    /// Prepared-save tokens, held server-side so the raw backup never
    /// crosses the socket. A token is consumed by `CommitSave`. Each entry
    /// carries the policy that was validated at prepare time, so the commit
    /// can re-authorize without the client resending it.
    preparations: Mutex<
        HashMap<
            String,
            (
                cortex_rs::safety::SavePolicy,
                cortex_rs::safety::SavePreparation,
            ),
        >,
    >,
    /// Next preparation token id.
    next_token: AtomicU64,
    health: Arc<Mutex<RuntimeHealth>>,
    stopping: Arc<AtomicBool>,
    started: Instant,
}

impl Daemon {
    /// Open the device and perform the SUBSCRIBED handshake.
    ///
    /// Subscribed, not minimal, and that is the whole point: subscribing is
    /// how the device reports edits made by the PLAYER on the hardware. It
    /// is expensive per command and correct once per session.
    fn connect() -> Result<Self> {
        eprintln!("cortex session: opening device ...");
        let session = Arc::new(Session::open(DeviceKind::QuadCortex)?);

        let started = Instant::now();
        session.connect_with_progress(
            cortex_rs::ConnectMode::Subscribed,
            Duration::from_secs(15),
            Duration::from_secs(2),
            |step| eprintln!("cortex session:   {step} ..."),
        )?;
        eprintln!(
            "cortex session: connected in {:.1}s",
            started.elapsed().as_secs_f32()
        );

        let daemon = Self::over(session);
        daemon.seed_live_state();
        Ok(daemon)
    }

    /// Build a daemon around an already-connected session.
    ///
    /// Separated from [`Self::connect`] so the request handling can be
    /// exercised without a device: everything interesting here - the socket
    /// protocol, the status shape, how a failed call is reported - is
    /// independent of how the session was obtained.
    fn over(session: Arc<Session>) -> Self {
        let state = session.state_cache();
        Self {
            session: Arc::new(Mutex::new(session)),
            state,
            catalog: Mutex::new(None),
            preparations: Mutex::new(HashMap::new()),
            next_token: AtomicU64::new(1),
            health: Arc::new(Mutex::new(RuntimeHealth::Connected)),
            stopping: Arc::new(AtomicBool::new(false)),
            started: Instant::now(),
        }
    }

    fn session(&self) -> Arc<Session> {
        self.session.lock().unwrap().clone()
    }

    fn client(&self) -> QuadCortex {
        QuadCortex::new(self.session())
    }

    /// Repair fields absent from the initial subscription dump before the
    /// background start command reports ready. Each explicit read is also
    /// observed by the non-consuming reducer before its waiter returns.
    fn seed_live_state(&self) {
        seed_session(&self.session(), &self.state);
    }

    /// Parsed model catalog matching the cache's exact payload revision.
    fn catalog(&self) -> Option<cortex_rs::Catalog> {
        let payload = self.state.model_repo()?;
        let mut parsed = self.catalog.lock().unwrap();
        if let Some((generation, revision, catalog)) = parsed.as_ref() {
            if *generation == payload.generation && *revision == payload.revision {
                return Some(catalog.clone());
            }
        }
        let catalog = cortex_rs::Catalog::parse(&payload.value).ok()?;
        *parsed = Some((payload.generation, payload.revision, catalog.clone()));
        Some(catalog)
    }

    /// Cache hits must not conceal a device that has stopped talking.
    fn cache_is_usable(&self) -> bool {
        self.session().is_responsive() && self.state.status().phase == cortex_rs::CachePhase::Live
    }

    /// A structural sparse push intentionally invalidates the live preset.
    /// Repair it with the side-effect-free read, never by guessing defaults.
    fn repair_live_preset(&self, timeout: Duration) {
        if self.state.current_preset().is_none() {
            if let Err(error) = self.client().read_current_preset(timeout) {
                eprintln!("cortex session: warning: live-grid cache refresh failed ({error})");
            }
        }
    }

    /// Explain why a device request cannot currently run.
    fn unavailable(&self) -> Option<String> {
        match self.health.lock().unwrap().clone() {
            RuntimeHealth::Connected => None,
            RuntimeHealth::Reconnecting {
                attempts,
                last_error,
            } => Some(format!(
                "device reconnecting (attempt {attempts}): {last_error}"
            )),
            RuntimeHealth::Failed { error } => Some(format!("device connection failed: {error}")),
        }
    }

    /// Handle one request.
    ///
    /// Every arm returns a `Response` rather than propagating, because a
    /// failed request must not take the daemon down with it - the session is
    /// shared by every other client.
    fn handle(&self, request: Request) -> Response {
        if !matches!(&request, Request::Status | Request::Shutdown) {
            if let Some(message) = self.unavailable() {
                return Response::error(message);
            }
        }
        match request {
            Request::Status => match Response::ok(&self.status()) {
                Ok(r) => r,
                Err(e) => Response::error(format!("building status: {e}")),
            },
            // Answer in the same shape the direct path emits, so a caller
            // cannot tell whether it went through the daemon. A `{:?}` dump
            // of the protobuf would have been a second, worse format.
            //
            // Served from the handshake's own READ where possible. That is
            // not merely faster: asking again would race the handshake's
            // announce, because `Version` READ replies carry no `request_id`
            // and a waiter matching on type alone cannot tell them apart.
            Request::Version => match self.session().device_version() {
                Some(v) => Response::ok(&cortex_rs::view::DeviceVersion::from(&v))
                    .unwrap_or_else(|e| Response::error(format!("DeviceVersion: {e}"))),
                None => self.respond(|c| {
                    c.version(REQUEST_TIMEOUT)
                        .map(|v| cortex_rs::view::DeviceVersion::from(&v))
                }),
            },
            Request::ActiveScene => {
                if self.cache_is_usable() {
                    if let Some(scene) = self.state.active_scene() {
                        return Response::ok(&scene.value).unwrap_or_else(|e| {
                            Response::error(format!("serialising active scene: {e}"))
                        });
                    }
                }
                self.respond(|c| c.active_scene(REQUEST_TIMEOUT))
            }
            Request::SwitchScene { scene } => self.respond(|c| {
                c.switch_scene(scene)
                    .map(|()| serde_json::json!({ "scene": scene }))
            }),
            Request::CurrentPreset {
                with_params,
                timeout_seconds,
            } => {
                let catalog = self.catalog();
                if self.cache_is_usable() {
                    if let Some(preset) = self.state.current_preset() {
                        let view = cortex_rs::view::Preset::from_binary(
                            &preset.value,
                            catalog.as_ref(),
                            "(live grid)",
                            "(live grid)",
                            with_params,
                        );
                        return Response::ok(&view).unwrap_or_else(|e| {
                            Response::error(format!("serialising live preset: {e}"))
                        });
                    }
                }
                self.respond(|c| {
                    c.read_current_preset(Duration::from_secs(timeout_seconds))
                        .map(|preset| {
                            cortex_rs::view::Preset::from_binary(
                                &preset,
                                catalog.as_ref(),
                                "(live grid)",
                                "(live grid)",
                                with_params,
                            )
                        })
                })
            }
            Request::ReadPreset {
                setlist,
                slot,
                factory,
                with_params,
                timeout_seconds,
            } => {
                let catalog = self.catalog();
                self.respond(move |c| {
                    c.read_preset(
                        &setlist,
                        &slot,
                        factory,
                        Duration::from_secs(timeout_seconds),
                    )
                    .map(|preset| {
                        cortex_rs::view::Preset::from_binary(
                            &preset,
                            catalog.as_ref(),
                            &slot,
                            &setlist,
                            with_params,
                        )
                    })
                })
            }
            Request::RecallPreset {
                setlist,
                slot,
                factory,
            } => self.respond(move |c| {
                c.recall_preset(&setlist, &slot, factory)
                    .map(|()| serde_json::json!({ "slot": slot }))
            }),
            Request::ListPresets {
                setlist,
                include_empty,
                timeout_seconds,
            } => {
                if self.cache_is_usable() {
                    if let Some(entries) = self.client().cached_presets(&setlist, include_empty) {
                        let slots: Vec<_> = entries
                            .iter()
                            .map(cortex_rs::view::PresetSlot::from)
                            .collect();
                        return Response::ok(&slots).unwrap_or_else(|e| {
                            Response::error(format!("serialising preset listing: {e}"))
                        });
                    }
                }
                self.respond(move |c| {
                    c.list_presets(
                        &setlist,
                        Duration::from_secs(timeout_seconds),
                        include_empty,
                    )
                    .map(|entries| {
                        entries
                            .iter()
                            .map(cortex_rs::view::PresetSlot::from)
                            .collect::<Vec<_>>()
                    })
                })
            }
            Request::ListFolders { window_seconds } => {
                self.respond(|c| c.list_folders(Duration::from_secs(window_seconds)))
            }
            // Return the raw payload, not a summary. The caller parses it
            // with the same code the direct path uses, so the two cannot
            // render different catalogs.
            //
            // Served from the handshake's own copy; asking twice needlessly
            // transfers the same 46 KB payload again.
            Request::Catalog { timeout_seconds } => {
                let cached = self.state.model_repo().map(|payload| payload.value);
                match cached {
                    Some(payload) => match serde_json::to_value(payload) {
                        Ok(v) => Response::Ok {
                            data: serde_json::json!({ "payload": v }),
                        },
                        Err(e) => Response::error(format!("catalog payload: {e}")),
                    },
                    None => self.respond(|c| {
                        c.fetch_model_repo(Duration::from_secs(timeout_seconds))
                            .map(|payload| serde_json::json!({ "payload": payload }))
                    }),
                }
            }
            // Rows arrive as plain wire indices and are rebuilt here, so
            // the newtype's guard against the wire/screen mix-up stays real.
            Request::SetParam {
                row,
                column,
                target,
                input,
                scene,
                promote,
                timeout_seconds,
            } => {
                let client = self.client();
                let result = (|| {
                    client.set_parameter(
                        cortex_rs::Row::try_from_wire(row)?,
                        column,
                        target,
                        input,
                        scene,
                        promote,
                        Duration::from_secs(timeout_seconds),
                    )
                })();
                match result {
                    Ok(applied) => {
                        self.repair_live_preset(Duration::from_secs(timeout_seconds));
                        Response::ok(&applied).unwrap_or_else(|e| {
                            Response::error(format!("serialising parameter write: {e}"))
                        })
                    }
                    Err(error) => Response::error(error.to_string()),
                }
            }
            Request::SetBlock {
                row,
                column,
                model,
                verify,
                timeout_seconds,
            } => {
                let client = self.client();
                let result = (|| {
                    let row = cortex_rs::Row::try_from_wire(row)?;
                    if verify {
                        client.set_block(row, column, model, Duration::from_secs(timeout_seconds))
                    } else {
                        client.set_block_unverified(row, column, model)?;
                        Ok(cortex_rs::Placement::Unverified)
                    }
                })();
                match result {
                    Ok(placement) => {
                        self.repair_live_preset(Duration::from_secs(timeout_seconds.max(1)));
                        Response::ok(&placement).unwrap_or_else(|e| {
                            Response::error(format!("serialising block placement: {e}"))
                        })
                    }
                    Err(error) => Response::error(error.to_string()),
                }
            }
            Request::SetBypass {
                row,
                column,
                bypass,
            } => self.respond(move |c| {
                c.set_bypass(cortex_rs::Row::try_from_wire(row)?, column, bypass)
                    .map(|()| serde_json::json!({ "applied": true }))
            }),
            Request::RemoveBlock { row, column } => self.respond(move |c| {
                c.remove_block(cortex_rs::Row::try_from_wire(row)?, column)
                    .map(|()| serde_json::json!({ "applied": true }))
            }),
            Request::SetSplit { row, split, mix } => self.respond(move |c| {
                c.set_split(cortex_rs::Row::try_from_wire(row)?, split, mix)
                    .map(|()| serde_json::json!({ "applied": true }))
            }),
            Request::SetRouting { row, input, output } => self.respond(move |c| {
                let row = cortex_rs::Row::try_from_wire(row)?;
                match (input, output) {
                    (Some(port), None) => c.set_chain_input(row, port),
                    (None, Some(port)) => c.set_chain_output(row, port),
                    _ => Err(cortex_rs::Error::NotFound(
                        "set-routing needs exactly one of input or output".into(),
                    )),
                }
                .map(|()| serde_json::json!({ "applied": true }))
            }),
            Request::PrepareSave {
                setlist,
                slot,
                policy,
                override_scratch,
                recall_consent,
                timeout_seconds,
            } => {
                let validated_policy = match policy.to_policy() {
                    Ok(p) => p,
                    Err(e) => return Response::error(e.to_string()),
                };
                let client = self.client();
                let result = client.prepare_save_before_editing(
                    &validated_policy,
                    &setlist,
                    &slot,
                    override_scratch,
                    recall_consent,
                    Duration::from_secs(timeout_seconds),
                );
                match result {
                    Ok(preparation) => {
                        let view = preparation.view();
                        let token =
                            format!("save-{}", self.next_token.fetch_add(1, Ordering::Relaxed));
                        self.preparations
                            .lock()
                            .unwrap()
                            .insert(token.clone(), (validated_policy, preparation));
                        let result = cortex_rs::daemon::PrepareSaveResult { token, view };
                        Response::ok(&result).unwrap_or_else(|e| {
                            Response::error(format!("serialising preparation: {e}"))
                        })
                    }
                    Err(error) => Response::error(error.to_string()),
                }
            }
            Request::CommitSave {
                token,
                confirmed,
                name,
                timeout_seconds,
            } => {
                let entry = self
                    .preparations
                    .lock()
                    .unwrap()
                    .remove(&token)
                    .ok_or_else(|| {
                        cortex_rs::Error::UnsafeSave(format!(
                            "unknown or already-used preparation token: {token}"
                        ))
                    });
                let (policy, preparation) = match entry {
                    Ok(pair) => pair,
                    Err(e) => return Response::error(e.to_string()),
                };
                let confirmation = match cortex_rs::safety::SaveConfirmation::explicit(confirmed) {
                    Ok(c) => c,
                    Err(e) => return Response::error(e.to_string()),
                };
                let client = self.client();
                let result = client.save_prepared(
                    &policy,
                    preparation,
                    confirmation,
                    name.as_deref(),
                    Duration::from_secs(timeout_seconds),
                );
                match result {
                    Ok(receipt) => Response::ok(&receipt.view()).unwrap_or_else(|e| {
                        Response::error(format!("serialising save receipt: {e}"))
                    }),
                    Err(error) => Response::error(error.to_string()),
                }
            }
            Request::DeletePreset { setlist, name } => self.respond(move |c| {
                c.delete_preset(&setlist, &name, REQUEST_TIMEOUT)
                    .map(|()| serde_json::json!({ "deleted": name }))
            }),
            Request::CpuLoad => match self.state.cpu_load() {
                Some(load) if self.cache_is_usable() => {
                    Response::ok(&cortex_rs::view::CpuLoad::from(&load.value))
                        .unwrap_or_else(|e| Response::error(format!("CpuLoad: {e}")))
                }
                None => Response::error(
                    "no CPU load received yet - the device pushes it about once a second \
                     after subscribing"
                        .to_string(),
                ),
                Some(_) => Response::error("the device is not currently responsive"),
            },
            // Handled by the caller, which needs to stop the accept loop.
            Request::Shutdown => Response::ok(&serde_json::json!({ "stopping": true }))
                .unwrap_or_else(|e| Response::error(format!("{e}"))),
        }
    }

    /// Run a client call, turning any error into a `Response` rather than
    /// letting it escape.
    fn respond<T, F>(&self, call: F) -> Response
    where
        T: serde::Serialize,
        F: FnOnce(&QuadCortex) -> cortex_rs::Result<T>,
    {
        match call(&self.client()) {
            Ok(value) => Response::ok(&value)
                .unwrap_or_else(|e| Response::error(format!("serialising response: {e}"))),
            Err(e) => Response::error(e.to_string()),
        }
    }

    fn status(&self) -> Status {
        // From the handshake's Version READ. Absent only if the device did
        // not answer it, which is not fatal - the session still works.
        let session = self.session();
        let identity = session
            .device_version()
            .map(|v| cortex_rs::view::DeviceVersion::from(&v));
        let state = self.state.status();
        let runtime_health = self.health.lock().unwrap().clone();
        let device = match runtime_health {
            RuntimeHealth::Connected if session.is_responsive() => DeviceHealth::Connected {
                serial: identity.as_ref().and_then(|d| d.serial_number.clone()),
                coros_version: identity.as_ref().and_then(|d| d.coros_version.clone()),
                last_message_seconds: session.seconds_since_last_message(),
            },
            RuntimeHealth::Connected => DeviceHealth::Reconnecting {
                attempts: 0,
                last_error: format!(
                    "device silent for {}s; reconnect pending",
                    session.seconds_since_last_message()
                ),
            },
            RuntimeHealth::Reconnecting {
                attempts,
                last_error,
            } => DeviceHealth::Reconnecting {
                attempts,
                last_error,
            },
            RuntimeHealth::Failed { error } => DeviceHealth::Failed { error },
        };
        Status {
            daemon_version: cortex_rs::daemon::DAEMON_PROTOCOL_VERSION.to_string(),
            uptime_seconds: self.started.elapsed().as_secs(),
            device,
            cache: CacheStatus {
                generation: state.generation,
                revision: state.revision,
                storage_revision: state.storage_revision,
                phase: state.phase,
                catalog: state.catalog,
                current_preset: state.current_preset,
                active_scene: state.active_scene,
                preset_dirty: state.preset_dirty,
                preset_location: state.preset_location,
                listed_setlists: state.listed_setlists,
                pushes_applied: state.counters.applied,
                messages_seen: state.counters.seen,
                messages_rejected: state.counters.rejected,
                stream_gaps: state.counters.stream_gaps,
                last_rejection: state.last_rejection,
            },
        }
    }

    /// Watch liveness independently of requests and replace a silent session.
    fn start_reconnect_monitor(&self) {
        let session = self.session.clone();
        let state = self.state.clone();
        let health = self.health.clone();
        let stopping = self.stopping.clone();
        let result = std::thread::Builder::new()
            .name("cortex-reconnect".into())
            .spawn(move || {
                reconnect_loop(
                    session,
                    state,
                    health.clone(),
                    stopping,
                    RECONNECT_TIMINGS,
                    open_replacement,
                );
            });
        if let Err(error) = result {
            *self.health.lock().unwrap() = RuntimeHealth::Failed {
                error: format!("could not start reconnect monitor: {error}"),
            };
        }
    }

    fn shutdown(&self) {
        self.stopping.store(true, Ordering::Relaxed);
        let session = self.session();
        session.disconnect();
        session.stop();
    }
}

/// Explicitly read any state the subscription seed did not provide.
fn seed_session(session: &Arc<Session>, state: &cortex_rs::DeviceStateCache) {
    let client = QuadCortex::new(session.clone());
    if state.current_preset().is_none() {
        if let Err(error) = client.read_current_preset(Duration::from_secs(15)) {
            eprintln!("cortex session: warning: live-grid cache not seeded ({error})");
        }
    }
    if state.active_scene().is_none() {
        if let Err(error) = client.active_scene(Duration::from_secs(10)) {
            eprintln!("cortex session: warning: scene cache not seeded ({error})");
        }
    }
}

/// Open and fully handshake one replacement session over a retained cache.
fn open_replacement(state: &cortex_rs::DeviceStateCache) -> Result<Arc<Session>> {
    let session = Arc::new(Session::open_with_state(
        DeviceKind::QuadCortex,
        state.clone(),
    )?);
    let connected = session.connect_with_progress(
        cortex_rs::ConnectMode::Subscribed,
        Duration::from_secs(15),
        Duration::from_secs(2),
        |step| eprintln!("cortex session: reconnect: {step} ..."),
    );
    if let Err(error) = connected {
        session.stop();
        return Err(error.into());
    }
    seed_session(&session, state);
    Ok(session)
}

/// Sleep in slices so shutdown does not wait out a 30-second backoff.
fn backoff(stopping: &AtomicBool, duration: Duration) -> bool {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        if stopping.load(Ordering::Relaxed) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    true
}

fn reconnect_loop<F>(
    session_slot: Arc<Mutex<Arc<Session>>>,
    state: cortex_rs::DeviceStateCache,
    health: Arc<Mutex<RuntimeHealth>>,
    stopping: Arc<AtomicBool>,
    timings: ReconnectTimings,
    connect: F,
) where
    F: Fn(&cortex_rs::DeviceStateCache) -> Result<Arc<Session>>,
{
    while backoff(&stopping, timings.poll) {
        let current = session_slot.lock().unwrap().clone();
        if current.is_responsive() {
            continue;
        }

        let silent_for = current.seconds_since_last_message();
        let mut attempts = 0_u32;
        let mut delay = timings.initial_backoff;
        let mut last_error = format!("device silent for {silent_for}s");
        state.invalidate(last_error.clone());
        *health.lock().unwrap() = RuntimeHealth::Reconnecting {
            attempts,
            last_error: last_error.clone(),
        };
        eprintln!("cortex session: {last_error}; reconnecting");

        // The old handle must be gone before opening another. Concurrent HID
        // ownership is accepted initially and wedges the next request.
        current.disconnect();
        current.stop();

        loop {
            if stopping.load(Ordering::Relaxed) {
                return;
            }
            attempts = attempts.saturating_add(1);
            *health.lock().unwrap() = RuntimeHealth::Reconnecting {
                attempts,
                last_error: last_error.clone(),
            };
            eprintln!("cortex session: reconnect attempt {attempts}");

            match connect(&state) {
                Ok(replacement) => {
                    if stopping.load(Ordering::Relaxed) {
                        replacement.disconnect();
                        replacement.stop();
                        return;
                    }
                    *session_slot.lock().unwrap() = replacement;
                    *health.lock().unwrap() = RuntimeHealth::Connected;
                    eprintln!("cortex session: reconnected on attempt {attempts}");
                    break;
                }
                Err(error) => {
                    last_error = error.to_string();
                    *health.lock().unwrap() = RuntimeHealth::Reconnecting {
                        attempts,
                        last_error: last_error.clone(),
                    };
                    eprintln!(
                        "cortex session: reconnect attempt {attempts} failed ({last_error}); \
                         retrying in {}s",
                        delay.as_secs()
                    );
                    if !backoff(&stopping, delay) {
                        return;
                    }
                    delay = (delay * 2).min(timings.max_backoff);
                }
            }
        }
    }
}

/// Run the daemon until told to stop.
pub fn run() -> Result<()> {
    let path = socket_path();

    // A socket left by a dead daemon would make bind() fail. Probe it first:
    // if something answers, another daemon is live and this one must not
    // start, because two processes cannot both own the HID interface.
    if path.exists() {
        if UnixStream::connect(&path).is_ok() {
            anyhow::bail!(
                "a cortex daemon is already running on {}. Stop it with \
                 `cortex session stop`, or use the existing one.",
                path.display()
            );
        }
        eprintln!(
            "cortex session: removing a stale socket at {} (nothing was listening)",
            path.display()
        );
        std::fs::remove_file(&path)?;
    }

    // Claim the socket BEFORE the handshake, not after.
    //
    // Hardware-verified, and the ordering is the whole point: the handshake
    // took 33 s on an already-unsettled device, and for that entire window
    // the daemon owned the HID interface while nothing was listening. So
    // `is_running()` answered false and a one-shot command opened the device
    // for itself - the exact collision the guard exists to prevent, during
    // the window when it is most likely, because startup is when someone is
    // most likely to be typing the next command.
    //
    // Binding first makes the claim visible for the daemon's whole life.
    // Clients that arrive mid-handshake wait in the listen backlog and are
    // served once the session is up, which is the right answer for them too.
    let listener = UnixListener::bind(&path)?;
    // The socket carries full control of the device, so restrict it to the
    // owner rather than relying on the directory's permissions.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }

    // The socket now exists, so a failed handshake must clear it on the way
    // out; otherwise the next start finds a file it has to treat as stale.
    let daemon = match Daemon::connect() {
        Ok(daemon) => daemon,
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            return Err(e);
        }
    };

    eprintln!("cortex session: listening on {}", path.display());
    eprintln!("cortex session: stop with `cortex session stop`, or Ctrl-C if attached");
    daemon.start_reconnect_monitor();

    for stream in listener.incoming() {
        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                eprintln!("cortex session: accept failed: {e}");
                continue;
            }
        };
        if serve(&daemon, stream) == Control::Stop {
            break;
        }
    }

    eprintln!("cortex session: shutting down");

    // Teardown talks to the device, and a wedged device is exactly the state
    // someone reaches for `--stop` in. Observed: a daemon whose session had
    // gone silent acknowledged the stop, then outlived it by minutes and
    // needed SIGTERM. The client had already been told "stopped", so it
    // looked done while still holding the HID interface - which blocks the
    // next session from starting. Bound it: the disconnect below is
    // fire-and-forget, so there is nothing left worth waiting for.
    std::thread::spawn(|| {
        std::thread::sleep(SHUTDOWN_GRACE);
        eprintln!(
            "cortex session: teardown did not finish within {}s; exiting anyway",
            SHUTDOWN_GRACE.as_secs()
        );
        std::process::exit(0);
    });

    daemon.shutdown();
    // Last, so the socket never outlives our claim on the device: while it
    // answers, other commands correctly refuse to open the device for
    // themselves. A file left behind by a forced exit is inert - nothing is
    // listening on it - and the next start clears it as stale.
    let _ = std::fs::remove_file(&path);
    Ok(())
}

/// Whether the accept loop should continue.
#[derive(Debug, PartialEq, Eq)]
enum Control {
    Continue,
    Stop,
}

/// Serve one connection. A client may send several requests on one stream.
fn serve(daemon: &Daemon, stream: UnixStream) -> Control {
    let peer = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cortex session: could not clone stream: {e}");
            return Control::Continue;
        }
    };
    let reader = BufReader::new(peer);
    let mut writer = stream;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("cortex session: read failed: {e}");
                return Control::Continue;
            }
        };
        if line.trim().is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<Request>(&line) {
            Ok(request) => {
                let stopping = matches!(request, Request::Shutdown);
                let response = daemon.handle(request);
                if stopping {
                    let _ = write_response(&mut writer, &response);
                    return Control::Stop;
                }
                response
            }
            Err(e) => Response::error(format!("could not parse request: {e}")),
        };

        if write_response(&mut writer, &response).is_err() {
            // The client hung up mid-response. Not our problem, and not a
            // reason to stop serving everyone else.
            return Control::Continue;
        }
    }
    Control::Continue
}

fn write_response(writer: &mut UnixStream, response: &Response) -> std::io::Result<()> {
    let mut line = serde_json::to_string(response)
        .unwrap_or_else(|e| format!(r#"{{"status":"error","message":"{e}"}}"#));
    line.push('\n');
    writer.write_all(line.as_bytes())?;
    writer.flush()
}

/// Whether a daemon is listening and answering on the socket.
///
/// This is the guard for direct device access. The device grants its HID
/// interface to one owner: opening it while the daemon holds it does not
/// fail loudly, it wedges the held session - the device simply stops
/// sending, heartbeat included, and every subsequent read times out. A
/// stale socket file does not count, so this connects rather than checking
/// for the path.
#[must_use]
pub fn is_running() -> bool {
    UnixStream::connect(socket_path()).is_ok()
}

// `setsid`, used to detach from the terminal so closing it does not kill the
// session. Declared rather than pulled in with a `libc` dependency, matching
// how the CLI already handles `SIGPIPE`.
#[cfg(unix)]
unsafe extern "C" {
    fn setsid() -> i32;
}

/// Start the session in the background, returning once it is serving.
///
/// Re-executes this binary with `--foreground` rather than forking in place.
/// The session spawns an RX thread and a keepalive thread, and forking a
/// process that is about to become multithreaded is a well-known way to
/// deadlock in the child; re-exec starts from a clean single-threaded state.
///
/// The parent waits for the socket to answer before reporting success. That
/// matters more here than it looks: the handshake can fail (no device, or
/// another owner), and a fire-and-forget spawn would report success and leave
/// the failure to be discovered by the next command.
///
/// # Errors
///
/// Returns an error if a session is already running, if the child cannot be
/// spawned, or if it exits or goes quiet before it starts serving.
pub fn start_detached() -> Result<()> {
    if is_running() {
        anyhow::bail!(
            "a cortex session is already running. \
             Check it with `cortex session status`, or replace it with \
             `cortex session stop` first."
        );
    }

    let log = cortex_rs::daemon::log_path();
    let file = std::fs::File::create(&log)?;
    let exe = std::env::current_exe()?;

    let mut cmd = std::process::Command::new(exe);
    cmd.args(["session", "start", "--foreground"])
        .stdin(std::process::Stdio::null())
        .stdout(file.try_clone()?)
        .stderr(file);

    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            // New session, so the daemon is not in the terminal's process
            // group and does not get SIGHUP when the terminal closes.
            if setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let mut child = cmd.spawn()?;
    eprintln!(
        "cortex session: starting in the background, logging to {}",
        log.display()
    );

    // Poll until it serves. The handshake is seconds, and slower on a unit
    // that is busy, so this is generous - but bounded, because a wait with no
    // end is indistinguishable from a hang.
    let deadline = Instant::now() + Duration::from_secs(120);
    while Instant::now() < deadline {
        // Ask it something, rather than merely checking the socket accepts.
        //
        // The socket is bound BEFORE the handshake on purpose, so that the
        // device is claimed for the whole startup window - which means an
        // accepting socket says nothing about whether the session is serving
        // yet. Only a served reply does. A client arriving early waits in the
        // listen backlog, so this both tests and waits.
        if answers_status(Duration::from_secs(2)) {
            eprintln!("cortex session: ready. Stop it with `cortex session stop`.");
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            let tail = std::fs::read_to_string(&log).unwrap_or_default();
            let tail: Vec<&str> = tail.lines().rev().take(6).collect();
            anyhow::bail!(
                "the session exited before it started serving ({status}):\n  {}",
                tail.into_iter().rev().collect::<Vec<_>>().join("\n  ")
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    anyhow::bail!(
        "the session did not start within 120s; see {}",
        log.display()
    )
}

/// Whether a running session answers a `Status` request within `timeout`.
///
/// Distinct from [`is_running`], which only asks whether the socket accepts.
/// During startup those differ for several seconds, which is exactly the
/// window this has to tell apart.
fn answers_status(timeout: Duration) -> bool {
    let Ok(stream) = UnixStream::connect(socket_path()) else {
        return false;
    };
    if stream.set_read_timeout(Some(timeout)).is_err() {
        return false;
    }
    let Ok(mut writer) = stream.try_clone() else {
        return false;
    };
    let Ok(mut line) = serde_json::to_string(&Request::Status) else {
        return false;
    };
    line.push('\n');
    if writer.write_all(line.as_bytes()).is_err() || writer.flush().is_err() {
        return false;
    }
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply).is_ok() && !reply.trim().is_empty()
}

/// Send one request to a running daemon, if there is one.
///
/// Returns `None` when no daemon is listening, so the caller can fall back to
/// a direct session rather than failing.
///
/// Checks the daemon's protocol version before sending the request. A
/// mismatch (e.g. an old daemon survived an upgrade) produces an error naming
/// the fix rather than a cryptic parse failure.
pub fn request(request: &Request) -> Option<Result<serde_json::Value>> {
    let path = socket_path();
    let stream = UnixStream::connect(&path).ok()?;

    Some((|| {
        let mut writer = stream.try_clone()?;

        // Version gate: check the daemon's protocol version before sending
        // a request it might not understand. An old daemon that survived an
        // upgrade would otherwise misparse a new request shape.
        let version_line = serde_json::to_string(&Request::Status)?;
        let mut vl = version_line.clone();
        vl.push('\n');
        writer.write_all(vl.as_bytes())?;
        writer.flush()?;
        let mut status_reply = String::new();
        BufReader::new(stream.try_clone()?).read_line(&mut status_reply)?;
        let status: Response = serde_json::from_str(&status_reply)?;
        let status_data = match status {
            Response::Ok { data } => data,
            Response::Error { message } => anyhow::bail!("{message}"),
        };
        let daemon_version: u32 = status_data
            .get("daemon_version")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        if daemon_version != cortex_rs::daemon::DAEMON_PROTOCOL_VERSION {
            anyhow::bail!(
                "daemon protocol version mismatch: client expects {}, daemon reports {}. \
                 The daemon is older or newer than this CLI. \
                 Run `cortex session stop` to stop the old daemon, then retry.",
                cortex_rs::daemon::DAEMON_PROTOCOL_VERSION,
                daemon_version
            );
        }

        // Now send the actual request on a fresh connection (the status
        // connection's reader has consumed the reply).
        let path = socket_path();
        let stream2 = UnixStream::connect(&path)?;
        let mut writer2 = stream2.try_clone()?;
        let mut line = serde_json::to_string(request)?;
        line.push('\n');
        writer2.write_all(line.as_bytes())?;
        writer2.flush()?;

        let mut reply = String::new();
        BufReader::new(stream2).read_line(&mut reply)?;
        match serde_json::from_str::<Response>(&reply)? {
            Response::Ok { data } => Ok(data),
            Response::Error { message } => anyhow::bail!("{message}"),
        }
    })())
}

#[cfg(test)]
mod tests {
    //! The daemon over a fake session.
    //!
    //! Everything here is independent of how the session was obtained: the
    //! socket protocol, the shape of a status, and how a call that cannot be
    //! answered is reported. Those were previously verified only by hand
    //! against real hardware, which is why this file sat at 0% coverage
    //! through several rewrites of its shutdown and readiness logic.
    //!
    //! Device-bound requests are deliberately absent: `REQUEST_TIMEOUT` is
    //! 30 s, so a test that waited for one would trade half a minute for
    //! nothing the fake can meaningfully assert.

    use super::*;
    use cortex_rs::link::{FakeLink, HidLink};
    use std::net::Shutdown;
    use std::sync::atomic::AtomicUsize;

    fn fake_daemon() -> Daemon {
        let link = FakeLink::new();
        let device: Arc<std::sync::Mutex<dyn HidLink>> = Arc::new(std::sync::Mutex::new(link));
        Daemon::over(Arc::new(
            Session::over(device).expect("session over a fake link"),
        ))
    }

    fn inbound(message_type: u16, body: &[u8]) -> Vec<u8> {
        let mut message = body.to_vec();
        message.extend_from_slice(&message_type.to_le_bytes());
        message.extend_from_slice(&[0_u8; 6]);
        let mut report = vec![0x01, u8::try_from(message.len()).unwrap(), 0xC0];
        report.extend_from_slice(&message);
        report
    }

    /// A client sends a line; the daemon answers on the same connection.
    fn round_trip(daemon: &Daemon, request: &Request) -> (Control, String) {
        let (mut client, server) = UnixStream::pair().expect("socket pair");
        let mut line = serde_json::to_string(request).expect("encode");
        line.push('\n');
        client.write_all(line.as_bytes()).expect("write");
        client.shutdown(Shutdown::Write).expect("half close");

        let control = serve(daemon, server);
        let mut reply = String::new();
        BufReader::new(client).read_line(&mut reply).expect("read");
        (control, reply)
    }

    #[test]
    fn status_is_answered_without_touching_the_device() {
        let daemon = fake_daemon();
        let Response::Ok { data } = daemon.handle(Request::Status) else {
            panic!("status should always be answerable - it reads local state only");
        };
        assert!(data.get("daemon_version").is_some(), "status: {data}");
        assert!(data.get("device").is_some(), "status: {data}");
        assert!(data.get("cache").is_some(), "status: {data}");
        daemon.shutdown();
    }

    #[test]
    fn a_malformed_request_is_answered_rather_than_dropped() {
        let daemon = fake_daemon();
        let (mut client, server) = UnixStream::pair().expect("socket pair");
        client.write_all(b"this is not json\n").expect("write");
        client.shutdown(Shutdown::Write).expect("half close");

        let control = serve(&daemon, server);
        let mut reply = String::new();
        BufReader::new(client).read_line(&mut reply).expect("read");

        assert_eq!(
            control,
            Control::Continue,
            "one bad client must not stop the daemon serving everyone else"
        );
        assert!(
            reply.contains("error"),
            "a bad request should get an error back, not silence: {reply}"
        );
        daemon.shutdown();
    }

    #[test]
    fn a_shutdown_request_stops_the_accept_loop() {
        let daemon = fake_daemon();
        let (control, reply) = round_trip(&daemon, &Request::Shutdown);
        assert_eq!(control, Control::Stop);
        assert!(
            reply.contains("stopping"),
            "the client should be told it is stopping: {reply}"
        );
        daemon.shutdown();
    }

    #[test]
    fn cpu_load_before_any_push_explains_itself() {
        let daemon = fake_daemon();
        let Response::Error { message } = daemon.handle(Request::CpuLoad) else {
            panic!("a fake session has received no CPU load, so this cannot succeed");
        };
        // The first push lands about 8s after subscribing, so "nothing yet"
        // is a normal state and has to read like one.
        assert!(
            message.contains("subscrib"),
            "the error should say why there is nothing yet: {message}"
        );
        daemon.shutdown();
    }

    #[test]
    fn requests_fail_fast_while_reconnecting() {
        let daemon = fake_daemon();
        *daemon.health.lock().unwrap() = RuntimeHealth::Reconnecting {
            attempts: 3,
            last_error: "device unplugged".into(),
        };
        let Response::Error { message } = daemon.handle(Request::ActiveScene) else {
            panic!("a request must not wait on a session being replaced");
        };
        assert!(message.contains("attempt 3"), "unexpected error: {message}");
        let Response::Ok { data } = daemon.handle(Request::Status) else {
            panic!("status must remain available while reconnecting");
        };
        assert_eq!(data["device"]["state"], "reconnecting");
        daemon.shutdown();
    }

    #[test]
    fn reconnect_retries_then_swaps_in_a_new_generation() {
        let state = cortex_rs::DeviceStateCache::new();
        let initial_link = FakeLink::new();
        let initial_device: Arc<Mutex<dyn HidLink>> = Arc::new(Mutex::new(initial_link));
        let initial = Arc::new(
            Session::over_with_state(initial_device, state.clone()).expect("initial fake session"),
        );
        let slot = Arc::new(Mutex::new(initial));
        let health = Arc::new(Mutex::new(RuntimeHealth::Connected));
        let stopping = Arc::new(AtomicBool::new(false));
        let attempts = Arc::new(AtomicUsize::new(0));
        let connector_attempts = attempts.clone();
        let worker_slot = slot.clone();
        let worker_state = state.clone();
        let worker_health = health.clone();
        let worker_stopping = stopping.clone();

        let worker = std::thread::spawn(move || {
            reconnect_loop(
                worker_slot,
                worker_state,
                worker_health,
                worker_stopping,
                ReconnectTimings {
                    poll: Duration::from_millis(1),
                    initial_backoff: Duration::from_millis(1),
                    max_backoff: Duration::from_millis(2),
                },
                move |state| {
                    let attempt = connector_attempts.fetch_add(1, Ordering::Relaxed) + 1;
                    if attempt == 1 {
                        anyhow::bail!("fictional first-open failure");
                    }
                    let link = FakeLink::new();
                    let device: Arc<Mutex<dyn HidLink>> = Arc::new(Mutex::new(link.clone()));
                    let session = Arc::new(Session::over_with_state(device, state.clone())?);
                    link.push_inbound(inbound(
                        cortex_rs::proto::cortex_message_type::Enum::GlobalTempo as u16,
                        &[],
                    ));
                    let deadline = Instant::now() + Duration::from_secs(1);
                    while Instant::now() < deadline && !session.has_heard_from_device() {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Ok(session)
                },
            );
        });

        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline
            && (attempts.load(Ordering::Relaxed) < 2 || !slot.lock().unwrap().is_responsive())
        {
            std::thread::sleep(Duration::from_millis(2));
        }
        stopping.store(true, Ordering::Relaxed);
        worker.join().unwrap();

        assert_eq!(attempts.load(Ordering::Relaxed), 2);
        assert!(matches!(*health.lock().unwrap(), RuntimeHealth::Connected));
        assert_eq!(state.status().generation, 2);
        slot.lock().unwrap().stop();
    }

    #[test]
    fn untrusted_socket_rows_are_rejected_before_device_io() {
        let daemon = fake_daemon();
        let Response::Error { message } = daemon.handle(Request::SetBlock {
            row: 4,
            column: 0,
            model: 42,
            verify: false,
            timeout_seconds: 0,
        }) else {
            panic!("an out-of-range socket row must be rejected");
        };
        assert!(message.contains("0-3"), "unexpected error: {message}");
        daemon.shutdown();
    }

    #[test]
    fn invalid_parameter_values_are_rejected_before_device_io() {
        let daemon = fake_daemon();
        let Response::Error { message } = daemon.handle(Request::SetParam {
            row: 0,
            column: 0,
            target: cortex_rs::ParameterTarget::Index(0),
            input: cortex_rs::ParameterInput::Normalised(1.5),
            scene: None,
            promote: false,
            timeout_seconds: 0,
        }) else {
            panic!("an out-of-range parameter value must be rejected");
        };
        assert!(message.contains("0.0-1.0"), "unexpected error: {message}");
        daemon.shutdown();
    }

    #[test]
    fn one_connection_serves_several_requests() {
        let daemon = fake_daemon();
        let (mut client, server) = UnixStream::pair().expect("socket pair");
        let mut line = serde_json::to_string(&Request::Status).expect("encode");
        line.push('\n');
        client.write_all(line.as_bytes()).expect("first");
        client.write_all(line.as_bytes()).expect("second");
        client.shutdown(Shutdown::Write).expect("half close");

        serve(&daemon, server);

        let reader = BufReader::new(client);
        let replies = reader.lines().count();
        assert_eq!(
            replies, 2,
            "a client may send several requests on one stream"
        );
        daemon.shutdown();
    }

    #[test]
    fn a_blank_line_is_ignored_rather_than_answered() {
        let daemon = fake_daemon();
        let (mut client, server) = UnixStream::pair().expect("socket pair");
        client.write_all(b"\n\n").expect("write");
        client.shutdown(Shutdown::Write).expect("half close");

        serve(&daemon, server);
        let replies = BufReader::new(client).lines().count();
        assert_eq!(replies, 0, "blank lines are keepalive noise, not requests");
        daemon.shutdown();
    }
}
