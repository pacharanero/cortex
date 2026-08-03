// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! `cortex connect`: one held session, served over a unix socket.
//!
//! The device grants its HID interface exclusively, so exactly one process
//! can own it. This is that process. Everything else talks to it.
//!
//! @see spec/roadmap.md PROT-008.6
//! @see spec/140-session/spec.md

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;
use cortex_rs::daemon::{CacheStatus, DeviceHealth, Request, Response, Status, socket_path};
use cortex_rs::{DeviceKind, QuadCortex, Session};

/// How long a request may take before the daemon gives up on the device.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// How long teardown gets before the process exits regardless.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(3);

/// State the daemon holds for the life of its session.
struct Daemon {
    session: Arc<Session>,
    client: QuadCortex,
    started: Instant,
    /// Device pushes seen since connecting. Surfaced by `status`, because a
    /// cache kept current by pushes is only as trustworthy as the stream
    /// feeding it, and a stalled stream should be visible.
    pushes: AtomicU64,
}

impl Daemon {
    /// Open the device and perform the SUBSCRIBED handshake.
    ///
    /// Subscribed, not minimal, and that is the whole point: subscribing is
    /// how the device reports edits made by the PLAYER on the hardware. It
    /// is expensive per command and correct once per session.
    fn connect() -> Result<Self> {
        eprintln!("cortex connect: opening device ...");
        let session = Arc::new(Session::open(DeviceKind::QuadCortex)?);

        let started = Instant::now();
        session.connect_with_progress(
            cortex_rs::ConnectMode::Subscribed,
            Duration::from_secs(15),
            Duration::from_secs(2),
            |step| eprintln!("cortex connect:   {step} ..."),
        )?;
        eprintln!(
            "cortex connect: connected in {:.1}s",
            started.elapsed().as_secs_f32()
        );

        let client = QuadCortex::new(session.clone());
        Ok(Self {
            session,
            client,
            started: Instant::now(),
            pushes: AtomicU64::new(0),
        })
    }

    /// Handle one request.
    ///
    /// Every arm returns a `Response` rather than propagating, because a
    /// failed request must not take the daemon down with it - the session is
    /// shared by every other client.
    fn handle(&self, request: Request) -> Response {
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
            Request::Version => match self.session.device_version() {
                Some(v) => match serde_json::to_value(crate::device_version(&v)) {
                    Ok(value) => Response::Ok { data: value },
                    Err(e) => Response::error(format!("DeviceVersion: {e}")),
                },
                None => self.respond(|c| {
                    c.version(REQUEST_TIMEOUT).and_then(|v| {
                        serde_json::to_value(crate::device_version(&v))
                            .map_err(|e| cortex_rs::Error::Decode(format!("DeviceVersion: {e}")))
                    })
                }),
            },
            Request::ActiveScene => {
                self.respond(|c| c.active_scene(REQUEST_TIMEOUT).map(serde_json::Value::from))
            }
            Request::SwitchScene { scene } => self.respond(|c| {
                c.switch_scene(scene)
                    .map(|()| serde_json::json!({ "scene": scene }))
            }),
            Request::CurrentPreset => self.respond(|c| {
                c.read_current_preset(REQUEST_TIMEOUT).map(|p| {
                    serde_json::json!({
                        "name": preset_name(&p),
                        "chains": p.chains.len(),
                    })
                })
            }),
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
            } => self.respond(move |c| {
                c.list_presets(&setlist, REQUEST_TIMEOUT, include_empty)
                    .map(|entries| {
                        serde_json::json!(
                            entries
                                .iter()
                                .map(|e| serde_json::json!({
                                    "index": e.index,
                                    "slot": cortex_rs::client::position_to_slot(e.index),
                                    "name": e.name,
                                }))
                                .collect::<Vec<_>>()
                        )
                    })
            }),
            // Return the raw payload, not a summary. The caller parses it
            // with the same code the direct path uses, so the two cannot
            // render different catalogs.
            //
            // Served from the handshake's own copy: the device builds this
            // on request, and asking twice costs another 46 KB transfer.
            Request::Catalog => {
                let cached = self.session.captured_model_repo();
                match cached {
                    Some(payload) => match serde_json::to_value(payload) {
                        Ok(v) => Response::Ok {
                            data: serde_json::json!({ "payload": v }),
                        },
                        Err(e) => Response::error(format!("catalog payload: {e}")),
                    },
                    None => self.respond(|c| {
                        c.fetch_model_repo(REQUEST_TIMEOUT)
                            .map(|payload| serde_json::json!({ "payload": payload }))
                    }),
                }
            }
            Request::CpuLoad => match self.session.cpu_load() {
                Some(load) => match serde_json::to_value(crate::cpu_load(&load)) {
                    Ok(value) => Response::Ok { data: value },
                    Err(e) => Response::error(format!("CpuLoad: {e}")),
                },
                None => Response::error(
                    "no CPU load received yet - the device pushes it about once a second \
                     after subscribing"
                        .to_string(),
                ),
            },
            // Handled by the caller, which needs to stop the accept loop.
            Request::Shutdown => Response::ok(&serde_json::json!({ "stopping": true }))
                .unwrap_or_else(|e| Response::error(format!("{e}"))),
        }
    }

    /// Run a client call, turning any error into a `Response` rather than
    /// letting it escape.
    fn respond<F>(&self, call: F) -> Response
    where
        F: FnOnce(&QuadCortex) -> cortex_rs::Result<serde_json::Value>,
    {
        match call(&self.client) {
            Ok(value) => Response::Ok { data: value },
            Err(e) => Response::error(e.to_string()),
        }
    }

    fn status(&self) -> Status {
        // From the handshake's Version READ. Absent only if the device did
        // not answer it, which is not fatal - the session still works.
        let identity = self
            .session
            .device_version()
            .map(|v| crate::device_version(&v));
        Status {
            daemon_version: env!("CARGO_PKG_VERSION").to_string(),
            uptime_seconds: self.started.elapsed().as_secs(),
            // Reported raw. A verdict is now defensible - a healthy idle
            // session reads 0 - but nothing acts on it yet; see roadmap
            // PROT-008.6.4.
            device: DeviceHealth::Connected {
                serial: identity.as_ref().and_then(|d| d.serial_number.clone()),
                coros_version: identity.as_ref().and_then(|d| d.coros_version.clone()),
                last_message_seconds: self.session.seconds_since_last_message(),
            },
            cache: CacheStatus {
                catalog: self.session.captured_model_repo().is_some(),
                current_preset: false,
                listed_setlists: Vec::new(),
                pushes_applied: self.pushes.load(Ordering::Relaxed),
            },
        }
    }
}

