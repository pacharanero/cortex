// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Process tests for the auto-managed idle-exit contract and explicit
//! persistent daemons: idle timeout, timeout reset on completion,
//! in-flight protection, and typed failures surviving the process boundary.
//!
//! @see spec/200-cli/spec.md [FR-18] [FR-26]

#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cortex_host::{
    CacheStatus, DAEMON_PROTOCOL_VERSION, DaemonClient, DaemonError, DaemonErrorCode,
    DaemonLifecycle, DeviceHealth, LocalEndpoint, LocalListener, Request, Response, Status,
    serve_listener,
};

const CHILD_ENV: &str = "CORTEX_LIFECYCLE_CHILD";
const MODE_ENV: &str = "CORTEX_LIFECYCLE_MODE";
const SOCKET_ENV: &str = "CORTEX_LIFECYCLE_SOCKET";
const TIMEOUT_ENV: &str = "CORTEX_LIFECYCLE_TIMEOUT_MS";

fn endpoint(label: &str) -> LocalEndpoint {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    LocalEndpoint::at(std::env::temp_dir().join(format!(
        "cortex-lifecycle-{label}-{}-{unique}.sock",
        std::process::id()
    )))
}

fn spawn_daemon(label: &str, lifecycle: DaemonLifecycle) -> (Child, LocalEndpoint) {
    let endpoint = endpoint(label);
    let (mode, timeout_ms) = match lifecycle {
        DaemonLifecycle::Explicit => ("explicit", 0),
        DaemonLifecycle::AutoManaged { idle_timeout } => ("auto", idle_timeout.as_millis() as u64),
    };
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "lifecycle_daemon_fixture"])
        .env(CHILD_ENV, "1")
        .env(MODE_ENV, mode)
        .env(TIMEOUT_ENV, timeout_ms.to_string())
        .env(SOCKET_ENV, endpoint.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && !DaemonClient::new(endpoint.clone()).is_running() {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        DaemonClient::new(endpoint.clone()).is_running(),
        "fixture daemon did not claim its endpoint"
    );
    (child, endpoint)
}

fn wait_for_exit(mut child: Child, endpoint: &LocalEndpoint) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "fixture daemon exited {status}");
            assert!(
                !DaemonClient::new(endpoint.clone()).is_running(),
                "endpoint still appears owned after process exit"
            );
            let lock = PathBuf::from(endpoint.to_string()).with_extension("lock");
            let _ = std::fs::remove_file(lock);
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    child.kill().unwrap();
    panic!("fixture daemon did not exit");
}

#[test]
fn auto_managed_daemon_exits_idle_and_cleans_its_endpoint() {
    let (child, endpoint) = spawn_daemon(
        "idle",
        DaemonLifecycle::AutoManaged {
            idle_timeout: Duration::from_millis(150),
        },
    );
    wait_for_exit(child, &endpoint);
}

#[test]
fn every_completed_request_restarts_the_idle_timeout() {
    let timeout = Duration::from_millis(250);
    let (mut child, endpoint) = spawn_daemon(
        "reset",
        DaemonLifecycle::AutoManaged {
            idle_timeout: timeout,
        },
    );
    std::thread::sleep(Duration::from_millis(150));
    DaemonClient::new(endpoint.clone())
        .require_compatible()
        .unwrap();
    std::thread::sleep(Duration::from_millis(150));
    assert!(
        child.try_wait().unwrap().is_none(),
        "request did not reset idle"
    );
    wait_for_exit(child, &endpoint);
}

#[test]
fn a_connection_accepted_at_the_idle_boundary_can_deliver_its_request() {
    let timeout = Duration::from_millis(250);
    let (mut child, endpoint) = spawn_daemon(
        "accepted-boundary",
        DaemonLifecycle::AutoManaged {
            idle_timeout: timeout,
        },
    );
    std::thread::sleep(Duration::from_millis(210));
    let mut connection = cortex_host::LocalConnection::connect(&endpoint).unwrap();
    connection
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    std::thread::sleep(Duration::from_millis(60));
    serde_json::to_writer(&mut connection, &Request::Status).unwrap();
    connection.write_all(b"\n").unwrap();
    connection.flush().unwrap();

    let mut response = String::new();
    BufReader::new(connection).read_line(&mut response).unwrap();
    assert!(
        matches!(serde_json::from_str(&response), Ok(Response::Ok { .. })),
        "accepted request was dropped at idle boundary: {response:?}"
    );
    std::thread::sleep(Duration::from_millis(150));
    assert!(
        child.try_wait().unwrap().is_none(),
        "accepted request did not restart the full idle timeout"
    );
    wait_for_exit(child, &endpoint);
}

