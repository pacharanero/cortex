// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! MCP tool registry and daemon request adapter.
//!
//! Enforces the safety surface (explicit confirmation, factory refusal,
//! pre-edit target preparation), reuses the held daemon as the sole HID
//! owner, and wraps each `QuadCortex` method as one tiered tool.
//!
//! @see spec/300-mcp/spec.md [FR-1] [FR-2] [FR-4] [FR-40] [FR-41]
//! @see spec/300-mcp/design.md [DES-SAFETY] [DES-OWNER] [DES-TOOLS]

use std::borrow::Cow;
use std::io::{Read as _, Write as _};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use cortex_host::{
    DAEMON_PROTOCOL_VERSION, DaemonClient, DaemonError, DaemonErrorCode, Request, Status,
};
use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResponse, CallToolResult, DiscoverResult,
    Implementation, JsonObject, ListToolsResult, ProtocolVersion, ServerCapabilities, ServerInfo,
    Tool, ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler};
use serde_json::{Value, json};

use crate::transport::BoundedStdioTransport;

const CATALOG_TTL_MS: u64 = 60_000;
const AUTO_MANAGED_IDLE: Duration = Duration::from_secs(60);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(120);
const STARTUP_POLL: Duration = Duration::from_millis(200);

struct DaemonSupervisor {
    client: Arc<DaemonClient>,
    probe: DaemonClient,
    startup: Mutex<()>,
}

impl DaemonSupervisor {
    fn new() -> Self {
        Self {
            client: Arc::new(DaemonClient::default()),
            probe: DaemonClient::default().with_timeout(Duration::from_secs(2)),
            startup: Mutex::new(()),
        }
    }

    fn ensure(&self) -> Result<()> {
        let _startup = self
            .startup
            .lock()
            .map_err(|_| anyhow::anyhow!("daemon startup gate is unavailable"))?;
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        while Instant::now() < deadline {
            if let Some(status) = self.ready_status() {
                ensure_status_compatible(&status)?;
                return Ok(());
            }
            if !self.probe.is_running() {
                self.start_sibling(deadline)?;
            }
            std::thread::sleep(STARTUP_POLL);
        }
        anyhow::bail!(
            "the auto-managed cortex session did not become ready within {}s",
            STARTUP_TIMEOUT.as_secs()
        )
    }

    fn ready_status(&self) -> Option<Status> {
        self.probe.status().ok()
    }

    fn start_sibling(&self, deadline: Instant) -> Result<()> {
        let sibling = sibling_cortex_binary()?;
        let _ = writeln!(
            std::io::stderr(),
            "cortex-mcp: starting auto-managed session with {}",
            sibling.display()
        );
        let idle_seconds = AUTO_MANAGED_IDLE.as_secs().to_string();
        let mut child = Command::new(&sibling)
            .args([
                "session",
                "start",
                "--auto-managed",
                "--idle-timeout-seconds",
                &idle_seconds,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("starting sibling cortex binary at {}", sibling.display()))?;
        while Instant::now() < deadline {
            if let Some(status) = child.try_wait().context("waiting for sibling cortex")? {
                let stderr = read_child_stderr(&mut child);
                if status.success() || self.probe.is_running() {
                    return Ok(());
                }
                anyhow::bail!(
                    "sibling cortex could not start an auto-managed session ({status}): {}",
                    stderr.trim()
                );
            }
            std::thread::sleep(STARTUP_POLL);
        }
        let _ = child.kill();
        let _ = child.wait();
        let stderr = read_child_stderr(&mut child);
        anyhow::bail!(
            "sibling cortex did not finish startup within {}s: {}",
            STARTUP_TIMEOUT.as_secs(),
            stderr.trim()
        )
    }
}

fn read_child_stderr(child: &mut std::process::Child) -> String {
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    stderr
}

fn ensure_status_compatible(status: &Status) -> Result<()> {
    let version = status.daemon_version.parse::<u32>().unwrap_or(0);
    if version != DAEMON_PROTOCOL_VERSION {
        anyhow::bail!(
            "daemon protocol version mismatch: cortex-mcp expects {DAEMON_PROTOCOL_VERSION}, daemon reports {version}. Run `cortex session stop` to stop the old daemon, then retry."
        );
    }
    Ok(())
}

fn sibling_cortex_binary() -> Result<PathBuf> {
    let path = if let Some(path) = std::env::var_os("CORTEX_CLI_PATH") {
        PathBuf::from(path)
    } else {
        let current = std::env::current_exe().context("locating cortex-mcp executable")?;
        sibling_path(&current)
    };
    if !path.is_file() {
        anyhow::bail!(
            "could not find the sibling cortex binary at {}; install cortex and cortex-mcp together, or set CORTEX_CLI_PATH",
            path.display()
        );
    }
    Ok(path)
}

fn sibling_path(current_executable: &std::path::Path) -> PathBuf {
    current_executable.with_file_name(format!("cortex{}", std::env::consts::EXE_SUFFIX))
}

#[derive(Clone)]
struct CortexMcp {
    daemon: Arc<DaemonSupervisor>,
    tools: Arc<Vec<Tool>>,
}

pub async fn serve() -> Result<()> {
    let daemon = Arc::new(DaemonSupervisor::new());
    daemon.ensure()?;
    let server = CortexMcp {
        daemon,
        tools: Arc::new(tools()),
    };
    let service = rmcp::serve_server(server, BoundedStdioTransport::stdio())
        .await
        .context("starting cortex MCP server")?;
    service
        .waiting()
        .await
        .context("running cortex MCP server")?;
    Ok(())
}

impl ServerHandler for CortexMcp {
    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(ProtocolVersion::KNOWN_VERSIONS)
    }

