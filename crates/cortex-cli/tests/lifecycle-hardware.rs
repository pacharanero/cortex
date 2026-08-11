// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

#![cfg(unix)]

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use cortex_host::DaemonClient;

#[test]
#[ignore = "requires a real Quad Cortex and exclusive access to its USB interface"]
fn auto_managed_daemon_releases_device_for_replacement_direct_read() {
    assert!(
        !DaemonClient::default().is_running(),
        "stop the existing cortex session before this lifecycle test"
    );
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_cortex"))
        .args([
            "session",
            "start",
            "--foreground",
            "--auto-managed",
            "--idle-timeout-seconds",
            "2",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("start auto-managed daemon");

    let ready_deadline = Instant::now() + Duration::from_secs(120);
    while Instant::now() < ready_deadline {
        if DaemonClient::default()
            .with_timeout(Duration::from_secs(1))
            .require_compatible()
            .is_ok()
        {
            break;
        }
        if let Some(status) = daemon.try_wait().unwrap() {
            panic!("auto-managed daemon exited before serving: {status}");
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(
        DaemonClient::default()
            .with_timeout(Duration::from_secs(1))
            .require_compatible()
            .is_ok(),
        "auto-managed daemon did not become ready"
    );

    let exit_deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = daemon.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= exit_deadline {
            daemon.kill().unwrap();
            panic!("auto-managed daemon did not exit after request inactivity");
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(status.success(), "auto-managed daemon exited {status}");
    assert!(!DaemonClient::default().is_running());

    let replacement = Command::new(env!("CARGO_BIN_EXE_cortex"))
        .args(["device", "version", "--format", "json"])
        .output()
        .expect("run replacement direct read");
    assert!(
        replacement.status.success(),
        "replacement direct read failed: {}",
        String::from_utf8_lossy(&replacement.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&replacement.stdout).unwrap();
    assert!(value.is_object(), "unexpected version output: {value}");
}
