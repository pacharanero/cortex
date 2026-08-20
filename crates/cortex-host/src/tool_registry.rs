// SPDX-FileCopyrightText: 2026 Dr Marcus Baw
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Machine-discoverable agent tool contracts shared by the CLI and MCP server.

use serde_json::{Map, Value, json};

/// The row numbering warning required on every row-taking operation.
pub const ROW_TRAP: &str = "Rows are zero-based 0-3 in this API but labelled 1-4 on the unit; the wrong row succeeds silently.";

/// One bounded agent-facing operation contract.
#[derive(Debug, Clone)]
pub struct ToolSpec {
    /// Stable machine-facing operation name.
    pub name: &'static str,
    /// Human and model-facing explanation, including applicable hazards.
    pub description: String,
    /// JSON Schema for the operation's arguments.
    pub input_schema: Map<String, Value>,
    /// Whether the operation has no device-visible side effect.
    pub read_only: bool,
}

/// The canonical operation registry for agent-facing CLI discovery and MCP.
#[must_use]
pub fn tools() -> Vec<ToolSpec> {
    let mut tools = reads();
    tools.extend(scenes());
    tools.extend(grid_edits());
    tools
}

fn reads() -> Vec<ToolSpec> {
    vec![
        spec(
            "get_status",
            "Read held-session health and cache status.",
            empty_schema(),
            true,
        ),
        spec(
            "read_nano_state",
            "Read the Nano Cortex fixed eight-role chain and raw amp controls. Read-only; requires a Nano-owned held session.",
            empty_schema(),
            true,
        ),
        spec(
            "get_device_version",
            "Read device identity and CorOS version.",
            empty_schema(),
            true,
        ),
        spec(
            "get_active_scene",
            "Read the active zero-based scene index (A-H is 0-7).",
            empty_schema(),
            true,
        ),
        spec(
            "get_cpu_load",
            "Read the most recent subscribed DSP CPU-load push.",
            empty_schema(),
            true,
        ),
        spec(
            "analyze_cpu_fit",
            "Explain the latest device-reported CPU load against the live grid. The Quad reports two DSP cores; rows are signal-chain lanes, not fixed cores. Returns only read-only, conservative rerouting guidance.",
            empty_schema(),
            true,
        ),
        spec(
            "read_current_preset",
            "Read the live working grid without recalling a slot or discarding unsaved edits.",
            read_schema(),
            true,
        ),
        spec(
            "list_blocks",
            "List blocks on the live working grid without recalling a slot.",
            read_schema(),
            true,
        ),
        spec(
            "read_preset",
            "RECALLS a stored slot, discarding unsaved edits and resetting the active scene. Use read_current_preset during editing.",
            stored_schema(),
            false,
        ),
        spec(
            "list_presets",
            "List preset slots in a device setlist.",
            list_schema(),
            true,
        ),
        spec(
            "list_folders",
            "List folders announced by the device.",
            object_schema(
                &json!({"window_seconds":{"type":"integer","minimum":1,"maximum":30,"default":2}}),
                &[],
            ),
            true,
        ),
        spec(
            "search_catalog",
            "Search model names, categories and Neural DSP's verbatim attribution.",
            object_schema(
                &json!({"query":{"type":"string","minLength":1}}),
                &["query"],
            ),
            true,
        ),
        spec(
            "list_captures",
            "List existing device Neural Captures. Choose an entry from this result before set_capture; capture creation and transfer are native-device workflows.",
            timeout_schema(30),
            true,
        ),
        spec(
            "list_irs",
            "List existing device IRs from the default or a named library folder. Choose an entry from this result before set_ir; IR transfer is a native-device workflow.",
            object_schema(
                &json!({"folder":{"type":["string","null"],"minLength":1},"timeout_seconds":{"type":"integer","minimum":1,"maximum":60,"default":30}}),
                &[],
            ),
            true,
        ),
    ]
}

fn scenes() -> Vec<ToolSpec> {
    vec![
        spec(
            "recall_preset",
            "Recall a stored preset. Changes what is heard, discards unsaved edits and resets the active scene; does not save.",
            stored_identity_schema(),
            false,
        ),
        spec(
            "switch_scene",
            "Switch scene A-H using zero-based index 0-7. Changes what is heard; does not save.",
            scene_schema("scene"),
            false,
        ),
        spec(
            "set_scene_label",
            "Set a scene label on the unsaved working copy. Scenes A-H are zero-based 0-7; does not save.",
            object_schema(
                &json!({"scene":{"type":"integer","minimum":0,"maximum":7},"label":{"type":"string","minLength":1}}),
                &["scene", "label"],
            ),
            false,
        ),
        spec(
            "unlabel_scene",
            "Clear a scene label on the unsaved working copy. Scenes A-H are zero-based 0-7; does not save.",
            scene_schema("scene"),
            false,
        ),
        spec(
            "set_scene_color",
            "Set a scene colour on the unsaved working copy as an ARGB uint32. Scenes A-H are zero-based 0-7; does not save.",
            object_schema(
                &json!({"scene":{"type":"integer","minimum":0,"maximum":7},"color":{"type":"integer","minimum":0,"maximum":4_294_967_295_u64}}),
                &["scene", "color"],
            ),
            false,
        ),
    ]
}