/// The preset's name, or a placeholder.
fn preset_name(preset: &cortex_rs::proto::BinaryPreset) -> String {
    preset
        .name
        .as_ref()
        .map_or("<unnamed>", |n| {
            let cortex_rs::proto::binary_preset::Name::Name(v) = n;
            v.as_str()
        })
        .to_string()
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
                 `cortex connect --stop`, or use the existing one.",
                path.display()
            );
        }
        eprintln!(
            "cortex connect: removing a stale socket at {} (nothing was listening)",
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

    eprintln!("cortex connect: listening on {}", path.display());
    eprintln!("cortex connect: press Ctrl-C to stop");

    for stream in listener.incoming() {
        let stream = match stream {
            Ok(s) => s,
            Err(e) => {
                eprintln!("cortex connect: accept failed: {e}");
                continue;
            }
        };
        if serve(&daemon, stream) == Control::Stop {
            break;
        }
    }

    eprintln!("cortex connect: shutting down");

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
            "cortex connect: teardown did not finish within {}s; exiting anyway",
            SHUTDOWN_GRACE.as_secs()
        );
        std::process::exit(0);
    });

    daemon.client.disconnect();
    daemon.session.stop();
    // Last, so the socket never outlives our claim on the device: while it
    // answers, other commands correctly refuse to open the device for
    // themselves. A file left behind by a forced exit is inert - nothing is
    // listening on it - and the next start clears it as stale.
    let _ = std::fs::remove_file(&path);
    Ok(())
}

/// Whether the accept loop should continue.
#[derive(PartialEq, Eq)]
enum Control {
    Continue,
    Stop,
}

/// Serve one connection. A client may send several requests on one stream.
fn serve(daemon: &Daemon, stream: UnixStream) -> Control {
    let peer = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cortex connect: could not clone stream: {e}");
            return Control::Continue;
        }
    };
    let reader = BufReader::new(peer);
    let mut writer = stream;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("cortex connect: read failed: {e}");
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

/// Send one request to a running daemon, if there is one.
///
/// Returns `None` when no daemon is listening, so the caller can fall back to
/// a direct session rather than failing.
pub fn request(request: &Request) -> Option<Result<serde_json::Value>> {
    let path = socket_path();
    let stream = UnixStream::connect(&path).ok()?;

    Some((|| {
        let mut writer = stream.try_clone()?;
        let mut line = serde_json::to_string(request)?;
        line.push('\n');
        writer.write_all(line.as_bytes())?;
        writer.flush()?;

        let mut reply = String::new();
        BufReader::new(stream).read_line(&mut reply)?;
        match serde_json::from_str::<Response>(&reply)? {
            Response::Ok { data } => Ok(data),
            Response::Error { message } => anyhow::bail!("{message}"),
        }
    })())
}
