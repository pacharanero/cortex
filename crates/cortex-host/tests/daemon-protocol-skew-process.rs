// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Process tests proving client-side protocol gating rejects ordinary work
//! against older and newer daemons while cross-version `Shutdown` remains
//! compatible.
//!
//! @see spec/200-cli/spec.md [FR-28]
//! @see spec/200-cli/design.md [DES-CLI]

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use cortex_host::{
    CacheStatus, DAEMON_PROTOCOL_VERSION, DaemonClient, DeviceHealth, LocalEndpoint, LocalListener,
    Request, Response, Status,
};

const CHILD_ENV: &str = "CORTEX_PROTOCOL_SKEW_CHILD";
const VERSION_ENV: &str = "CORTEX_PROTOCOL_SKEW_VERSION";
const SOCKET_ENV: &str = "CORTEX_PROTOCOL_SKEW_SOCKET";

fn endpoint(label: &str) -> LocalEndpoint {
    LocalEndpoint::at(std::env::temp_dir().join(format!(
        "cortex-protocol-skew-{label}-{}-{}.sock",
        std::process::id(),
        DAEMON_PROTOCOL_VERSION
    )))
}

fn spawn_daemon(label: &str, version: u32) -> (Child, LocalEndpoint) {
    let endpoint = endpoint(label);
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "protocol_skew_daemon_fixture"])
        .env(CHILD_ENV, "1")
        .env(VERSION_ENV, version.to_string())
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
        "fixture daemon did not start"
    );
    (child, endpoint)
}

fn wait(mut child: Child, endpoint: &LocalEndpoint) {
    #[cfg(windows)]
    let _ = endpoint;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "fixture daemon exited {status}");
            #[cfg(unix)]
            std::fs::remove_file(PathBuf::from(endpoint.to_string()).with_extension("lock"))
                .unwrap();
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    child.kill().unwrap();
    panic!("fixture daemon did not exit");
}

#[test]
fn spawned_older_daemon_is_refused_but_shutdown_remains_compatible() {
    let (child, endpoint) = spawn_daemon("older", DAEMON_PROTOCOL_VERSION - 1);
    let client = DaemonClient::new(endpoint.clone());
    let error = client
        .request_value(&Request::Status)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("client expects"),
        "unexpected error: {error}"
    );
    assert!(
        error.contains("daemon reports"),
        "unexpected error: {error}"
    );
    assert!(
        error.contains(&endpoint.to_string()),
        "unexpected error: {error}"
    );
    assert!(!error.contains("session stop"), "unexpected error: {error}");
    client.request_value(&Request::Shutdown).unwrap();
    wait(child, &endpoint);
}

#[test]
fn spawned_newer_daemon_is_refused_but_shutdown_remains_compatible() {
    let (child, endpoint) = spawn_daemon("newer", DAEMON_PROTOCOL_VERSION + 1);
    let client = DaemonClient::new(endpoint.clone());
    let error = client
        .request_value(&Request::Status)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("client expects"),
        "unexpected error: {error}"
    );
    assert!(
        error.contains("daemon reports"),
        "unexpected error: {error}"
    );
    assert!(
        error.contains(&endpoint.to_string()),
        "unexpected error: {error}"
    );
    assert!(!error.contains("session stop"), "unexpected error: {error}");
    client.request_value(&Request::Shutdown).unwrap();
    wait(child, &endpoint);
}

#[test]
#[ignore]
fn protocol_skew_daemon_fixture() {
    if std::env::var_os(CHILD_ENV).is_none() {
        return;
    }
    let version = std::env::var(VERSION_ENV).unwrap().parse::<u32>().unwrap();
    let endpoint = LocalEndpoint::at(PathBuf::from(std::env::var_os(SOCKET_ENV).unwrap()));
    let bound = LocalListener::bind(&endpoint).unwrap();
    loop {
        let mut connection = bound.listener.accept().unwrap();
        let mut reader = BufReader::new(connection.try_clone().unwrap());
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap() == 0 {
            continue;
        }
        let request: Request = serde_json::from_str(&line).unwrap();
        assert!(matches!(request, Request::Status));
        let data = if version < DAEMON_PROTOCOL_VERSION {
            // A real older daemon does not know lifecycle status fields. The
            // new client must still decode enough to report protocol skew.
            serde_json::json!({
                "daemon_version": version.to_string(),
                "uptime_seconds": 0,
                "device": { "state": "failed", "error": "fictional fixture" },
                "cache": CacheStatus::default(),
            })
        } else {
            serde_json::to_value(Status {
                daemon_version: version.to_string(),
                uptime_seconds: 0,
                auto_managed: false,
                idle_timeout_seconds: None,
                device_kind: cortex_rs::DeviceKind::QuadCortex,
                device: DeviceHealth::Failed {
                    error: "fictional fixture".into(),
                },
                cache: CacheStatus::default(),
            })
            .unwrap()
        };
        serde_json::to_writer(&mut connection, &Response::Ok { data }).unwrap();
        connection.write_all(b"\n").unwrap();
        connection.flush().unwrap();

        line.clear();
        if reader.read_line(&mut line).unwrap() == 0 {
            continue;
        }
        let request: Request = serde_json::from_str(&line).unwrap();
        assert!(matches!(request, Request::Shutdown));
        serde_json::to_writer(
            &mut connection,
            &Response::ok(&"stopping".to_string()).unwrap(),
        )
        .unwrap();
        connection.write_all(b"\n").unwrap();
        connection.flush().unwrap();
        bound.listener.cleanup_endpoint().unwrap();
        return;
    }
}