fn grid_edits() -> Vec<ToolSpec> {
    vec![
        spec(
            "set_nano_amp",
            "Set one Nano amp control to a raw 0-255 value. Changes heard working state, saves nothing, and succeeds only after a fresh state read confirms the value. Requires a Nano-owned held session and takes about six seconds.",
            object_schema(
                &json!({"control":{"type":"string","enum":["gain","level","bass","mid","treble"]},"value":{"type":"integer","minimum":0,"maximum":255}}),
                &["control", "value"],
            ),
            false,
        ),
        spec(
            "copy_scene",
            "Copy one scene's parameter, bypass, label and colour state onto another scene in the unsaved working copy. Scenes A-H are zero-based 0-7; does not save.",
            scene_pair_schema("from_scene", "to_scene"),
            false,
        ),
        spec(
            "swap_scenes",
            "Exchange two scenes' parameter, bypass, label and colour state in the unsaved working copy. Scenes A-H are zero-based 0-7; does not save.",
            scene_pair_schema("first_scene", "second_scene"),
            false,
        ),
        spec(
            "set_block",
            format!(
                "Place a model on the unsaved grid. {ROW_TRAP} DSP-capacity refusal is silent; verify defaults true and catches it by echo or read-back."
            ),
            cell_schema(
                &json!({"model":{"type":"integer","minimum":1},"verify":{"type":"boolean","default":true},"timeout_seconds":{"type":"integer","minimum":1,"maximum":60,"default":5}}),
                &["model"],
            ),
            false,
        ),
        spec(
            "set_param",
            format!(
                "Set and read back one parameter on the unsaved grid. {ROW_TRAP} A scene-targeted write sends promote/switch/write and leaves that scene active."
            ),
            param_schema(),
            false,
        ),
        spec(
            "set_bypass",
            format!("Bypass or enable a block and verify it by live-grid read-back. {ROW_TRAP}"),
            cell_schema(&json!({"bypass":{"type":"boolean"}}), &["bypass"]),
            false,
        ),
        spec(
            "remove_block",
            format!(
                "Remove a block and verify the cell is empty by live-grid read-back. {ROW_TRAP}"
            ),
            cell_schema(&json!({}), &[]),
            false,
        ),
        spec(
            "set_chain_input",
            format!("Set and read back a typed row input on the unsaved grid. {ROW_TRAP}"),
            routing_schema(cortex_rs::GridInputPort::ALL),
            false,
        ),
        spec(
            "set_chain_output",
            format!(
                "Set and read back a typed row output on the unsaved grid. {ROW_TRAP} Internal next-row routes and the real multiple output are named explicitly."
            ),
            routing_schema(cortex_rs::GridOutputPort::ALL),
            false,
        ),
        spec(
            "set_split",
            format!(
                "Set and read back row branch and rejoin columns on the unsaved grid. {ROW_TRAP} Only rows 0 and 2 can branch; split=-1 clears and mix=-1 means never rejoin."
            ),
            split_schema(),
            false,
        ),
        spec(
            "set_capture",
            format!(
                "Select an exact entry returned by list_captures in the unsaved grid. {ROW_TRAP} The daemon rechecks that key/name against a fresh device listing; set model 14000 to place a Capture block first. Capture creation and transfer are native-device workflows."
            ),
            cell_schema(
                &json!({"capture":library_entry_schema(),"model":{"type":["integer","null"],"enum":[14000,null]},"timeout_seconds":{"type":"integer","minimum":1,"maximum":60,"default":15}}),
                &["capture"],
            ),
            false,
        ),
        spec(
            "set_ir",
            format!(
                "Select an exact entry returned by list_irs in one IR Loader slot on the unsaved grid. {ROW_TRAP} The daemon rechecks that key/name against a fresh device listing; model may place an IR Loader first. Read-back proves stored strings only, so inspect the unit for its warning icon."
            ),
            cell_schema(
                &json!({"ir":library_entry_schema(),"slot":{"type":"integer","minimum":0,"maximum":1},"model":{"type":["integer","null"],"minimum":29001,"maximum":29008},"folder":{"type":["string","null"],"minLength":1},"timeout_seconds":{"type":"integer","minimum":1,"maximum":60,"default":15}}),
                &["ir", "slot"],
            ),
            false,
        ),
    ]
}

