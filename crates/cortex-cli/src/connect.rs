// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! `cortex session start`: one held session, served over local IPC.
//!
//! The protocol requires one effective HID owner, so exactly one process
//! can own it. This is that process. Everything else talks to it.
//!
//! @see spec/200-cli/spec.md [FR-18] [FR-19] [FR-20] [FR-21]
//! @see spec/200-cli/spec.md [FR-22] [FR-26] [FR-27] [FR-28]
//! @see spec/200-cli/design.md [DES-CLI]
//! @see spec/140-session/spec.md

use std::collections::HashMap;
#[cfg(test)]
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Duration, Instant};

use anyhow::Result;
#[cfg(test)]
use cortex_host::LocalConnection;
use cortex_host::{
    CacheStatus, DAEMON_PROTOCOL_VERSION, DaemonClient, DaemonErrorCode, DaemonLifecycle,
    DeviceHealth, LocalEndpoint, LocalListener, PrepareSaveResult, Request, Response, Status,
    serve_listener,
};
use cortex_rs::{DeviceKind, QuadCortex, Session, Transport};

/// How long a request may take before the daemon gives up on the device.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const COPY_DEVICE_STEP_TIMEOUT: Duration = Duration::from_secs(40);
const SETLIST_DEVICE_TIMEOUT: Duration = Duration::from_secs(60);
pub const COPY_IPC_TIMEOUT: Duration = Duration::from_secs(10 * 60);
pub const SETLIST_IPC_TIMEOUT: Duration = Duration::from_secs(3 * 60);
pub const SAVE_IPC_TIMEOUT: Duration = Duration::from_secs(3 * 60);
pub const DUPLICATE_IPC_TIMEOUT: Duration = Duration::from_secs(48 * 60 * 60);

fn string_parameter_at(
    preset: &cortex_rs::proto::BinaryPreset,
    row: u32,
    column: u32,
    index: u32,
) -> Option<&str> {
    let model = preset
        .chains
        .iter()
        .enumerate()
        .find(|(position, chain)| {
            let stored = chain.row.as_ref().map(|row| {
                let cortex_rs::proto::chain::Row::Row(row) = row;
                *row
            });
            stored.or_else(|| u32::try_from(*position).ok()) == Some(row)
        })?
        .1
        .models
        .iter()
        .enumerate()
        .find(|(position, model)| {
            let stored = model.column.as_ref().map(|column| {
                let cortex_rs::proto::model::Column::Column(column) = column;
                *column
            });
            stored.or_else(|| u32::try_from(*position).ok()) == Some(column)
        })?
        .1;
    let parameter = model
        .params
        .iter()
        .enumerate()
        .find(|(position, parameter)| {
            let positional = u32::try_from(*position).ok();
            let stored = parameter.index.as_ref().map(|index| {
                let cortex_rs::proto::param::Index::Index(value) = index;
                *value
            });
            stored.or(positional) == Some(index)
        })?
        .1;
    let value = parameter.param_values.first()?.value.as_ref()?;
    let cortex_rs::proto::param_value::Value::StringValue(value) = value else {
        return None;
    };
    Some(value)
}

/// How long teardown gets before the process exits regardless.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(3);

/// How often the held session's measured liveness is checked.
const HEALTH_POLL: Duration = Duration::from_secs(1);

/// Reconnect starts quickly, then backs off to this ceiling while unplugged.
const MAX_RECONNECT_BACKOFF: Duration = Duration::from_secs(30);

fn policy_for_slots(
    setlist: &str,
    slots: &[&str],
) -> cortex_rs::Result<cortex_rs::safety::SavePolicy> {
    let ranges = slots
        .iter()
        .map(|slot| cortex_rs::ScratchRange::new(slot, slot))
        .collect::<cortex_rs::Result<Vec<_>>>()?;
    cortex_rs::safety::SavePolicy::new(setlist, ranges)
}

/// Bound a caller waiting on a wedged local IPC connection.
#[cfg(test)]
const SOCKET_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

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

#[derive(Default)]
struct ReconnectControl {
    stopping: AtomicBool,
    retry_now: AtomicBool,
}

impl ReconnectControl {
    fn wait(&self, duration: Duration) -> bool {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            if self.stopping.load(Ordering::Relaxed) {
                return false;
            }
            if self.retry_now.swap(false, Ordering::AcqRel) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        true
    }
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
    /// Serializes device operations and excludes reconnect/recovery.
    operations: Arc<Mutex<()>>,
    accepting_device_requests: AtomicBool,
    reconnect: Arc<ReconnectControl>,
    started: Instant,
    lifecycle: DaemonLifecycle,
    force_exit_on_shutdown: bool,
    shutdown_ready: Arc<AtomicBool>,
}

impl Daemon {
    /// Open the device and perform the SUBSCRIBED handshake.
    ///
    /// Subscribed, not minimal, and that is the whole point: subscribing is
    /// how the device reports edits made by the PLAYER on the hardware. It
    /// is expensive per command and correct once per session.
    fn connect(lifecycle: DaemonLifecycle) -> Result<Self> {
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

        let daemon = Self::over_with_lifecycle(session, lifecycle, true);
        daemon.seed_live_state();
        Ok(daemon)
    }

    /// Build a daemon around an already-connected session.
    ///
    /// Separated from [`Self::connect`] so the request handling can be
    /// exercised without a device: everything interesting here - the socket
    /// protocol, the status shape, how a failed call is reported - is
    /// independent of how the session was obtained.
    #[cfg(test)]
    fn over(session: Arc<Session>) -> Self {
        Self::over_with_lifecycle(session, DaemonLifecycle::Explicit, false)
    }