    fn discover(
        &self,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<DiscoverResult, ErrorData>> + Send + '_ {
        std::future::ready(Ok(DiscoverResult::from_server_info(
            self.supported_protocol_versions().into_owned(),
            self.get_info(),
        )
        .with_ttl_ms(CATALOG_TTL_MS)
        .with_cache_scope(CacheScope::Private)))
    }

    async fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(self.tools.as_ref().clone())
            .with_ttl_ms(CATALOG_TTL_MS)
            .with_cache_scope(CacheScope::Private))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tools.iter().find(|tool| tool.name == name).cloned()
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if context.ct.is_cancelled() {
            return Ok(tool_error("tool call cancelled").into());
        }
        let server = self.clone();
        let cancellation = context.ct.clone();
        let task = tokio::task::spawn_blocking(move || {
            if cancellation.is_cancelled() {
                return Ok(tool_error("tool call cancelled"));
            }
            server.call_sync(request)
        });
        let result = task.await.map_err(|error| {
            ErrorData::internal_error(format!("MCP tool worker failed: {error}"), None)
        })??;
        drop(context);
        Ok(result.into())
    }

    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::V_2026_07_28)
            .with_server_info(
                Implementation::new("cortex-mcp", env!("CARGO_PKG_VERSION"))
                    .with_title("Quad Cortex MCP server")
                    .with_description("Provisional, non-persistent live-grid tools for the Neural DSP Quad Cortex")
                    .with_website_url("https://github.com/pacharanero/cortex"),
            )
            .with_instructions("Starts and reuses an auto-managed cortex session when needed. Tools may recall presets or alter the unsaved working grid, but this server exposes no save or delete operation.")
    }
}

impl CortexMcp {
    fn call_sync(&self, request: CallToolRequestParams) -> Result<CallToolResult, ErrorData> {
        if let Err(error) = self.daemon.ensure() {
            return Ok(tool_error_code(
                DaemonErrorCode::DeviceUnavailable,
                error.to_string(),
            ));
        }
        let name = request.name.as_ref();
        let args = Value::Object(request.arguments.unwrap_or_default());
        let result = self.dispatch(name, &args);
        Ok(match result {
            Ok(value) => CallToolResult::structured(value),
            Err(error) => match error.downcast_ref::<DaemonError>() {
                Some(daemon) => tool_error_code(daemon.code, daemon.message.clone()),
                None => tool_error(error.to_string()),
            },
        })
    }

