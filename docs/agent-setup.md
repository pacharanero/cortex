# Agent Setup

`cortex-mcp` gives a local agent controlled access to the Quad Cortex connected to this Linux machine. It uses stdio and talks through the held `cortex session` daemon; it does not open a second USB connection and it sends no device data to a hosted Cortex service.

The current tool surface can read device and preset state, recall presets, switch scenes, and edit the unsaved live grid. It deliberately exposes no save or delete tool. Recall and grid edits are audible and can discard an unsaved working copy, but they do not persist unless separately saved through the CLI.

## Before registering it

Install both binaries using the [installation guide](install.md), connect the Quad Cortex, and quit Cortex Control. Start the one process that owns the USB interface:

```sh
cortex session start
cortex session status
```

The MCP server currently requires that held session to be running before the agent starts it. Automatic daemon startup and idle shutdown are the next distribution lifecycle step.

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

- `no held cortex session is listening`: run `cortex session start`, then restart or reconnect the MCP server.
- `device not found`: install the udev rule, replug the unit, and make sure Cortex Control or a VM is not holding it.
- `daemon protocol version mismatch`: run `cortex session stop`, then `cortex session start` so both installed binaries use the same protocol version.
- MCP connects but a tool reports `reconnecting`: wait for `cortex session status` to return `connected`; operations fail closed while the daemon rebuilds live state.
