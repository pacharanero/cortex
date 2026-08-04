// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! `cortex session start`: one held session, served over a unix socket.
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

        Ok(Self::over(session))
    }

    /// Build a daemon around an already-connected session.
    ///
    /// Separated from [`Self::connect`] so the request handling can be
    /// exercised without a device: everything interesting here - the socket
    /// protocol, the status shape, how a failed call is reported - is
    /// independent of how the session was obtained.
    fn over(session: Arc<Session>) -> Self {
        let client = QuadCortex::new(session.clone());
        Self {
            session,
            client,
            started: Instant::now(),
            pushes: AtomicU64::new(0),
        }
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
            // Rows arrive as plain wire indices and are rebuilt here, so
            // the newtype's guard against the wire/screen mix-up stays real.
            Request::SetParam {
                row,
                column,
                param_index,
                value,
                scene,
                promote,
            } => self.respond(move |c| {
                let row = cortex_rs::Row::from_wire(row);
                match scene {
                    Some(scene) => {
                        c.set_param_in_scene(row, column, param_index, value, scene, promote)
                    }
                    None => {
                        if promote {
                            c.set_param_scene_mode(row, column, param_index, true)?;
                        }
                        c.set_param(row, column, param_index, value)
                    }
                }
                .map(|()| serde_json::json!({ "applied": true }))
            }),
            Request::SetBypass {
                row,
                column,
                bypass,
            } => self.respond(move |c| {
                c.set_bypass(cortex_rs::Row::from_wire(row), column, bypass)
                    .map(|()| serde_json::json!({ "applied": true }))
            }),
            Request::RemoveBlock { row, column } => self.respond(move |c| {
                c.remove_block(cortex_rs::Row::from_wire(row), column)
                    .map(|()| serde_json::json!({ "applied": true }))
            }),
            Request::SetSplit { row, split, mix } => self.respond(move |c| {
                c.set_split(cortex_rs::Row::from_wire(row), split, mix)
                    .map(|()| serde_json::json!({ "applied": true }))
            }),
            Request::SetRouting { row, input, output } => self.respond(move |c| {
                let row = cortex_rs::Row::from_wire(row);
                match (input, output) {
                    (Some(port), None) => c.set_chain_input(row, port),
                    (None, Some(port)) => c.set_chain_output(row, port),
                    _ => Err(cortex_rs::Error::NotFound(
                        "set-routing needs exactly one of input or output".into(),
                    )),
                }
                .map(|()| serde_json::json!({ "applied": true }))
            }),
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

    fn fake_daemon() -> Daemon {
        let link = FakeLink::new();
        let device: Arc<std::sync::Mutex<dyn HidLink>> = Arc::new(std::sync::Mutex::new(link));
        Daemon::over(Arc::new(
            Session::over(device).expect("session over a fake link"),
        ))
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
        daemon.session.stop();
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
        daemon.session.stop();
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
        daemon.session.stop();
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
        daemon.session.stop();
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
        daemon.session.stop();
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
        daemon.session.stop();
    }
}
