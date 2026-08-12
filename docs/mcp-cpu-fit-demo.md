# MCP CPU-Fit Demo

`analyze_cpu_fit` answers a more useful version of "why did that block not fit?" It joins the Quad Cortex's latest subscribed CPU push to its current live grid and returns the device-reported CPU share by cell and by DSP core. It changes nothing on the device.

## The important distinction

The Quad reports **two DSP cores**. It has **four grid rows**, but a row is a signal-chain lane rather than a core. CPU entries are per row and column and carry `on_core2`, the device-reported assignment to the second core. Do not infer that rows one and two are core one while rows three and four are core two, or any similar fixed mapping.

Moving a block to another row can create a parallel signal route and may change where the device schedules work. It is not a guaranteed way to transfer load to a chosen core. Only wire rows `0` and `2` can branch, and a cross-row move can create or alter split/rejoin routing. It is audible and must be read back and auditioned.

## Run

Start a held session and ask the local MCP server to inspect the working grid:

```sh
cortex session start
```

Call `analyze_cpu_fit` from an MCP client with `{}`. It returns this shape:

```json
{
  "total": 57.0,
  "cores": [
    { "core": 1, "load": 20.0 },
    { "core": 2, "load": 37.0 }
  ],
  "cells": [
    {
      "row": 1,
      "column": 0,
      "model": "Example block",
      "load": 27.0,
      "on_core2": true
    }
  ],
  "advice": ["..."]
}
```

The figures and model name above are illustrative. The MCP response always comes from the connected unit's latest push and live grid.

## How to act on it

1. Identify the highest-load cells and the more-loaded core from the response.
2. If placement is refused, first reduce or replace one expensive block. This preserves routing and is the least disruptive option.
3. If the tone needs a parallel route, make one cross-row move or placement through the normal verified grid tools. Use the returned `on_core2` values only as observations after the change, not as a routing plan.
4. Fresh-read the grid and call `analyze_cpu_fit` again. Confirm the route, the device-reported core mapping, and the sound before making another change.

The analyzer deliberately makes no automatic edits and does not claim capacity from catalog CPU metadata. A placement can still be silently refused, so every `set_block` must retain its normal echo-or-read-back verification.

## Verification

The official MCP-client hardware smoke passed against a real Quad Cortex running CorOS 4.0.1 on 2026-08-12 in 0.10 seconds. It started from a held subscribed session, made no grid edit, and confirmed that `analyze_cpu_fit` returned exactly two cores and the no-fixed-row-to-core safety guidance.
