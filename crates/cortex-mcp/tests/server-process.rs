// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use cortex_host::{
    CacheStatus, DeviceHealth, LocalConnection, LocalEndpoint, LocalListener, Request, Response,
    Status,
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
    let daemon = std::thread::spawn(move || serve_daemon(listener, 3));

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
    client.cancel().await?;
    daemon.join().expect("fake daemon thread");
    let _ = std::fs::remove_dir(runtime_dir);
    Ok(())
}

fn serve_daemon(listener: LocalListener, connections: usize) {
    for _ in 0..connections {
        serve_connection(listener.accept().expect("accept fake daemon connection"));
    }
    listener.cleanup_endpoint().expect("clean fake endpoint");
}

fn serve_connection(stream: LocalConnection) {
    let mut writer = stream.try_clone().expect("clone fake daemon stream");
    for line in BufReader::new(stream).lines() {
        let request: Request = serde_json::from_str(&line.expect("read fake daemon request"))
            .expect("decode fake daemon request");
        let response = match request {
            Request::Status => Response::ok(&status()).expect("encode status"),
            Request::SetSceneLabel {
                scene: 2,
                label: Some(label),
            } if label == "Wide Lead" => Response::ok(&serde_json::json!({
                "scene": 2,
                "label": label,
            }))
            .expect("encode scene label response"),
            other => Response::error(format!("unexpected request in process test: {other:?}")),
        };
        serde_json::to_writer(&mut writer, &response).expect("write fake daemon response");
        writer
            .write_all(b"\n")
            .expect("delimit fake daemon response");
        writer.flush().expect("flush fake daemon response");
    }
}

fn status() -> Status {
    Status {
        daemon_version: cortex_host::DAEMON_PROTOCOL_VERSION.to_string(),
        uptime_seconds: 1,
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
