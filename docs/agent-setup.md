# Agent Setup

`cortex-mcp` gives a local agent controlled access to the Quad Cortex connected to this Linux machine. It uses stdio and talks through the held `cortex session` daemon; it does not open a second USB connection and it sends no device data to a hosted Cortex service.

The current tool surface can read device and preset state, recall presets, switch/label/recolour/copy/swap scenes, and edit the unsaved live grid. It deliberately exposes no save or delete tool. Recall and grid edits are audible and can discard an unsaved working copy, but they do not persist unless separately saved through the CLI.

## Before registering it

Install both binaries together using the [installation guide](install.md), connect the Quad Cortex, and quit Cortex Control. `cortex-mcp` locates the sibling `cortex` binary and starts an auto-managed device session when the agent connects. No separate startup command is required.

To hold the device connection independently of an MCP harness, start an explicit persistent session:

```sh
cortex session start
cortex session status
```

An explicitly user-started session remains persistent until `cortex session stop`. An MCP-started session instead exits after 60 seconds without a completed request; the same running MCP process starts a replacement automatically on its next tool call. Concurrent MCP launches converge on the endpoint claim, and an existing explicit session is reused rather than replaced.

## Safety and conventions

- MCP rows are zero-based (`0` to `3`), while the hardware screen labels them `1` to `4`. Every row-taking tool repeats this because a wrong-row edit can succeed silently.
- `read_preset` recalls the stored slot, discards an unsaved working copy and resets the active scene. Use `read_current_preset` for side-effect-free inspection of what is loaded now.
- Recall and live-grid writes are audible but non-persistent. The server has no save or delete tool.
- Routing tools take closed typed names rather than wire integers. Examples include `input1`, `return1`, `previous_row`, `xlr12`, `next_row3`, and `multiple`; MCP discovery exposes every accepted input and output name.
- Parameter, bypass, removal, routing, and split writes return only after a complete live-grid read confirms the requested state. A mismatch is returned as `outcome_unconfirmed`; block placement retains its echo-or-read-back confirmation path.
- `analyze_cpu_fit` is read-only. The Quad reports two DSP cores, while its four rows are signal-chain lanes; use the returned per-cell `on_core2` mapping rather than treating a row as a core. A cross-row move changes routing and must be fresh-read and auditioned.
- Scene indices are zero-based 0-7 (A-H). `copy_scene` and `swap_scenes` move labels and colours as well as sound state. Scene colours are decimal ARGB `uint32` values in MCP schemas; copy/swap force a fresh live-grid read before returning because the device's acknowledgement omits its swap flag.

The server exposes status, device version, active scene, CPU load, live/stored preset reads, block/folder/preset listings, catalog search, recall, scene switching and metadata/copy management, block placement/removal, read-back-verified parameter/bypass writes, typed chain input/output routing and split control. Use MCP discovery for the authoritative schemas.

## Claude Code

Register the absolute executable path at user scope so the server is available from every project and does not depend on the restricted `PATH` of a graphical launcher:

```sh
claude mcp add --transport stdio --scope user cortex -- "$(command -v cortex-mcp)"
claude mcp list
```

The `--` is required: it separates Claude Code's options from the command it launches. In Claude Code, use `/mcp` to confirm that `cortex` is connected and exposes tools.

To remove it later:

```sh
claude mcp remove --scope user cortex
```

## Other MCP harnesses

Configure a local stdio server whose command is the absolute path printed by `command -v cortex-mcp`. The common JSON shape is:

```json
{
  "mcpServers": {
    "cortex": {
      "command": "/home/you/.cargo/bin/cortex-mcp",
      "args": []
    }
  }
}
```

Configuration filenames and top-level keys differ between harnesses, so use that harness's local-stdio MCP settings rather than assuming this exact JSON file location. The server requires no API key or environment variable.

## Troubleshooting

- `could not find the sibling cortex binary`: install `cortex` and `cortex-mcp` together in the same directory. `CORTEX_CLI_PATH` can identify an explicit binary when a development harness separates them.
- `device not found`: install the udev rule, replug the unit, and make sure Cortex Control or a VM is not holding it.
- `daemon protocol version mismatch`: run `cortex session stop`, then retry the MCP connection. The server refuses to kill or replace an incompatible owner automatically.
- MCP connects but a tool reports `reconnecting`: wait for `cortex session status` to return `connected`; operations fail closed while the daemon rebuilds live state.
