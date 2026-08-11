// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
#[cfg(target_os = "linux")]
use std::os::unix::fs::PermissionsExt;
#[cfg(target_os = "linux")]
use std::path::Path;
use std::process::Stdio;
#[cfg(target_os = "linux")]
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use cortex_host::{
    CacheStatus, DaemonErrorCode, DaemonLifecycle, DeviceHealth, LocalConnection, LocalEndpoint,
    LocalListener, Request, Response, Status, serve_listener,
};
use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};

#[tokio::test]
async fn official_client_discovers_and_calls_the_real_server() -> anyhow::Result<()> {
    let runtime_dir = std::env::temp_dir().join(format!(
        "cortex-mcp-test-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    std::fs::create_dir(&runtime_dir)?;
    let socket = runtime_dir.join("cortex.sock");
    let endpoint = LocalEndpoint::at(socket);
    let listener = LocalListener::bind(&endpoint)?.listener;
    // Startup compatibility, then compatibility + request for each tool call.
    let daemon = std::thread::spawn(move || serve_daemon(listener, 7, false));

    let transport = TokioChildProcess::builder(
        tokio::process::Command::new(env!("CARGO_BIN_EXE_cortex-mcp")).configure(|command| {
            command.env("XDG_RUNTIME_DIR", &runtime_dir);
        }),
    )
    .stderr(Stdio::piped())
    .spawn()?
    .0;
    let client = ().serve(transport).await?;
    let tools = client.list_all_tools().await?;
    assert!(tools.iter().any(|tool| tool.name == "get_status"));
    assert!(tools.iter().any(|tool| tool.name == "set_scene_label"));
    assert!(!tools.iter().any(|tool| tool.name.contains("save")));
    for destructive in [
        "delete",
        "copy_preset",
        "create_setlist",
        "duplicate_setlist",
    ] {
        assert!(
            !tools.iter().any(|tool| tool.name.contains(destructive)),
            "MCP unexpectedly exposed destructive tool {destructive}"
        );
    }

    let result = client
        .call_tool(CallToolRequestParams::new("get_status"))
        .await?;
    assert_eq!(result.is_error, Some(false));
    assert_eq!(
        result
            .structured_content
            .as_ref()
            .and_then(|value| value.get("device"))
            .and_then(|value| value.get("state"))
            .and_then(serde_json::Value::as_str),
        Some("connected")
    );
    let result = client
        .call_tool(
            CallToolRequestParams::new("set_scene_label").with_arguments(
                serde_json::json!({"scene":2,"label":"Wide Lead"})
                    .as_object()
                    .expect("scene arguments are an object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(result.is_error, Some(false));
    let result = client
        .call_tool(
            CallToolRequestParams::new("set_block").with_arguments(
                serde_json::json!({"row":0,"column":0,"model":42})
                    .as_object()
                    .expect("block arguments are an object")
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(result.is_error, Some(true));
    assert_eq!(
        result
            .structured_content
            .as_ref()
            .and_then(|value| value.get("code"))
            .and_then(serde_json::Value::as_str),
        Some("dsp_refused")
    );
    assert_eq!(
        result
            .structured_content
            .as_ref()
            .and_then(|value| value.get("error"))
            .and_then(serde_json::Value::as_str),
        Some("fictional DSP capacity exhausted")
    );
    client.cancel().await?;
    daemon.join().expect("fake daemon thread");
    let _ = std::fs::remove_dir(runtime_dir);
    Ok(())
}

fn serve_daemon(listener: LocalListener, connections: usize, auto_managed: bool) {
    for _ in 0..connections {
        serve_connection(
            listener.accept().expect("accept fake daemon connection"),
            auto_managed,
        );
    }
    listener.cleanup_endpoint().expect("clean fake endpoint");
}

fn serve_connection(stream: LocalConnection, auto_managed: bool) {
    let mut writer = stream.try_clone().expect("clone fake daemon stream");
    for line in BufReader::new(stream).lines() {
        let request: Request = serde_json::from_str(&line.expect("read fake daemon request"))
            .expect("decode fake daemon request");
        let response = match request {
            Request::Status => Response::ok(&status(auto_managed)).expect("encode status"),
            Request::SetSceneLabel {
                scene: 2,
                label: Some(label),
            } if label == "Wide Lead" => Response::ok(&serde_json::json!({
                "scene": 2,
                "label": label,
            }))
            .expect("encode scene label response"),
            Request::SetBlock {
                row: 0,
                column: 0,
                model: 42,
                ..
            } => Response::coded_error(
                DaemonErrorCode::DspRefused,
                "fictional DSP capacity exhausted",
            ),
            other => Response::error(format!("unexpected request in process test: {other:?}")),
        };
        serde_json::to_writer(&mut writer, &response).expect("write fake daemon response");
        writer
            .write_all(b"\n")
            .expect("delimit fake daemon response");
        writer.flush().expect("flush fake daemon response");
    }
}

fn status(auto_managed: bool) -> Status {
    Status {
        daemon_version: cortex_host::DAEMON_PROTOCOL_VERSION.to_string(),
        uptime_seconds: 1,
        auto_managed,
        idle_timeout_seconds: auto_managed.then_some(60),
        device: DeviceHealth::Connected {
            serial: None,
            coros_version: Some("4.0.1".to_string()),
            last_message_seconds: 0,
        },
        cache: CacheStatus {
            phase: cortex_rs::CachePhase::Live,
            ..CacheStatus::default()
        },
    }
}

#[test]
#[ignore]
#[cfg(target_os = "linux")]
fn sibling_daemon_fixture() {
    let endpoint = LocalEndpoint::daemon();
    let listener = LocalListener::bind(&endpoint)
        .expect("claim fake sibling daemon endpoint")
        .listener;
    serve_listener(
        &listener,
        DaemonLifecycle::AutoManaged {
            idle_timeout: Duration::from_secs(1),
        },
        &|request| match request {
            Request::Status => Response::ok(&status(true)).expect("encode fixture status"),
            other => Response::error(format!("unexpected sibling fixture request: {other:?}")),
        },
    )
    .expect("serve fake sibling daemon");
    let cleanup_delay = std::env::var("CORTEX_FAKE_CLEANUP_DELAY_MS")
        .expect("fixture cleanup delay")
        .parse()
        .expect("fixture cleanup delay is numeric");
    std::thread::sleep(Duration::from_millis(cleanup_delay));
    listener
        .cleanup_endpoint()
        .expect("clean fake sibling endpoint");
    if let Some(claim) = std::env::var_os("CORTEX_START_CLAIM") {
        std::fs::remove_dir(claim).expect("clean fake starter claim");
    }
}

#[tokio::test]
#[cfg(target_os = "linux")]
async fn concurrent_missing_daemon_starts_converge_on_one_sibling() -> anyhow::Result<()> {
    let runtime_dir = std::env::temp_dir().join(format!(
        "cortex-mcp-start-test-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    std::fs::create_dir(&runtime_dir)?;
    let script = runtime_dir.join("cortex");
    let marker = runtime_dir.join("starts.txt");
    write_fake_sibling(&script)?;

    let first = mcp_transport(&runtime_dir, &script, &marker, 2, 0)?;
    let second = mcp_transport(&runtime_dir, &script, &marker, 2, 0)?;
    let (first, second) = tokio::try_join!(().serve(first), ().serve(second))?;

    let (first_status, second_status) = tokio::try_join!(
        first.call_tool(CallToolRequestParams::new("get_status")),
        second.call_tool(CallToolRequestParams::new("get_status")),
    )?;
    for result in [first_status, second_status] {
        assert_eq!(result.is_error, Some(false));
        assert_eq!(
            result
                .structured_content
                .as_ref()
                .and_then(|value| value.get("auto_managed"))
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
    }
    first.cancel().await?;
    second.cancel().await?;

    let starts = std::fs::read_to_string(&marker)?;
    assert_eq!(
        starts.lines().count(),
        2,
        "both MCP processes should race through the sibling start contract"
    );
    assert!(
        starts
            .lines()
            .all(|line| { line == "session start --auto-managed --idle-timeout-seconds 60" })
    );
    wait_for_endpoint_removal(&runtime_dir.join("cortex.sock")).await;
    let _ = std::fs::remove_dir_all(runtime_dir);
    Ok(())
}

#[tokio::test]
#[cfg(target_os = "linux")]
async fn a_long_lived_mcp_restarts_after_auto_managed_idle_exit() -> anyhow::Result<()> {
    let runtime_dir = std::env::temp_dir().join(format!(
        "cortex-mcp-restart-test-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    std::fs::create_dir(&runtime_dir)?;
    let script = runtime_dir.join("cortex");
    let marker = runtime_dir.join("starts.txt");
    let endpoint = runtime_dir.join("cortex.sock");
    write_fake_sibling(&script)?;

    let transport = mcp_transport(&runtime_dir, &script, &marker, 1, 500)?;
    let client = ().serve(transport).await?;
    let first = client
        .call_tool(CallToolRequestParams::new("get_status"))
        .await?;
    assert_eq!(first.is_error, Some(false));
    // Enter the interval after idle serving stops but before the old owner
    // releases its claim. The next tool must wait for release, then start.
    tokio::time::sleep(Duration::from_millis(1_100)).await;

    let second = client
        .call_tool(CallToolRequestParams::new("get_status"))
        .await?;
    assert_eq!(second.is_error, Some(false));
    client.cancel().await?;
    wait_for_endpoint_removal(&endpoint).await;

    let starts = std::fs::read_to_string(&marker)?;
    assert_eq!(starts.lines().count(), 2);
    let _ = std::fs::remove_dir_all(runtime_dir);
    Ok(())
}

#[cfg(target_os = "linux")]
fn mcp_transport(
    runtime_dir: &Path,
    sibling: &Path,
    marker: &Path,
    expected_starters: usize,
    cleanup_delay_ms: u64,
) -> anyhow::Result<TokioChildProcess> {
    Ok(TokioChildProcess::builder(
        tokio::process::Command::new(env!("CARGO_BIN_EXE_cortex-mcp")).configure(|command| {
            command
                .env("XDG_RUNTIME_DIR", runtime_dir)
                .env("CORTEX_CLI_PATH", sibling)
                .env("CORTEX_START_MARKER", marker)
                .env("CORTEX_EXPECTED_STARTERS", expected_starters.to_string())
                .env("CORTEX_FAKE_CLEANUP_DELAY_MS", cleanup_delay_ms.to_string())
                .env(
                    "CORTEX_TEST_EXE",
                    std::env::current_exe().expect("test executable"),
                );
        }),
    )
    .stderr(Stdio::null())
    .spawn()?
    .0)
}

#[cfg(target_os = "linux")]
fn write_fake_sibling(path: &Path) -> anyhow::Result<()> {
    std::fs::write(
        path,
        r#"#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "$CORTEX_START_MARKER"
for _ in $(seq 1 100); do
  [[ $(wc -l < "$CORTEX_START_MARKER") -ge $CORTEX_EXPECTED_STARTERS ]] && break
  sleep 0.01
done
claim="$CORTEX_START_MARKER.claim"
if ! mkdir "$claim" 2>/dev/null; then
  for _ in $(seq 1 200); do
    [[ -S "$XDG_RUNTIME_DIR/cortex.sock" ]] && exit 1
    sleep 0.01
  done
  exit 1
fi
CORTEX_START_CLAIM="$claim" setsid -f \
  "$CORTEX_TEST_EXE" --ignored --exact sibling_daemon_fixture --nocapture \
  >/dev/null 2>&1
"#,
    )?;
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(target_os = "linux")]
async fn wait_for_endpoint_removal(path: &Path) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while path.exists() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        !path.exists(),
        "fake sibling daemon did not clean its endpoint"
    );
}

#[tokio::test]
#[ignore = "requires a real Quad Cortex, a held session, and disposable live-grid state"]
async fn hardware_smoke_builds_and_restores_a_live_grid() -> anyhow::Result<()> {
    let transport = TokioChildProcess::new(tokio::process::Command::new(env!(
        "CARGO_BIN_EXE_cortex-mcp"
    )))?;
    let client = ().serve(transport).await?;
    let setlist = cortex_rs::client::USER_SETLIST;

    let run = async {
        call(&client, "get_status", serde_json::json!({})).await?;
        call(&client, "get_device_version", serde_json::json!({})).await?;
        call(&client, "get_cpu_load", serde_json::json!({})).await?;
        call(
            &client,
            "search_catalog",
            serde_json::json!({"query":"Brit 2203"}),
        )
        .await?;
        call(
            &client,
            "recall_preset",
            serde_json::json!({"setlist":setlist,"slot":"7A"}),
        )
        .await?;
        call(
            &client,
            "set_block",
            serde_json::json!({"row":0,"column":0,"model":1001,"verify":true}),
        )
        .await?;
        call(
            &client,
            "set_param",
            serde_json::json!({"row":0,"column":0,"target":{"by":"name","value":"GAIN"},"input":{"kind":"real","value":5.0}}),
        )
        .await?;
        call(
            &client,
            "set_bypass",
            serde_json::json!({"row":0,"column":0,"bypass":true}),
        )
        .await?;
        call(
            &client,
            "set_bypass",
            serde_json::json!({"row":0,"column":0,"bypass":false}),
        )
        .await?;
        call(
            &client,
            "set_chain_input",
            serde_json::json!({"row":0,"port":1}),
        )
        .await?;
        call(
            &client,
            "set_chain_output",
            serde_json::json!({"row":0,"port":19}),
        )
        .await?;
        call(
            &client,
            "set_split",
            serde_json::json!({"row":0,"split":3,"mix":-1}),
        )
        .await?;
        let grid = call(
            &client,
            "read_current_preset",
            serde_json::json!({"with_params":true}),
        )
        .await?;
        let blocks = grid
            .structured_content
            .as_ref()
            .and_then(|value| value.get("blocks"))
            .and_then(serde_json::Value::as_array)
            .context("hardware smoke grid has no blocks array")?;
        anyhow::ensure!(
            blocks.iter().any(|block| {
                block.get("row").and_then(serde_json::Value::as_u64) == Some(0)
                    && block.get("column").and_then(serde_json::Value::as_u64) == Some(0)
                    && block.get("model_id").and_then(serde_json::Value::as_u64) == Some(1001)
            }),
            "MCP-edited block was absent from live-grid read-back"
        );
        call(
            &client,
            "remove_block",
            serde_json::json!({"row":0,"column":0}),
        )
        .await?;
        anyhow::Ok(())
    }
    .await;

    let restore = call(
        &client,
        "recall_preset",
        serde_json::json!({"setlist":setlist,"slot":"1A"}),
    )
    .await;
    client.cancel().await?;
    restore?;
    run
}

async fn call(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &str,
    arguments: serde_json::Value,
) -> anyhow::Result<rmcp::model::CallToolResult> {
    let arguments = arguments
        .as_object()
        .cloned()
        .context("tool arguments must be an object")?;
    let result = client
        .call_tool(CallToolRequestParams::new(name.to_string()).with_arguments(arguments))
        .await?;
    anyhow::ensure!(
        result.is_error != Some(true),
        "{name} failed: {:?}",
        result.structured_content
    );
    Ok(result)
}