    fn dispatch(&self, name: &str, args: &Value) -> Result<Value> {
        let request = match name {
            "get_status" => Request::Status,
            "get_device_version" => Request::Version,
            "get_active_scene" => Request::ActiveScene,
            "get_cpu_load" => Request::CpuLoad,
            "analyze_cpu_fit" => Request::AnalyzeCpuFit,
            "read_current_preset" | "list_blocks" => Request::CurrentPreset {
                with_params: bool_arg(args, "with_params", true)?,
                timeout_seconds: u64_arg(args, "timeout_seconds", 15)?,
            },
            "read_preset" => {
                let setlist = string_arg(args, "setlist")?;
                Request::ReadPreset {
                    factory: cortex_rs::client::is_factory_setlist(&setlist),
                    setlist,
                    slot: string_arg(args, "slot")?,
                    with_params: bool_arg(args, "with_params", true)?,
                    timeout_seconds: u64_arg(args, "timeout_seconds", 15)?,
                }
            }
            "list_presets" => Request::ListPresets {
                setlist: string_arg(args, "setlist")?,
                include_empty: bool_arg(args, "include_empty", false)?,
                timeout_seconds: u64_arg(args, "timeout_seconds", 30)?,
            },
            "list_folders" => Request::ListFolders {
                window_seconds: u64_arg(args, "window_seconds", 2)?,
            },
            "list_captures" => Request::ListCaptures {
                timeout_seconds: u64_arg(args, "timeout_seconds", 30)?,
            },
            "list_irs" => Request::ListIrs {
                folder: optional_string_arg(args, "folder")?,
                timeout_seconds: u64_arg(args, "timeout_seconds", 30)?,
            },
            "recall_preset" => {
                let setlist = string_arg(args, "setlist")?;
                Request::RecallPreset {
                    factory: cortex_rs::client::is_factory_setlist(&setlist),
                    setlist,
                    slot: string_arg(args, "slot")?,
                }
            }
            "switch_scene" => Request::SwitchScene {
                scene: bounded_u32(args, "scene", 0, 7)?,
            },
            "set_scene_label" => Request::SetSceneLabel {
                scene: bounded_u32(args, "scene", 0, 7)?,
                label: Some(string_arg(args, "label")?),
            },
            "unlabel_scene" => Request::SetSceneLabel {
                scene: bounded_u32(args, "scene", 0, 7)?,
                label: None,
            },
            "set_scene_color" => Request::SetSceneColor {
                scene: bounded_u32(args, "scene", 0, 7)?,
                color: u32_arg(args, "color")?,
            },
            "copy_scene" => Request::CopyScene {
                from_scene: bounded_u32(args, "from_scene", 0, 7)?,
                to_scene: bounded_u32(args, "to_scene", 0, 7)?,
                swap: false,
            },
            "swap_scenes" => Request::CopyScene {
                from_scene: bounded_u32(args, "first_scene", 0, 7)?,
                to_scene: bounded_u32(args, "second_scene", 0, 7)?,
                swap: true,
            },
            "set_block" => Request::SetBlock {
                row: bounded_u32(args, "row", 0, 3)?,
                column: bounded_u32(args, "column", 0, 7)?,
                model: u32_arg(args, "model")?,
                verify: bool_arg(args, "verify", true)?,
                timeout_seconds: u64_arg(args, "timeout_seconds", 5)?,
            },
            "set_param" => Request::SetParam {
                row: bounded_u32(args, "row", 0, 3)?,
                column: bounded_u32(args, "column", 0, 7)?,
                target: serde_json::from_value(required(args, "target")?.clone())?,
                input: serde_json::from_value(required(args, "input")?.clone())?,
                scene: optional_bounded_u32(args, "scene", 0, 7)?,
                promote: bool_arg(args, "promote", false)?,
                timeout_seconds: u64_arg(args, "timeout_seconds", 15)?,
            },
            "set_bypass" => Request::SetBypass {
                row: bounded_u32(args, "row", 0, 3)?,
                column: bounded_u32(args, "column", 0, 7)?,
                bypass: required(args, "bypass")?
                    .as_bool()
                    .context("bypass must be a boolean")?,
            },
            "remove_block" => Request::RemoveBlock {
                row: bounded_u32(args, "row", 0, 3)?,
                column: bounded_u32(args, "column", 0, 7)?,
            },
            "set_chain_input" => Request::SetRouting {
                row: bounded_u32(args, "row", 0, 3)?,
                input: Some(serde_json::from_value(required(args, "port")?.clone())?),
                output: None,
            },
            "set_chain_output" => Request::SetRouting {
                row: bounded_u32(args, "row", 0, 3)?,
                input: None,
                output: Some(serde_json::from_value(required(args, "port")?.clone())?),
            },
            "set_split" => Request::SetSplit {
                row: bounded_u32(args, "row", 0, 3)?,
                split: i32_arg(args, "split")?,
                mix: i32_arg_default(args, "mix", -1)?,
            },
            "set_capture" => Request::SetCapture {
                row: bounded_u32(args, "row", 0, 3)?,
                column: bounded_u32(args, "column", 0, 7)?,
                capture: serde_json::from_value(required(args, "capture")?.clone())?,
                model: optional_u32_arg(args, "model")?,
                timeout_seconds: u64_arg(args, "timeout_seconds", 15)?,
            },
            "set_ir" => Request::SetIr {
                row: bounded_u32(args, "row", 0, 3)?,
                column: bounded_u32(args, "column", 0, 7)?,
                ir: serde_json::from_value(required(args, "ir")?.clone())?,
                slot: bounded_u32(args, "slot", 0, 1)?,
                model: optional_u32_arg(args, "model")?,
                folder: optional_string_arg(args, "folder")?,
                timeout_seconds: u64_arg(args, "timeout_seconds", 15)?,
            },
            "search_catalog" => {
                let query = string_arg(args, "query")?;
                let payload = self.daemon.client.request_value(&Request::Catalog {
                    timeout_seconds: 15,
                })?;
                let bytes: Vec<u8> =
                    serde_json::from_value(required(&payload, "payload")?.clone())?;
                let catalog = cortex_rs::Catalog::parse(&bytes)?;
                return Ok(serde_json::to_value(catalog.search(&query))?);
            }
            _ => anyhow::bail!("unknown tool: {name}"),
        };
        let client = match &request {
            Request::SetParam {
                timeout_seconds, ..
            } => self
                .daemon
                .client
                .as_ref()
                .clone()
                .with_timeout(Duration::from_secs(timeout_seconds.saturating_mul(3) + 5)),
            _ => self.daemon.client.as_ref().clone(),
        };
        let value = client.request_value(&request)?;
        if name == "list_blocks" {
            let preset: cortex_rs::view::Preset = serde_json::from_value(value)?;
            return Ok(serde_json::to_value(preset.blocks)?);
        }
        Ok(value)
    }
}