#[test]
fn in_flight_request_protects_idle_exit_and_other_clients_are_not_blocked() {
    let (child, endpoint) = spawn_daemon(
        "concurrent",
        DaemonLifecycle::AutoManaged {
            idle_timeout: Duration::from_millis(150),
        },
    );
    let slow_endpoint = endpoint.clone();
    let slow = std::thread::spawn(move || {
        DaemonClient::new(slow_endpoint)
            .request_value(&Request::ReconnectNow)
            .unwrap();
    });
    std::thread::sleep(Duration::from_millis(75));

    let started = Instant::now();
    DaemonClient::new(endpoint.clone())
        .require_compatible()
        .unwrap();
    assert!(
        started.elapsed() < Duration::from_millis(150),
        "a slow client monopolised the accept loop"
    );
    slow.join().unwrap();
    wait_for_exit(child, &endpoint);
}

#[test]
fn explicitly_started_daemon_does_not_gain_an_idle_timeout() {
    let (mut child, endpoint) = spawn_daemon("explicit", DaemonLifecycle::Explicit);
    std::thread::sleep(Duration::from_millis(350));
    assert!(
        child.try_wait().unwrap().is_none(),
        "explicit daemon exited idle"
    );
    DaemonClient::new(endpoint.clone())
        .request_value(&Request::Shutdown)
        .unwrap();
    wait_for_exit(child, &endpoint);
}

#[test]
fn typed_daemon_failure_survives_the_process_boundary() {
    let (child, endpoint) = spawn_daemon("typed-error", DaemonLifecycle::Explicit);
    let error = DaemonClient::new(endpoint.clone())
        .request_value(&Request::ActiveScene)
        .expect_err("fixture active-scene request must fail");
    let daemon = error
        .downcast_ref::<DaemonError>()
        .expect("typed daemon error was erased");
    assert_eq!(daemon.code, DaemonErrorCode::InvalidRow);
    assert_eq!(daemon.message, "fictional invalid row");

    DaemonClient::new(endpoint.clone())
        .request_value(&Request::Shutdown)
        .unwrap();
    wait_for_exit(child, &endpoint);
}

#[test]
#[ignore]
fn lifecycle_daemon_fixture() {
    if std::env::var_os(CHILD_ENV).is_none() {
        return;
    }
    let endpoint = LocalEndpoint::at(PathBuf::from(std::env::var_os(SOCKET_ENV).unwrap()));
    let lifecycle = if std::env::var(MODE_ENV).unwrap() == "explicit" {
        DaemonLifecycle::Explicit
    } else {
        DaemonLifecycle::AutoManaged {
            idle_timeout: Duration::from_millis(
                std::env::var(TIMEOUT_ENV).unwrap().parse().unwrap(),
            ),
        }
    };
    let bound = LocalListener::bind(&endpoint).unwrap();
    let handler = |request| {
        if matches!(request, Request::ReconnectNow) {
            std::thread::sleep(Duration::from_millis(400));
        }
        match request {
            Request::Status => Response::ok(&fixture_status(lifecycle)).unwrap(),
            Request::ActiveScene => {
                Response::coded_error(DaemonErrorCode::InvalidRow, "fictional invalid row")
            }
            _ => Response::ok(&true).unwrap(),
        }
    };
    serve_listener(&bound.listener, lifecycle, &handler).unwrap();
    bound.listener.cleanup_endpoint().unwrap();
}

fn fixture_status(lifecycle: DaemonLifecycle) -> Status {
    Status {
        daemon_version: DAEMON_PROTOCOL_VERSION.to_string(),
        uptime_seconds: 0,
        auto_managed: lifecycle.is_auto_managed(),
        idle_timeout_seconds: lifecycle.idle_timeout().map(|timeout| timeout.as_secs()),
        device_kind: cortex_rs::DeviceKind::QuadCortex,
        device: DeviceHealth::Failed {
            error: "fictional fixture".into(),
        },
        cache: CacheStatus::default(),
    }
}