fn spec(
    name: &'static str,
    description: impl Into<String>,
    input_schema: Map<String, Value>,
    read_only: bool,
) -> ToolSpec {
    ToolSpec {
        name,
        description: description.into(),
        input_schema,
        read_only,
    }
}
fn empty_schema() -> Map<String, Value> {
    object_schema(&json!({}), &[])
}
fn read_schema() -> Map<String, Value> {
    object_schema(
        &json!({"with_params":{"type":"boolean","default":true},"timeout_seconds":{"type":"integer","minimum":1,"maximum":60,"default":15}}),
        &[],
    )
}
fn timeout_schema(default: u64) -> Map<String, Value> {
    object_schema(
        &json!({"timeout_seconds":{"type":"integer","minimum":1,"maximum":60,"default":default}}),
        &[],
    )
}
fn library_entry_schema() -> Value {
    json!({"type":"object","properties":{"key":{"type":"string","minLength":1},"name":{"type":"string","minLength":1}},"required":["key","name"],"additionalProperties":false})
}
fn stored_identity_schema() -> Map<String, Value> {
    object_schema(
        &json!({"setlist":{"type":"string","minLength":1},"slot":{"type":"string","pattern":"^(?:[1-9]|[12][0-9]|3[0-2])[A-H]$"}}),
        &["setlist", "slot"],
    )
}
fn stored_schema() -> Map<String, Value> {
    let mut schema = stored_identity_schema();
    let properties = schema
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .expect("object schema properties");
    properties.insert(
        "with_params".into(),
        json!({"type":"boolean","default":true}),
    );
    properties.insert(
        "timeout_seconds".into(),
        json!({"type":"integer","minimum":1,"maximum":60,"default":15}),
    );
    schema
}
fn list_schema() -> Map<String, Value> {
    object_schema(
        &json!({"setlist":{"type":"string","minLength":1},"include_empty":{"type":"boolean","default":false},"timeout_seconds":{"type":"integer","minimum":1,"maximum":120,"default":30}}),
        &["setlist"],
    )
}
fn routing_schema<T: serde::Serialize>(ports: &[T]) -> Map<String, Value> {
    object_schema(
        &json!({"row":{"type":"integer","minimum":0,"maximum":3,"description":ROW_TRAP},"port":{"type":"string","enum":serde_json::to_value(ports).expect("routing ports serialize")}}),
        &["row", "port"],
    )
}
fn split_schema() -> Map<String, Value> {
    object_schema(
        &json!({"row":{"type":"integer","minimum":0,"maximum":3,"description":ROW_TRAP},"split":{"type":"integer","minimum":-1,"maximum":7},"mix":{"type":"integer","minimum":-1,"maximum":7,"default":-1}}),
        &["row", "split"],
    )
}
fn scene_schema(name: &str) -> Map<String, Value> {
    object_schema(
        &json!({(name):{"type":"integer","minimum":0,"maximum":7}}),
        &[name],
    )
}
fn scene_pair_schema(first: &str, second: &str) -> Map<String, Value> {
    object_schema(
        &json!({(first):{"type":"integer","minimum":0,"maximum":7},(second):{"type":"integer","minimum":0,"maximum":7}}),
        &[first, second],
    )
}
fn param_schema() -> Map<String, Value> {
    cell_schema(
        &json!({"target":{"oneOf":[{"type":"object","properties":{"by":{"const":"index"},"value":{"type":"integer","minimum":0}},"required":["by","value"]},{"type":"object","properties":{"by":{"const":"name"},"value":{"type":"string","minLength":1}},"required":["by","value"]}]},"input":{"oneOf":[{"type":"object","properties":{"kind":{"const":"normalised"},"value":{"type":"number","minimum":0,"maximum":1}},"required":["kind","value"]},{"type":"object","properties":{"kind":{"const":"real"},"value":{"type":"number"}},"required":["kind","value"]},{"type":"object","properties":{"kind":{"const":"text"},"value":{"type":"string"}},"required":["kind","value"]}]},"scene":{"type":["integer","null"],"minimum":0,"maximum":7},"promote":{"type":"boolean","default":false},"timeout_seconds":{"type":"integer","minimum":1,"maximum":60,"default":15}}),
        &["target", "input"],
    )
}
fn cell_schema(extra: &Value, extra_required: &[&str]) -> Map<String, Value> {
    let mut properties = json!({"row":{"type":"integer","minimum":0,"maximum":3,"description":ROW_TRAP},"column":{"type":"integer","minimum":0,"maximum":7}});
    properties
        .as_object_mut()
        .expect("object properties")
        .extend(extra.as_object().expect("extra object").clone());
    let mut required = vec!["row", "column"];
    required.extend_from_slice(extra_required);
    object_schema(&properties, &required)
}
fn object_schema(properties: &Value, required: &[&str]) -> Map<String, Value> {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}).as_object().expect("object schema").clone()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn every_tool_has_a_unique_bounded_object_schema() {
        let tools = tools();
        let names = tools.iter().map(|tool| tool.name).collect::<BTreeSet<_>>();
        assert_eq!(names.len(), tools.len(), "tool names must be unique");
        for tool in tools {
            assert_eq!(tool.input_schema["type"], "object", "{}", tool.name);
            assert_eq!(
                tool.input_schema["additionalProperties"], false,
                "{} accepts unknown arguments",
                tool.name
            );
            assert!(
                tool.input_schema["properties"].is_object(),
                "{} has object properties",
                tool.name
            );
        }
    }

    #[test]
    fn every_row_tool_carries_the_numbering_trap() {
        for tool in tools()
            .into_iter()
            .filter(|tool| tool.input_schema["properties"].get("row").is_some())
        {
            assert_eq!(
                tool.input_schema["properties"]["row"]["description"], ROW_TRAP,
                "{} must explain the row-numbering trap",
                tool.name
            );
        }
    }
}