fn tool_error(message: impl Into<String>) -> CallToolResult {
    tool_error_code(DaemonErrorCode::Internal, message)
}

fn tool_error_code(code: DaemonErrorCode, message: impl Into<String>) -> CallToolResult {
    CallToolResult::structured_error(json!({
        "code": code,
        "error": message.into(),
    }))
}

fn tools() -> Vec<Tool> {
    cortex_host::tool_registry::tools()
        .into_iter()
        .map(|spec| {
            tool(
                spec.name,
                &spec.description,
                spec.input_schema,
                spec.read_only,
            )
        })
        .collect()
}

fn tool(name: &'static str, description: &str, schema: JsonObject, read_only: bool) -> Tool {
    Tool::new(
        Cow::Borrowed(name),
        Cow::Owned(description.to_string()),
        Arc::new(schema),
    )
    .with_annotations(
        ToolAnnotations::with_title(name)
            .read_only(read_only)
            .destructive(false)
            .idempotent(read_only)
            .open_world(false),
    )
}

fn required<'a>(args: &'a Value, name: &str) -> Result<&'a Value> {
    args.get(name)
        .with_context(|| format!("missing required argument: {name}"))
}
fn string_arg(args: &Value, name: &str) -> Result<String> {
    Ok(required(args, name)?
        .as_str()
        .with_context(|| format!("{name} must be a string"))?
        .to_string())
}
fn optional_string_arg(args: &Value, name: &str) -> Result<Option<String>> {
    args.get(name).map_or(Ok(None), |value| {
        if value.is_null() {
            Ok(None)
        } else {
            value
                .as_str()
                .map(ToString::to_string)
                .map(Some)
                .with_context(|| format!("{name} must be a string or null"))
        }
    })
}
fn bool_arg(args: &Value, name: &str, default: bool) -> Result<bool> {
    args.get(name).map_or(Ok(default), |v| {
        v.as_bool()
            .with_context(|| format!("{name} must be a boolean"))
    })
}
fn u64_arg(args: &Value, name: &str, default: u64) -> Result<u64> {
    args.get(name).map_or(Ok(default), |v| {
        v.as_u64()
            .with_context(|| format!("{name} must be a non-negative integer"))
    })
}
fn optional_u32_arg(args: &Value, name: &str) -> Result<Option<u32>> {
    args.get(name).map_or(Ok(None), |value| {
        if value.is_null() {
            Ok(None)
        } else {
            u32::try_from(
                value
                    .as_u64()
                    .with_context(|| format!("{name} must be an unsigned integer or null"))?,
            )
            .map(Some)
            .with_context(|| format!("{name} exceeds u32"))
        }
    })
}
fn u32_arg(args: &Value, name: &str) -> Result<u32> {
    u32::try_from(
        required(args, name)?
            .as_u64()
            .with_context(|| format!("{name} must be a non-negative integer"))?,
    )
    .with_context(|| format!("{name} is too large"))
}
fn bounded_u32(args: &Value, name: &str, min: u32, max: u32) -> Result<u32> {
    let value = u32_arg(args, name)?;
    anyhow::ensure!((min..=max).contains(&value), "{name} must be {min}-{max}");
    Ok(value)
}
fn optional_bounded_u32(args: &Value, name: &str, min: u32, max: u32) -> Result<Option<u32>> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => bounded_u32(args, name, min, max).map(Some),
    }
}
fn i32_arg(args: &Value, name: &str) -> Result<i32> {
    i32::try_from(
        required(args, name)?
            .as_i64()
            .with_context(|| format!("{name} must be an integer"))?,
    )
    .with_context(|| format!("{name} is out of range"))
}
fn i32_arg_default(args: &Value, name: &str, default: i32) -> Result<i32> {
    args.get(name).map_or(Ok(default), |_| i32_arg(args, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destructive_tools_are_not_registered() {
        let names: Vec<_> = tools()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect();
        for forbidden in ["save_preset", "delete_preset", "move_preset"] {
            assert!(!names.iter().any(|name| name == forbidden));
        }
        assert!(names.contains(&"set_block".to_string()));
        assert!(names.contains(&"analyze_cpu_fit".to_string()));
        for scene_tool in [
            "set_scene_label",
            "unlabel_scene",
            "set_scene_color",
            "copy_scene",
            "swap_scenes",
        ] {
            assert!(names.contains(&scene_tool.to_string()));
        }
    }

    #[test]
    fn every_row_tool_surfaces_the_numbering_trap() {
        for tool in tools().into_iter().filter(|tool| {
            tool.input_schema
                .get("properties")
                .and_then(|p| p.get("row"))
                .is_some()
        }) {
            assert!(
                tool.description
                    .as_ref()
                    .is_some_and(|description| description.contains("zero-based")),
                "{}",
                tool.name
            );
        }
    }

    #[test]
    fn routing_tools_expose_closed_typed_port_schemas() {
        for (name, expected) in [
            (
                "set_chain_input",
                serde_json::to_value(cortex_rs::GridInputPort::ALL).unwrap(),
            ),
            (
                "set_chain_output",
                serde_json::to_value(cortex_rs::GridOutputPort::ALL).unwrap(),
            ),
        ] {
            let tool = tools().into_iter().find(|tool| tool.name == name).unwrap();
            let port = &tool.input_schema["properties"]["port"];
            assert_eq!(port["type"], "string");
            assert_eq!(port["enum"], expected);
            assert_eq!(tool.input_schema["required"], json!(["row", "port"]));
            assert!(tool.input_schema["properties"].get("column").is_none());
        }

        assert!(serde_json::from_value::<cortex_rs::GridInputPort>(json!(1)).is_err());
        assert!(serde_json::from_value::<cortex_rs::GridOutputPort>(json!("not_a_port")).is_err());
    }

    #[test]
    fn installed_mcp_finds_cortex_in_the_same_directory() {
        let current = PathBuf::from(format!(
            "/opt/cortex/bin/cortex-mcp{}",
            std::env::consts::EXE_SUFFIX
        ));
        assert_eq!(
            sibling_path(&current),
            PathBuf::from(format!(
                "/opt/cortex/bin/cortex{}",
                std::env::consts::EXE_SUFFIX
            ))
        );
    }

    #[test]
    fn incompatible_daemons_are_refused_not_replaced() {
        let status = Status {
            daemon_version: DAEMON_PROTOCOL_VERSION.saturating_sub(1).to_string(),
            uptime_seconds: 0,
            auto_managed: false,
            idle_timeout_seconds: None,
            device: cortex_host::DeviceHealth::Failed {
                error: "fixture".into(),
            },
            cache: cortex_host::CacheStatus::default(),
        };
        let error = ensure_status_compatible(&status).unwrap_err();
        assert!(error.to_string().contains("cortex session stop"));
    }
}