    fn over_with_lifecycle(
        session: Arc<Session>,
        lifecycle: DaemonLifecycle,
        force_exit_on_shutdown: bool,
    ) -> Self {
        let state = session.state_cache();
        Self {
            session: Arc::new(Mutex::new(session)),
            state,
            catalog: Mutex::new(None),
            preparations: Mutex::new(HashMap::new()),
            next_token: AtomicU64::new(1),
            health: Arc::new(Mutex::new(RuntimeHealth::Connected)),
            operations: Arc::new(Mutex::new(())),
            accepting_device_requests: AtomicBool::new(true),
            reconnect: Arc::new(ReconnectControl::default()),
            started: Instant::now(),
            lifecycle,
            force_exit_on_shutdown,
            shutdown_ready: Arc::new(AtomicBool::new(false)),
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
        let mut parsed = self.catalog.lock().unwrap();
        let Some(payload) = self.state.model_repo() else {
            // NewModels, a stream gap, or a new generation invalidates the raw
            // payload. Evict the separately parsed form at the same boundary
            // so name and parameter resolution cannot retain an older repo.
            *parsed = None;
            return None;
        };
        if let Some((generation, revision, catalog)) = parsed.as_ref() {
            if *generation == payload.generation && *revision == payload.revision {
                return Some(catalog.clone());
            }
        }
        *parsed = None;
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
    fn unavailable(&self) -> Option<(DaemonErrorCode, String)> {
        match self.health.lock().unwrap().clone() {
            RuntimeHealth::Connected => None,
            RuntimeHealth::Reconnecting {
                attempts,
                last_error,
            } => Some((
                DaemonErrorCode::Reconnecting,
                format!("device reconnecting (attempt {attempts}): {last_error}"),
            )),
            RuntimeHealth::Failed { error } => Some((
                DaemonErrorCode::DeviceUnavailable,
                format!("device connection failed: {error}"),
            )),
        }
    }

    /// Handle one request.
    ///
    /// Every arm returns a `Response` rather than propagating, because a
    /// failed request must not take the daemon down with it - the session is
    /// shared by every other client.
    fn handle(&self, request: Request) -> Response {
        if matches!(request, Request::Shutdown) {
            return self.begin_shutdown();
        }
        let needs_device = !matches!(&request, Request::Status | Request::ReconnectNow);
        if needs_device {
            if !self.accepting_device_requests.load(Ordering::Acquire) {
                return Response::coded_error(
                    DaemonErrorCode::ShuttingDown,
                    "cortex session is shutting down",
                );
            }
            if let Some((code, message)) = self.unavailable() {
                return Response::coded_error(code, message);
            }
        }
        let _operation = if needs_device {
            match self.operations.try_lock() {
                Ok(operation) => Some(operation),
                Err(TryLockError::WouldBlock) => {
                    return Response::coded_error(
                        DaemonErrorCode::Busy,
                        "another device operation is already in progress; retry after it completes",
                    );
                }
                Err(TryLockError::Poisoned(_)) => {
                    return Response::error("device operation gate is unavailable");
                }
            }
        } else {
            None
        };
        if needs_device {
            if !self.accepting_device_requests.load(Ordering::Acquire) {
                return Response::coded_error(
                    DaemonErrorCode::ShuttingDown,
                    "cortex session is shutting down",
                );
            }
            if let Some((code, message)) = self.unavailable() {
                return Response::coded_error(code, message);
            }
        }
        match request {
            Request::Status => match Response::ok(&self.status()) {
                Ok(r) => r,
                Err(e) => Response::error(format!("building status: {e}")),
            },
            Request::ReconnectNow => self.reconnect_now(),
            Request::NanoState => Response::coded_error(
                DaemonErrorCode::Protocol,
                "the held session owns a Quad Cortex; start it with `--device nano`",
            ),
            Request::NanoSetAmp { .. } => Response::coded_error(
                DaemonErrorCode::Protocol,
                "the held session owns a Quad Cortex; start it with `--device nano`",
            ),
            Request::NanoSetBypass { .. } => Response::coded_error(
                DaemonErrorCode::Protocol,
                "the held session owns a Quad Cortex; start it with `--device nano`",
            ),
            Request::NanoReadFxParams { .. } => Response::coded_error(
                DaemonErrorCode::Protocol,
                "the held session owns a Quad Cortex; start it with `--device nano`",
            ),
            Request::NanoSetFxParam { .. } => Response::coded_error(
                DaemonErrorCode::Protocol,
                "the held session owns a Quad Cortex; start it with `--device nano`",
            ),
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
            Request::SetSceneLabel { scene, label } => self.respond(|c| {
                c.set_scene_label(scene, label.as_deref())
                    .map(|()| serde_json::json!({ "scene": scene, "label": label }))
            }),
            Request::SetSceneColor { scene, color } => self.respond(|c| {
                c.set_scene_color(scene, color)
                    .map(|()| serde_json::json!({ "scene": scene, "color": color }))
            }),
            Request::CopyScene {
                from_scene,
                to_scene,
                swap,
            } => self.respond(|c| {
                c.copy_scene(from_scene, to_scene, swap)?;
                c.read_current_preset(REQUEST_TIMEOUT)?;
                Ok(serde_json::json!({
                    "from_scene": from_scene,
                    "to_scene": to_scene,
                    "swap": swap,
                    "verified": true,
                }))
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
                c.recall_preset(&setlist, &slot, factory, REQUEST_TIMEOUT)
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
            Request::ListCaptures { timeout_seconds } => {
                self.respond(move |c| c.captures(Duration::from_secs(timeout_seconds)))
            }
            Request::ListIrs {
                folder,
                timeout_seconds,
            } => self.respond(move |c| {
                c.list_irs(folder.as_deref(), Duration::from_secs(timeout_seconds))
            }),
            Request::SetCapture {
                row,
                column,
                capture,
                model,
                timeout_seconds,
            } => {
                let client = self.client();
                let timeout = Duration::from_secs(timeout_seconds);
                let result = (|| {
                    let returned = client.captures(timeout)?;
                    if !returned.contains(&capture) {
                        return Err(cortex_rs::Error::NotFound(
                            "capture must be selected from the current device listing".into(),
                        ));
                    }
                    client.set_capture(
                        cortex_rs::Row::try_from_wire(row)?,
                        column,
                        &capture,
                        model,
                        &[],
                        timeout,
                    )?;
                    let preset = client.read_current_preset(timeout)?;
                    let expected = format!("{}{}", capture.key, capture.name);
                    if string_parameter_at(&preset, row, column, 5) != Some(expected.as_str()) {
                        return Err(cortex_rs::Error::GridWriteUnconfirmed(
                            "capture selector did not read back from the live grid".into(),
                        ));
                    }
                    Ok(preset)
                })();
                match result {
                    Ok(preset) => {
                        self.repair_live_preset(timeout);
                        Response::ok(&cortex_rs::view::Preset::from_binary(
                            &preset,
                            self.catalog().as_ref(),
                            "(live grid)",
                            "",
                            true,
                        ))
                        .unwrap_or_else(|error| {
                            Response::error(format!("serialising capture selection: {error}"))
                        })
                    }
                    Err(error) => Response::cortex_error(&error),
                }
            }
            Request::SetIr {
                row,
                column,
                ir,
                slot,
                model,
                folder,
                timeout_seconds,
            } => {
                let client = self.client();
                let timeout = Duration::from_secs(timeout_seconds);
                let result = (|| {
                    let returned = client.list_irs(folder.as_deref(), timeout)?;
                    if !returned.contains(&ir) {
                        return Err(cortex_rs::Error::NotFound(
                            "IR must be selected from the current device listing".into(),
                        ));
                    }
                    client.set_ir(
                        cortex_rs::Row::try_from_wire(row)?,
                        column,
                        &ir,
                        slot,
                        model,
                        timeout,
                    )?;
                    let preset = client.read_current_preset(timeout)?;
                    let (key_index, name_index) = if slot == 0 { (2, 22) } else { (10, 23) };
                    if string_parameter_at(&preset, row, column, key_index) != Some(ir.key.as_str())
                        || string_parameter_at(&preset, row, column, name_index)
                            != Some(ir.name.as_str())
                    {
                        return Err(cortex_rs::Error::GridWriteUnconfirmed(
                            "IR key and name did not read back from the live grid".into(),
                        ));
                    }
                    Ok(preset)
                })();
                match result {
                    Ok(preset) => {
                        self.repair_live_preset(timeout);
                        Response::ok(&cortex_rs::view::Preset::from_binary(
                            &preset,
                            self.catalog().as_ref(),
                            "(live grid)",
                            "",
                            true,
                        ))
                        .unwrap_or_else(|error| {
                            Response::error(format!("serialising IR selection: {error}"))
                        })
                    }
                    Err(error) => Response::cortex_error(&error),
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
                    Err(error) => Response::cortex_error(&error),
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
                    Err(error) => Response::cortex_error(&error),
                }
            }
            Request::SetBypass {
                row,
                column,
                bypass,
            } => self.respond(move |c| {
                c.set_bypass(cortex_rs::Row::try_from_wire(row)?, column, bypass)
                    .map(|()| serde_json::json!({ "applied": true, "verified": true }))
            }),
            Request::RemoveBlock { row, column } => self.respond(move |c| {
                c.remove_block(cortex_rs::Row::try_from_wire(row)?, column)
                    .map(|()| serde_json::json!({ "applied": true, "verified": true }))
            }),
            Request::MoveBlock {
                from_row,
                from_column,
                to_row,
                to_column,
                timeout_seconds,
            } => self.respond(move |c| {
                c.move_block(
                    cortex_rs::Row::try_from_wire(from_row)?,
                    from_column,
                    cortex_rs::Row::try_from_wire(to_row)?,
                    to_column,
                    true,
                    Duration::from_secs(timeout_seconds),
                )
                .map(|()| serde_json::json!({ "applied": true, "verified": true }))
            }),
            Request::SetSplit { row, split, mix } => self.respond(move |c| {
                c.set_split(cortex_rs::Row::try_from_wire(row)?, split, mix)
                    .map(|()| serde_json::json!({ "applied": true, "verified": true }))
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
                .map(|()| serde_json::json!({ "applied": true, "verified": true }))
            }),
            Request::PrepareSave {
                setlist,
                slot,
                recall_consent,
                timeout_seconds,
            } => {
                let validated_policy = match policy_for_slots(&setlist, &[&slot]) {
                    Ok(p) => p,
                    Err(e) => return Response::cortex_error(&e),
                };
                let client = self.client();
                let result = client.prepare_save_before_editing(
                    &validated_policy,
                    &setlist,
                    &slot,
                    cortex_rs::ScratchOverride::ScratchOnly,
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
                        let result = PrepareSaveResult { token, view };
                        Response::ok(&result).unwrap_or_else(|e| {
                            Response::error(format!("serialising preparation: {e}"))
                        })
                    }
                    Err(error) => Response::cortex_error(&error),
                }
            }
            Request::CommitSave {
                token,
                confirmed,
                name,
                instrument,
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
                    Err(e) => return Response::cortex_error(&e),
                };
                let confirmation = match cortex_rs::safety::SaveConfirmation::explicit(confirmed) {
                    Ok(c) => c,
                    Err(e) => return Response::cortex_error(&e),
                };
                let client = self.client();
                let result = client.save_prepared(
                    &policy,
                    preparation,
                    confirmation,
                    name.as_deref(),
                    instrument,
                    Duration::from_secs(timeout_seconds),
                );
                match result {
                    Ok(receipt) => Response::ok(&receipt.result_view()).unwrap_or_else(|e| {
                        Response::error(format!("serialising save receipt: {e}"))
                    }),
                    Err(error) => Response::cortex_error(&error),
                }
            }
            Request::DeletePreset { setlist, name } => self.respond(move |c| {
                c.delete_preset(&setlist, &name, REQUEST_TIMEOUT)
                    .map(|()| serde_json::json!({ "deleted": name }))
            }),
            Request::MovePreset {
                setlist,
                from_slot,
                to_slot,
                confirmed,
            } => {
                if !confirmed {
                    return Response::coded_error(
                        DaemonErrorCode::SafetyRefused,
                        cortex_rs::Error::UnsafeMove("explicit confirmation is required".into())
                            .to_string(),
                    );
                }
                let policy = match policy_for_slots(&setlist, &[&from_slot, &to_slot]) {
                    Ok(policy) => policy,
                    Err(error) => return Response::cortex_error(&error),
                };
                self.respond(move |c| {
                    c.move_preset(
                        &policy,
                        &setlist,
                        &from_slot,
                        &to_slot,
                        Duration::from_secs(20),
                    )
                    .map(|()| serde_json::json!({ "from_slot": from_slot, "to_slot": to_slot }))
                })
            }
            Request::CopyPreset {
                from_setlist,
                from_slot,
                to_setlist,
                to_slot,
                name,
                instrument,
                confirmed,
            } => {
                if !confirmed {
                    return Response::coded_error(
                        DaemonErrorCode::SafetyRefused,
                        "explicit confirmation is required",
                    );
                }
                let policy = match policy_for_slots(&to_setlist, &[&to_slot]) {
                    Ok(policy) => policy,
                    Err(error) => return Response::cortex_error(&error),
                };
                self.respond(move |c| {
                    c.copy_preset(
                        &policy,
                        &from_setlist,
                        &from_slot,
                        &to_setlist,
                        &to_slot,
                        name.as_deref(),
                        instrument,
                        cortex_rs::RecallConsent::DiscardWorkingCopy,
                        COPY_DEVICE_STEP_TIMEOUT,
                    )
                })
            }
            Request::CreateSetlist { name, confirmed } => {
                if !confirmed {
                    return Response::coded_error(
                        DaemonErrorCode::SafetyRefused,
                        "explicit confirmation is required",
                    );
                }
                self.respond(move |c| c.create_setlist(&name, SETLIST_DEVICE_TIMEOUT))
            }
            Request::DeleteSetlist { name, confirmed } => {
                if !confirmed {
                    return Response::coded_error(
                        DaemonErrorCode::SafetyRefused,
                        "explicit confirmation is required",
                    );
                }
                self.respond(move |c| {
                    c.delete_setlist(&name, SETLIST_DEVICE_TIMEOUT)
                        .map(|()| serde_json::json!({ "deleted": name }))
                })
            }
            Request::DuplicateSetlist {
                source_name,
                destination_name,
                limit,
                confirmed,
            } => {
                if !confirmed {
                    return Response::coded_error(
                        DaemonErrorCode::SafetyRefused,
                        "explicit confirmation is required",
                    );
                }
                self.respond(move |c| {
                    c.duplicate_setlist(
                        &source_name,
                        &destination_name,
                        limit,
                        cortex_rs::RecallConsent::DiscardWorkingCopy,
                        Duration::from_secs(60),
                    )
                })
            }
            Request::CpuLoad => match self.state.cpu_load() {
                Some(load) if self.cache_is_usable() => {
                    Response::ok(&cortex_rs::view::CpuLoad::from(&load.value))
                        .unwrap_or_else(|e| Response::error(format!("CpuLoad: {e}")))
                }
                None => Response::coded_error(
                    DaemonErrorCode::NotReady,
                    "no CPU load received yet - the device pushes it about once a second \
                     after subscribing"
                        .to_string(),
                ),
                Some(_) => Response::coded_error(
                    DaemonErrorCode::DeviceUnavailable,
                    "the device is not currently responsive",
                ),
            },
            Request::AnalyzeCpuFit => match (self.state.cpu_load(), self.state.current_preset()) {
                (Some(load), Some(preset)) if self.cache_is_usable() => {
                    let cpu = cortex_rs::view::CpuLoad::from(&load.value);
                    let grid = cortex_rs::view::Preset::from_binary(
                        &preset.value,
                        self.catalog().as_ref(),
                        "(live grid)",
                        "(live grid)",
                        false,
                    );
                    Response::ok(&cortex_host::CpuFitAnalysis::from_live(&cpu, &grid))
                        .unwrap_or_else(|error| Response::error(format!("CPU fit: {error}")))
                }
                (None, _) => Response::coded_error(
                    DaemonErrorCode::NotReady,
                    "no CPU load received yet - the device pushes it about once a second after subscribing",
                ),
                (_, None) => Response::coded_error(
                    DaemonErrorCode::NotReady,
                    "no live grid is available yet - wait for the subscribed session to settle",
                ),
                _ => Response::coded_error(
                    DaemonErrorCode::DeviceUnavailable,
                    "the device is not currently responsive",
                ),
            },
            Request::Shutdown => unreachable!("shutdown is handled before device admission"),
        }
    }

    fn begin_shutdown(&self) -> Response {
        self.shutdown();
        Response::ok(&serde_json::json!({ "stopping": true }))
            .unwrap_or_else(|error| Response::error(error.to_string()))
    }

    fn reconnect_now(&self) -> Response {
        let requested = match self.health.lock().unwrap().clone() {
            RuntimeHealth::Reconnecting { .. } => true,
            RuntimeHealth::Connected => !self.session().is_responsive(),
            RuntimeHealth::Failed { error } => {
                return Response::coded_error(
                    DaemonErrorCode::DeviceUnavailable,
                    format!("reconnect monitor failed: {error}"),
                );
            }
        };
        if requested {
            self.reconnect.retry_now.store(true, Ordering::Release);
        }
        Response::ok(&requested).unwrap_or_else(|error| {
            Response::error(format!("building reconnect response: {error}"))
        })
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
            Err(e) => Response::cortex_error(&e),
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
            daemon_version: DAEMON_PROTOCOL_VERSION.to_string(),
            uptime_seconds: self.started.elapsed().as_secs(),
            auto_managed: self.lifecycle.is_auto_managed(),
            idle_timeout_seconds: self.lifecycle.idle_timeout().map(|value| value.as_secs()),
            device_kind: DeviceKind::QuadCortex,
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
        let reconnect = self.reconnect.clone();
        let operations = self.operations.clone();
        let result = std::thread::Builder::new()
            .name("cortex-reconnect".into())
            .spawn(move || {
                reconnect_loop(
                    session,
                    state,
                    health.clone(),
                    reconnect,
                    operations,
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
        self.accepting_device_requests
            .store(false, Ordering::Release);
        self.reconnect.stopping.store(true, Ordering::Relaxed);
        if self.force_exit_on_shutdown && !self.shutdown_ready.load(Ordering::Acquire) {
            let shutdown_ready = Arc::clone(&self.shutdown_ready);
            std::thread::spawn(move || {
                std::thread::sleep(SHUTDOWN_GRACE);
                if shutdown_ready.load(Ordering::Acquire) {
                    return;
                }
                eprintln!(
                    "cortex session: device operation did not finish within {}s; exiting without acknowledging shutdown",
                    SHUTDOWN_GRACE.as_secs()
                );
                std::process::exit(0);
            });
        }
        // The same exclusive gate covers ordinary operations and reconnect.
        // Closing under it means a successful explicit response proves no
        // device call can continue after the acknowledgement.
        let _operation = self.operations.lock().unwrap();
        self.session().close();
        self.shutdown_ready.store(true, Ordering::Release);
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
        session.close();
        return Err(error.into());
    }
    seed_session(&session, state);
    if state.status().phase != cortex_rs::CachePhase::Live {
        session.close();
        anyhow::bail!("replacement session did not establish a live subscribed cache");
    }
    Ok(session)
}

fn reconnect_loop<F>(
    session_slot: Arc<Mutex<Arc<Session>>>,
    state: cortex_rs::DeviceStateCache,
    health: Arc<Mutex<RuntimeHealth>>,
    reconnect: Arc<ReconnectControl>,
    operations: Arc<Mutex<()>>,
    timings: ReconnectTimings,
    connect: F,
) where
    F: Fn(&cortex_rs::DeviceStateCache) -> Result<Arc<Session>>,
{
    while reconnect.wait(timings.poll) {
        let current = session_slot.lock().unwrap().clone();
        let cache_phase = state.status().phase;
        if current.is_responsive() && cache_phase != cortex_rs::CachePhase::Invalidated {
            continue;
        }

        let silent_for = current.seconds_since_last_message();
        let mut attempts = 0_u32;
        let mut delay = timings.initial_backoff;
        let mut last_error = if cache_phase == cortex_rs::CachePhase::Invalidated {
            "subscribed state continuity was lost".to_string()
        } else {
            format!("device silent for {silent_for}s")
        };
        state.invalidate(last_error.clone());
        *health.lock().unwrap() = RuntimeHealth::Reconnecting {
            attempts,
            last_error: last_error.clone(),
        };
        eprintln!("cortex session: {last_error}; reconnecting");

        // Health changes first so no new request can acquire the operation
        // side while recovery waits for existing calls to drain.
        let _recovery = operations.lock().unwrap();

        // The old handle must be gone before opening another. Concurrent HID
        // ownership is accepted initially and wedges the next request.
        current.close();

        loop {
            if reconnect.stopping.load(Ordering::Relaxed) {
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
                    if reconnect.stopping.load(Ordering::Relaxed) {
                        replacement.close();
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
                    if !reconnect.wait(delay) {
                        return;
                    }
                    delay = (delay * 2).min(timings.max_backoff);
                }
            }
        }
    }
}

struct NanoDaemon {
    transport: Mutex<Transport>,
    state: Mutex<NanoCache>,
    health: Mutex<DeviceHealth>,
    started: Instant,
    lifecycle: DaemonLifecycle,
}

struct NanoCache {
    snapshot: cortex_rs::nano::NanoCurrentState,
    last_attempt: Instant,
    last_received: Instant,
    revision: u64,
}

impl NanoDaemon {
    fn connect(lifecycle: DaemonLifecycle) -> Result<Self> {
        eprintln!("cortex session: opening Nano Cortex ...");
        let transport = Transport::open(DeviceKind::NanoCortex)?;
        // Prove the editor channel is answering before startup reports ready.
        let state = cortex_rs::nano::read_current_state(&transport, Duration::from_secs(5))?;
        eprintln!("cortex session: Nano Cortex connected");
        Ok(Self {
            transport: Mutex::new(transport),
            state: Mutex::new(NanoCache {
                snapshot: state,
                last_attempt: Instant::now(),
                last_received: Instant::now(),
                revision: 1,
            }),
            health: Mutex::new(DeviceHealth::Connected {
                serial: None,
                coros_version: None,
                last_message_seconds: 0,
            }),
            started: Instant::now(),
            lifecycle,
        })
    }

    fn handle(&self, request: Request) -> Response {
        match request {
            Request::Status => {
                let cached = self.state.lock().unwrap();
                let cache = CacheStatus {
                    generation: 1,
                    revision: cached.revision,
                    phase: cortex_rs::CachePhase::Live,
                    ..CacheStatus::default()
                };
                let mut device = self.health.lock().unwrap().clone();
                if let DeviceHealth::Connected {
                    last_message_seconds,
                    ..
                } = &mut device
                {
                    *last_message_seconds = cached.last_received.elapsed().as_secs();
                }
                Response::ok(&Status {
                    daemon_version: DAEMON_PROTOCOL_VERSION.to_string(),
                    uptime_seconds: self.started.elapsed().as_secs(),
                    auto_managed: self.lifecycle.is_auto_managed(),
                    idle_timeout_seconds: self
                        .lifecycle
                        .idle_timeout()
                        .map(|value| value.as_secs()),
                    device_kind: DeviceKind::NanoCortex,
                    device,
                    cache,
                })
                .unwrap_or_else(|error| Response::error(error.to_string()))
            }
            Request::NanoState => match self.transport.try_lock() {
                Ok(transport) => {
                    let mut cached = self.state.lock().unwrap();
                    if cached.last_attempt.elapsed() >= Duration::from_secs(5) {
                        cached.last_attempt = Instant::now();
                        match cortex_rs::nano::read_current_state(
                            &transport,
                            Duration::from_secs(5),
                        ) {
                            Ok(state) => {
                                cached.snapshot = state;
                                cached.last_received = Instant::now();
                                cached.revision += 1;
                                *self.health.lock().unwrap() = DeviceHealth::Connected {
                                    serial: None,
                                    coros_version: None,
                                    last_message_seconds: 0,
                                };
                            }
                            Err(error) => {
                                *self.health.lock().unwrap() = DeviceHealth::Failed {
                                    error: error.to_string(),
                                };
                                return Response::cortex_error(&error);
                            }
                        }
                    }
                    Response::ok(&cached.snapshot)
                        .unwrap_or_else(|error| Response::error(error.to_string()))
                }
                Err(TryLockError::WouldBlock) => {
                    // A write is in progress (amp/bypass takes ~6s). Return
                    // the cached snapshot rather than erroring, so the GUI's
                    // auto-refresh gets a usable state instead of a failure.
                    let cached = self.state.lock().unwrap();
                    Response::ok(&cached.snapshot)
                        .unwrap_or_else(|error| Response::error(error.to_string()))
                }
                Err(TryLockError::Poisoned(_)) => {
                    Response::error("Nano transport lock is unavailable")
                }
            },
            Request::NanoSetAmp { control, value } => match self.transport.try_lock() {
                Ok(transport) => {
                    if let Err(error) = cortex_rs::nano::write_amp(&transport, control, value) {
                        return Response::cortex_error(&error);
                    }
                    std::thread::sleep(Duration::from_secs(6));
                    match cortex_rs::nano::read_current_state(&transport, Duration::from_secs(5)) {
                        Ok(state) if state.amp.value(control) == Some(value) => {
                            let mut cached = self.state.lock().unwrap();
                            cached.snapshot = state.clone();
                            cached.last_attempt = Instant::now();
                            cached.last_received = Instant::now();
                            cached.revision += 1;
                            *self.health.lock().unwrap() = DeviceHealth::Connected {
                                serial: None,
                                coros_version: None,
                                last_message_seconds: 0,
                            };
                            Response::ok(&state)
                                .unwrap_or_else(|error| Response::error(error.to_string()))
                        }
                        Ok(state) => {
                            let actual = state.amp.value(control);
                            let mut cached = self.state.lock().unwrap();
                            cached.snapshot = state;
                            cached.last_attempt = Instant::now();
                            cached.last_received = Instant::now();
                            cached.revision += 1;
                            Response::coded_error(
                                DaemonErrorCode::OutcomeUnconfirmed,
                                format!(
                                    "Nano amp write did not read back: expected {value}, got {actual:?}"
                                ),
                            )
                        }
                        Err(error) => {
                            *self.health.lock().unwrap() = DeviceHealth::Failed {
                                error: error.to_string(),
                            };
                            Response::cortex_error(&error)
                        }
                    }
                }
                Err(TryLockError::WouldBlock) => Response::coded_error(
                    DaemonErrorCode::Busy,
                    "another Nano operation is already in progress",
                ),
                Err(TryLockError::Poisoned(_)) => {
                    Response::error("Nano transport lock is unavailable")
                }
            },
            Request::NanoSetBypass { target, bypassed } => match self.transport.try_lock() {
                Ok(transport) => {
                    if let Err(error) = cortex_rs::nano::write_bypass(&transport, target, bypassed)
                    {
                        return Response::cortex_error(&error);
                    }
                    std::thread::sleep(Duration::from_secs(6));
                    match cortex_rs::nano::read_current_state(&transport, Duration::from_secs(5)) {
                        Ok(state) => {
                            let actual = state
                                .slots
                                .iter()
                                .find(|slot| slot.role == target.role())
                                .and_then(|slot| slot.bypassed);
                            let confirmed = actual == Some(bypassed);
                            let mut cached = self.state.lock().unwrap();
                            cached.snapshot = state.clone();
                            cached.last_attempt = Instant::now();
                            cached.last_received = Instant::now();
                            cached.revision += 1;
                            *self.health.lock().unwrap() = DeviceHealth::Connected {
                                serial: None,
                                coros_version: None,
                                last_message_seconds: 0,
                            };
                            if confirmed {
                                Response::ok(&state)
                                    .unwrap_or_else(|error| Response::error(error.to_string()))
                            } else {
                                Response::coded_error(
                                    DaemonErrorCode::OutcomeUnconfirmed,
                                    format!(
                                        "Nano bypass write did not read back: expected {bypassed}, got {actual:?}"
                                    ),
                                )
                            }
                        }
                        Err(error) => {
                            *self.health.lock().unwrap() = DeviceHealth::Failed {
                                error: error.to_string(),
                            };
                            Response::cortex_error(&error)
                        }
                    }
                }
                Err(TryLockError::WouldBlock) => Response::coded_error(
                    DaemonErrorCode::Busy,
                    "another Nano operation is already in progress",
                ),
                Err(TryLockError::Poisoned(_)) => {
                    Response::error("Nano transport lock is unavailable")
                }
            },
            Request::NanoReadFxParams { slot } => match self.transport.try_lock() {
                Ok(transport) => {
                    match cortex_rs::nano::read_fx_params(&transport, slot, Duration::from_secs(5))
                    {
                        Ok(values) => Response::ok(&values)
                            .unwrap_or_else(|error| Response::error(error.to_string())),
                        Err(error) => {
                            *self.health.lock().unwrap() = DeviceHealth::Failed {
                                error: error.to_string(),
                            };
                            Response::cortex_error(&error)
                        }
                    }
                }
                Err(TryLockError::WouldBlock) => Response::coded_error(
                    DaemonErrorCode::Busy,
                    "another Nano operation is already in progress",
                ),
                Err(TryLockError::Poisoned(_)) => {
                    Response::error("Nano transport lock is unavailable")
                }
            },
            Request::NanoSetFxParam {
                slot,
                param_index,
                value,
            } => match self.transport.try_lock() {
                Ok(transport) => {
                    if let Err(error) =
                        cortex_rs::nano::write_fx_param(&transport, slot, param_index, value)
                    {
                        return Response::cortex_error(&error);
                    }
                    std::thread::sleep(Duration::from_secs(2));
                    match cortex_rs::nano::read_fx_params(&transport, slot, Duration::from_secs(5))
                    {
                        Ok(values)
                            if param_index < values.len() as u8
                                && (values[param_index as usize] - value).abs() < 0.001 =>
                        {
                            Response::ok(&values)
                                .unwrap_or_else(|error| Response::error(error.to_string()))
                        }
                        Ok(values) => {
                            let actual = values.get(param_index as usize).copied();
                            Response::coded_error(
                                DaemonErrorCode::OutcomeUnconfirmed,
                                format!(
                                    "Nano FX param write did not read back: expected {value}, got {actual:?}"
                                ),
                            )
                        }
                        Err(error) => {
                            *self.health.lock().unwrap() = DeviceHealth::Failed {
                                error: error.to_string(),
                            };
                            Response::cortex_error(&error)
                        }
                    }
                }
                Err(TryLockError::WouldBlock) => Response::coded_error(
                    DaemonErrorCode::Busy,
                    "another Nano operation is already in progress",
                ),
                Err(TryLockError::Poisoned(_)) => {
                    Response::error("Nano transport lock is unavailable")
                }
            },
            Request::Shutdown => Response::ok(&serde_json::json!({ "stopping": true }))
                .unwrap_or_else(|error| Response::error(error.to_string())),
            _ => Response::coded_error(
                DaemonErrorCode::Protocol,
                "the held session owns a Nano Cortex; this operation requires a Quad Cortex",
            ),
        }
    }
}

enum DeviceDaemon {
    Quad(Daemon),
    Nano(NanoDaemon),
}

impl DeviceDaemon {
    fn handle(&self, request: Request) -> Response {
        match self {
            Self::Quad(daemon) => daemon.handle(request),
            Self::Nano(daemon) => daemon.handle(request),
        }
    }

    fn start_monitor(&self) {
        if let Self::Quad(daemon) = self {
            daemon.start_reconnect_monitor();
        }
    }

    fn shutdown(&self) {
        if let Self::Quad(daemon) = self {
            daemon.shutdown();
        }
    }
}

/// Run the daemon until explicitly stopped or its host-managed idle bound expires.
pub fn run(device: DeviceKind, lifecycle: DaemonLifecycle) -> Result<()> {
    refuse_legacy_nano_owner(device)?;
    let endpoint = LocalEndpoint::for_device(device);

    // Claim local IPC BEFORE the handshake, not after.
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
    let claim = LocalListener::bind(&endpoint).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AddrInUse {
            anyhow::anyhow!(
                "a cortex daemon is already running on {endpoint}. Stop it with \
                 `cortex session stop --device {}`, or use the existing one.",
                match device {
                    DeviceKind::QuadCortex => "quad",
                    DeviceKind::NanoCortex => "nano",
                }
            )
        } else {
            anyhow::Error::new(error).context(format!("claiming local endpoint {endpoint}"))
        }
    })?;
    if claim.removed_stale_endpoint {
        eprintln!("cortex session: removed a stale endpoint at {endpoint} (nothing was listening)");
    }
    let listener = claim.listener;

    // The endpoint now exists, so a failed handshake must clear it on the way
    // out; otherwise the next start finds a file it has to treat as stale.
    let daemon = match match device {
        DeviceKind::QuadCortex => Daemon::connect(lifecycle).map(DeviceDaemon::Quad),
        DeviceKind::NanoCortex => NanoDaemon::connect(lifecycle).map(DeviceDaemon::Nano),
    } {
        Ok(daemon) => daemon,
        Err(e) => {
            let _ = listener.cleanup_endpoint();
            return Err(e);
        }
    };

    eprintln!("cortex session: listening on {endpoint}");
    match lifecycle {
        DaemonLifecycle::Explicit => {
            eprintln!(
                "cortex session: stop with `cortex session stop --device {}`, or Ctrl-C if attached",
                match device {
                    DeviceKind::QuadCortex => "quad",
                    DeviceKind::NanoCortex => "nano",
                }
            );
        }
        DaemonLifecycle::AutoManaged { idle_timeout } => eprintln!(
            "cortex session: auto-managed; exits after {}s without a completed request",
            idle_timeout.as_secs()
        ),
    }
    daemon.start_monitor();

    let serving = serve_listener(&listener, lifecycle, &|request| daemon.handle(request));
    if matches!(serving, Ok(cortex_host::ServerExit::IdleTimeout)) {
        eprintln!("cortex session: request-idle timeout reached");
    }

    eprintln!("cortex session: shutting down");

    daemon.shutdown();
    // Last, so the endpoint never outlives our claim on the device: while it
    // answers, other commands correctly refuse to open the device for
    // themselves. A file left behind by a forced exit is inert - nothing is
    // listening on it - and the next start clears it as stale.
    let _ = listener.cleanup_endpoint();
    serving?;
    Ok(())
}

fn refuse_legacy_nano_owner(device: DeviceKind) -> Result<()> {
    if device != DeviceKind::NanoCortex {
        return Ok(());
    }

    // Nano sessions used the Quad endpoint before product-scoped endpoints
    // landed. Refuse an already-running legacy owner; concurrently launching
    // mixed binary versions is unsupported because the older process cannot
    // participate in the new product-scoped ownership contract.
    let legacy = DaemonClient::default().with_timeout(Duration::from_secs(2));
    if !legacy.is_running() {
        return Ok(());
    }
    match legacy.status() {
        Ok(status) if status.device_kind == DeviceKind::NanoCortex => anyhow::bail!(
            "a Nano Cortex session is still running on the legacy endpoint. Stop it with `cortex session stop --device quad`, then retry."
        ),
        Ok(_) => Ok(()),
        Err(error) => anyhow::bail!(
            "could not verify the owner of the legacy Cortex endpoint while starting Nano: {error}. Wait for the other session to finish starting, then retry."
        ),
    }
}

/// Whether the accept loop should continue.
#[derive(Debug, PartialEq, Eq)]
#[cfg(test)]
enum Control {
    Continue,
    Stop,
}

/// Serve one connection. A client may send several requests on one stream.
#[cfg(test)]
fn serve(daemon: &Daemon, stream: LocalConnection) -> Control {
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
            Err(e) => Response::coded_error(
                DaemonErrorCode::InvalidRequest,
                format!("could not parse request: {e}"),
            ),
        };

        if write_response(&mut writer, &response).is_err() {
            // The client hung up mid-response. Not our problem, and not a
            // reason to stop serving everyone else.
            return Control::Continue;
        }
    }
    Control::Continue
}

#[cfg(test)]
fn write_response(writer: &mut LocalConnection, response: &Response) -> std::io::Result<()> {
    let mut line = serde_json::to_string(response).unwrap_or_else(|_| {
        r#"{"status":"error","code":"internal","message":"could not serialise response"}"#
            .to_string()
    });
    line.push('\n');
    writer.write_all(line.as_bytes())?;
    writer.flush()
}

/// Whether a daemon is listening on the local endpoint.
///
/// This is the guard for direct device access. The device grants its HID
/// interface to one owner: opening it while the daemon holds it does not
/// fail loudly, it wedges the held session - the device simply stops
/// sending, heartbeat included, and every subsequent read times out. A
/// stale socket file does not count, so this connects rather than checking
/// for the path.
#[must_use]
pub fn is_running() -> bool {
    cortex_host::is_running()
}

/// Whether one product's daemon is listening or starting.
#[must_use]
pub fn is_running_for(device: DeviceKind) -> bool {
    DaemonClient::for_device(device).is_running()
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
pub fn start_detached(device: DeviceKind, lifecycle: DaemonLifecycle) -> Result<()> {
    if is_running_for(device) {
        anyhow::bail!(
            "a cortex session is already running for {:?}. \
             Check it with `cortex session status --device {}`, or replace it with \
             `cortex session stop --device {}` first.",
            device,
            match device {
                DeviceKind::QuadCortex => "quad",
                DeviceKind::NanoCortex => "nano",
            },
            match device {
                DeviceKind::QuadCortex => "quad",
                DeviceKind::NanoCortex => "nano",
            }
        );
    }

    let log = LocalEndpoint::for_device(device).log_path();
    let file = std::fs::File::create(&log)?;
    let exe = std::env::current_exe()?;

    let mut cmd = std::process::Command::new(exe);
    cmd.args(["session", "start", "--foreground"]);
    cmd.arg("--device").arg(match device {
        DeviceKind::QuadCortex => "quad",
        DeviceKind::NanoCortex => "nano",
    });
    if let DaemonLifecycle::AutoManaged { idle_timeout } = lifecycle {
        cmd.arg("--auto-managed")
            .arg("--idle-timeout-seconds")
            .arg(idle_timeout.as_secs().to_string());
    }
    cmd.stdin(std::process::Stdio::null())
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
        if answers_status(device, Duration::from_secs(2)) {
            eprintln!(
                "cortex session: ready. Stop it with `cortex session stop --device {}`.",
                match device {
                    DeviceKind::QuadCortex => "quad",
                    DeviceKind::NanoCortex => "nano",
                }
            );
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
fn answers_status(device: DeviceKind, timeout: Duration) -> bool {
    DaemonClient::for_device(device)
        .with_timeout(timeout)
        .require_compatible()
        .is_ok()
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
    cortex_host::request(request)
}

/// Send a composition whose legitimate duration exceeds the ordinary IPC bound.
pub fn request_with_timeout(
    request: &Request,
    timeout: Duration,
) -> Option<Result<serde_json::Value>> {
    cortex_host::request_with_timeout(request, timeout)
}

#[cfg(test)]
fn request_on(stream: LocalConnection, request: &Request) -> Result<serde_json::Value> {
    stream.set_read_timeout(Some(SOCKET_REQUEST_TIMEOUT))?;
    stream.set_write_timeout(Some(SOCKET_REQUEST_TIMEOUT))?;
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream.try_clone()?);

    // Version gate: check the daemon's protocol version before sending
    // a request it might not understand. An old daemon that survived an
    // upgrade would otherwise misparse a new request shape.
    let version_line = serde_json::to_string(&Request::Status)?;
    let mut vl = version_line.clone();
    vl.push('\n');
    writer.write_all(vl.as_bytes())?;
    writer.flush()?;
    let mut status_reply = String::new();
    reader.read_line(&mut status_reply)?;
    let status: Response = serde_json::from_str(&status_reply)?;
    let status_data = match status {
        Response::Ok { data } => data,
        Response::Error { message, .. } => anyhow::bail!("{message}"),
    };
    let daemon_version: u32 = status_data
        .get("daemon_version")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    if daemon_version != DAEMON_PROTOCOL_VERSION {
        anyhow::bail!(
            "daemon protocol version mismatch: client expects {}, daemon reports {}. \
                 The daemon is older or newer than this CLI. \
                 Run `cortex session stop` to stop the old daemon, then retry.",
            DAEMON_PROTOCOL_VERSION,
            daemon_version
        );
    }

    // The daemon serves each accepted stream until EOF, so send the real
    // request on this same stream. Opening a second connection while
    // retaining the first deadlocks against that serial accept loop.
    let mut line = serde_json::to_string(request)?;
    line.push('\n');
    writer.write_all(line.as_bytes())?;
    writer.flush()?;

    let mut reply = String::new();
    reader.read_line(&mut reply)?;
    match serde_json::from_str::<Response>(&reply)? {
        Response::Ok { data } => Ok(data),
        Response::Error { message, .. } => anyhow::bail!("{message}"),
    }
}

#[cfg(test)]
mod tests {
    //! The daemon over a fake session.
    //!
    //! Everything here is independent of how the session was obtained: the
    //! local IPC protocol, the shape of a status, and how a call that cannot be
    //! answered is reported. Those were previously verified only by hand
    //! against real hardware, which is why this file sat at 0% coverage
    //! through several rewrites of its shutdown and readiness logic.
    //!
    //! Device-bound requests are deliberately absent: `REQUEST_TIMEOUT` is
    //! 30 s, so a test that waited for one would trade half a minute for
    //! nothing the fake can meaningfully assert.

    use super::*;
    use cortex_rs::link::{FakeLink, HidLink};
    use std::sync::atomic::AtomicUsize;

    struct ExclusiveLink {
        inner: FakeLink,
        active: Arc<AtomicBool>,
    }

    impl ExclusiveLink {
        fn new(inner: FakeLink, active: Arc<AtomicBool>) -> Self {
            assert!(!active.swap(true, Ordering::AcqRel), "link opened twice");
            Self { inner, active }
        }
    }

    impl HidLink for ExclusiveLink {
        fn write(&self, report: &[u8]) -> cortex_rs::Result<usize> {
            self.inner.write(report)
        }

        fn read_timeout(&self, buf: &mut [u8], timeout_ms: i32) -> cortex_rs::Result<usize> {
            self.inner.read_timeout(buf, timeout_ms)
        }
    }

    impl Drop for ExclusiveLink {
        fn drop(&mut self) {
            self.active.store(false, Ordering::Release);
        }
    }

    fn fake_daemon() -> Daemon {
        let link = FakeLink::new();
        Daemon::over(Arc::new(
            Session::over(link).expect("session over a fake link"),
        ))
    }

    fn fake_daemon_with_link() -> (Daemon, FakeLink) {
        let link = FakeLink::new();
        let daemon = Daemon::over(Arc::new(
            Session::over(link.clone()).expect("session over a fake link"),
        ));
        (daemon, link)
    }

    fn push_state<T: prost::Message>(
        daemon: &Daemon,
        link: &FakeLink,
        message_type: cortex_rs::proto::cortex_message_type::Enum,
        message: &T,
    ) {
        let before = daemon.state.status().revision;
        link.push_inbound(inbound(message_type as u16, &message.encode_to_vec()));
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline && daemon.state.status().revision == before {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_ne!(
            daemon.state.status().revision,
            before,
            "fake state push was not observed"
        );
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
        let (mut client, server) = LocalConnection::pair().expect("local IPC pair");
        let mut line = serde_json::to_string(request).expect("encode");
        line.push('\n');
        client.write_all(line.as_bytes()).expect("write");
        client.shutdown_write().expect("half close");

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
        let (mut client, server) = LocalConnection::pair().expect("local IPC pair");
        client.write_all(b"this is not json\n").expect("write");
        client.shutdown_write().expect("half close");

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
        let Response::Error { code, message } = daemon.handle(Request::CpuLoad) else {
            panic!("a fake session has received no CPU load, so this cannot succeed");
        };
        assert_eq!(code, DaemonErrorCode::NotReady);
        // The first push lands about 8s after subscribing, so "nothing yet"
        // is a normal state and has to read like one.
        assert!(
            message.contains("subscrib"),
            "the error should say why there is nothing yet: {message}"
        );
        daemon.shutdown();
    }

    #[test]
    fn parsed_catalog_tracks_new_models_and_the_exact_replacement_payload() {
        use cortex_rs::proto::{ModelRepoMessage, NewModelsMessage, model_repo_message};

        let (daemon, link) = fake_daemon_with_link();
        push_state(
            &daemon,
            &link,
            cortex_rs::proto::cortex_message_type::Enum::ModelRepo,
            &ModelRepoMessage {
                model_repo_payload: Some(model_repo_message::ModelRepoPayload::ModelRepoPayload(
                    vec![0xa1],
                )),
                ..Default::default()
            },
        );
        let payload = daemon.state.model_repo().expect("catalog A payload");
        let catalog_a = cortex_rs::Catalog::from_xml(
            r#"<Models><Category id="1" name="Fictional"><Model id="41" name="Catalog A"><Parameter name="OLD" type="float" min="0" max="1"/></Model></Category></Models>"#,
        )
        .expect("catalog A");
        *daemon.catalog.lock().unwrap() = Some((payload.generation, payload.revision, catalog_a));
        assert_eq!(daemon.catalog().unwrap().get(41).unwrap().name, "Catalog A");

        push_state(
            &daemon,
            &link,
            cortex_rs::proto::cortex_message_type::Enum::NewModels,
            &NewModelsMessage::default(),
        );
        assert_eq!(
            daemon.catalog().unwrap().get(41).unwrap().name,
            "Catalog A",
            "an empty NewModels announcement must not invalidate catalog A"
        );

        push_state(
            &daemon,
            &link,
            cortex_rs::proto::cortex_message_type::Enum::NewModels,
            &NewModelsMessage {
                models: vec![42],
                ..Default::default()
            },
        );

        assert!(daemon.catalog().is_none());
        assert!(
            daemon.catalog.lock().unwrap().is_none(),
            "parsed catalog A must not survive invalidation for later name or parameter resolution"
        );
        assert!(
            daemon.session().captured_model_repo().is_none(),
            "client-side named parameter resolution must not recover catalog A"
        );

        push_state(
            &daemon,
            &link,
            cortex_rs::proto::cortex_message_type::Enum::ModelRepo,
            &ModelRepoMessage {
                model_repo_payload: Some(model_repo_message::ModelRepoPayload::ModelRepoPayload(
                    vec![0xb2],
                )),
                ..Default::default()
            },
        );
        let payload = daemon.state.model_repo().expect("catalog B payload");
        let catalog_b = cortex_rs::Catalog::from_xml(
            r#"<Models><Category id="1" name="Fictional"><Model id="41" name="Catalog B"><Parameter name="NEW" type="float" min="0" max="1"/></Model></Category></Models>"#,
        )
        .expect("catalog B");
        *daemon.catalog.lock().unwrap() = Some((payload.generation, payload.revision, catalog_b));
        let current = daemon.catalog().expect("parsed catalog B");
        let model = current.get(41).expect("model from B");
        assert_eq!(model.name, "Catalog B");
        assert!(model.parameter("NEW").is_some());
        assert!(
            model.parameter("OLD").is_none(),
            "name and parameter resolution must use B rather than stale A"
        );
        daemon.shutdown();
    }

    #[test]
    fn an_unconfirmed_preset_move_is_refused_before_device_io() {
        let daemon = fake_daemon();
        let Response::Error { code, message } = daemon.handle(Request::MovePreset {
            setlist: cortex_rs::client::USER_SETLIST.into(),
            from_slot: "2A".into(),
            to_slot: "2B".into(),
            confirmed: false,
        }) else {
            panic!("an unconfirmed move must be refused");
        };
        assert!(
            message.contains("confirmation"),
            "unexpected error: {message}"
        );
        assert_eq!(code, DaemonErrorCode::SafetyRefused);
        daemon.shutdown();
    }

    #[test]
    fn an_unconfirmed_setlist_create_is_refused_before_device_io() {
        let daemon = fake_daemon();
        let response = daemon.handle(Request::CreateSetlist {
            name: "Fictional Setlist".into(),
            confirmed: false,
        });
        assert!(
            matches!(response, Response::Error { message, .. } if message.contains("confirmation"))
        );
        daemon.shutdown();
    }

    #[test]
    fn concurrent_device_requests_are_refused_by_the_exclusive_operation_gate() {
        let daemon = Arc::new(fake_daemon());
        let held = daemon.operations.lock().unwrap();
        assert!(matches!(
            daemon.handle(Request::Status),
            Response::Ok { .. }
        ));
        let response = daemon.handle(Request::CpuLoad);
        assert!(matches!(
            response,
            Response::Error { message, .. } if message.contains("already in progress")
        ));
        drop(held);
        daemon.shutdown();
    }

    #[test]
    fn shutdown_is_not_acknowledged_until_the_device_gate_is_released() {
        let daemon = Arc::new(fake_daemon());
        let held = daemon.operations.lock().unwrap();
        let worker_daemon = Arc::clone(&daemon);
        let (sent, received) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            sent.send(worker_daemon.handle(Request::Shutdown)).unwrap();
        });

        assert!(
            received.recv_timeout(Duration::from_millis(50)).is_err(),
            "shutdown was acknowledged while a device operation was in flight"
        );
        assert!(!daemon.accepting_device_requests.load(Ordering::Acquire));
        drop(held);
        assert!(matches!(
            received.recv_timeout(Duration::from_secs(1)).unwrap(),
            Response::Ok { .. }
        ));
        worker.join().unwrap();
        daemon.shutdown();
    }

    #[test]
    fn requests_fail_fast_while_reconnecting() {
        let daemon = fake_daemon();
        *daemon.health.lock().unwrap() = RuntimeHealth::Reconnecting {
            attempts: 3,
            last_error: "device unplugged".into(),
        };
        let Response::Error { code, message } = daemon.handle(Request::ActiveScene) else {
            panic!("a request must not wait on a session being replaced");
        };
        assert_eq!(code, DaemonErrorCode::Reconnecting);
        assert!(message.contains("attempt 3"), "unexpected error: {message}");
        let Response::Ok { data } = daemon.handle(Request::Status) else {
            panic!("status must remain available while reconnecting");
        };
        assert_eq!(data["device"]["state"], "reconnecting");
        let Response::Ok { data } = daemon.handle(Request::ReconnectNow) else {
            panic!("manual retry must remain available while reconnecting");
        };
        assert_eq!(data, true);
        assert!(daemon.reconnect.retry_now.load(Ordering::Acquire));
        daemon.shutdown();
    }

    #[test]
    fn reconnect_retries_then_swaps_in_a_new_generation() {
        let state = cortex_rs::DeviceStateCache::new();
        let initial_link = FakeLink::new();
        let active_lease = Arc::new(AtomicBool::new(false));
        let initial = Arc::new(
            Session::over_with_state(
                ExclusiveLink::new(initial_link.clone(), active_lease.clone()),
                state.clone(),
            )
            .expect("initial fake session"),
        );
        let _retained_old_session = initial.clone();
        initial_link.push_inbound(inbound(
            cortex_rs::proto::cortex_message_type::Enum::GlobalTempo as u16,
            &[],
        ));
        let heard_deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < heard_deadline && !initial.is_responsive() {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(initial.is_responsive());
        state.invalidate("fictional stream gap");
        let slot = Arc::new(Mutex::new(initial));
        let health = Arc::new(Mutex::new(RuntimeHealth::Connected));
        let reconnect = Arc::new(ReconnectControl::default());
        let operations = Arc::new(Mutex::new(()));
        let attempts = Arc::new(AtomicUsize::new(0));
        let connector_attempts = attempts.clone();
        let connector_lease = active_lease.clone();
        let worker_slot = slot.clone();
        let worker_state = state.clone();
        let worker_health = health.clone();
        let worker_reconnect = reconnect.clone();
        let worker_operations = operations.clone();

        let worker = std::thread::spawn(move || {
            reconnect_loop(
                worker_slot,
                worker_state,
                worker_health,
                worker_reconnect,
                worker_operations,
                ReconnectTimings {
                    poll: Duration::from_millis(1),
                    initial_backoff: Duration::from_millis(1),
                    max_backoff: Duration::from_millis(2),
                },
                move |state| {
                    assert!(
                        !connector_lease.load(Ordering::Acquire),
                        "replacement opened before old link dropped"
                    );
                    let attempt = connector_attempts.fetch_add(1, Ordering::Relaxed) + 1;
                    if attempt == 1 {
                        anyhow::bail!("fictional first-open failure");
                    }
                    let link = FakeLink::new();
                    let session = Arc::new(Session::over_with_state(link.clone(), state.clone())?);
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
            && (attempts.load(Ordering::Relaxed) < 2
                || state.status().generation < 2
                || !matches!(*health.lock().unwrap(), RuntimeHealth::Connected))
        {
            std::thread::sleep(Duration::from_millis(2));
        }
        reconnect.stopping.store(true, Ordering::Relaxed);
        worker.join().unwrap();

        assert_eq!(attempts.load(Ordering::Relaxed), 2);
        assert!(matches!(*health.lock().unwrap(), RuntimeHealth::Connected));
        assert_eq!(state.status().generation, 2);
        assert!(!active_lease.load(Ordering::Acquire));
        slot.lock().unwrap().close();
    }

    #[test]
    fn manual_retry_interrupts_reconnect_backoff() {
        let reconnect = Arc::new(ReconnectControl::default());
        let worker_reconnect = reconnect.clone();
        let started = Instant::now();
        let worker = std::thread::spawn(move || worker_reconnect.wait(Duration::from_secs(10)));

        std::thread::sleep(Duration::from_millis(20));
        reconnect.retry_now.store(true, Ordering::Release);

        assert!(worker.join().unwrap());
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "manual retry should interrupt rather than wait out backoff"
        );
    }

    #[test]
    fn shutdown_interrupts_reconnect_backoff() {
        let reconnect = Arc::new(ReconnectControl::default());
        let worker_reconnect = reconnect.clone();
        let started = Instant::now();
        let worker = std::thread::spawn(move || worker_reconnect.wait(Duration::from_secs(10)));

        std::thread::sleep(Duration::from_millis(20));
        reconnect.stopping.store(true, Ordering::Release);

        assert!(!worker.join().unwrap());
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "shutdown should interrupt rather than wait out reconnect backoff"
        );
    }

    #[test]
    fn untrusted_socket_rows_are_rejected_before_device_io() {
        let daemon = fake_daemon();
        let Response::Error { code, message } = daemon.handle(Request::SetBlock {
            row: 4,
            column: 0,
            model: 42,
            verify: false,
            timeout_seconds: 0,
        }) else {
            panic!("an out-of-range socket row must be rejected");
        };
        assert!(message.contains("0-3"), "unexpected error: {message}");
        assert_eq!(code, DaemonErrorCode::InvalidRow);
        daemon.shutdown();
    }

    #[test]
    fn invalid_parameter_values_are_rejected_before_device_io() {
        let daemon = fake_daemon();
        let Response::Error { code, message } = daemon.handle(Request::SetParam {
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
        assert_eq!(code, DaemonErrorCode::InvalidParameter);
        daemon.shutdown();
    }

    #[test]
    fn one_connection_serves_several_requests() {
        let daemon = fake_daemon();
        let (mut client, server) = LocalConnection::pair().expect("local IPC pair");
        let mut line = serde_json::to_string(&Request::Status).expect("encode");
        line.push('\n');
        client.write_all(line.as_bytes()).expect("first");
        client.write_all(line.as_bytes()).expect("second");
        client.shutdown_write().expect("half close");

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
    fn client_version_check_and_request_share_one_connection() {
        let daemon = fake_daemon();
        let (client, server) = LocalConnection::pair().expect("local IPC pair");
        std::thread::scope(|scope| {
            let worker = scope.spawn(|| serve(&daemon, server));
            let status = request_on(client, &Request::Status).expect("status request");
            assert_eq!(
                status["daemon_version"],
                DAEMON_PROTOCOL_VERSION.to_string()
            );
            assert_eq!(worker.join().unwrap(), Control::Continue);
        });
        daemon.shutdown();
    }

    #[test]
    fn a_blank_line_is_ignored_rather_than_answered() {
        let daemon = fake_daemon();
        let (mut client, server) = LocalConnection::pair().expect("local IPC pair");
        client.write_all(b"\n\n").expect("write");
        client.shutdown_write().expect("half close");

        serve(&daemon, server);
        let replies = BufReader::new(client).lines().count();
        assert_eq!(replies, 0, "blank lines are keepalive noise, not requests");
        daemon.shutdown();
    }
}
