<!--
SPDX-FileCopyrightText: 2026 Dr Marcus Baw
SPDX-License-Identifier: CC-BY-SA-4.0
-->

# Queries

Open questions for Marcus. Answer inline under each; I will action and remove them.

Raised while working through `spec/marcus-notes.md`.

## 1. CLI redesign: what is the target shape?

You want commands rooted in the noun primitives, named exactly as the Neural DSP / Quad Cortex user guide names them - preset, slot, grid, row, column.

Today the surface is flat and verb-first: `set-param`, `set-bypass`, `set-block`, `remove-block`, `set-input`, `set-output`, `set-split`, `presets`, `preset`, `grid`, `recall`, `folders`.

The obvious reshaping is noun-then-verb:

```sh
cortex preset list                 # was: presets
cortex preset show 1B              # was: preset --slot 1B
cortex preset recall 1B            # was: recall --slot 1B
cortex grid show                   # was: grid
cortex block bypass --row 1 --column 0
cortex block remove --row 1 --column 0
cortex block set   --row 1 --column 0 --model 1001
cortex row  input  --row 1 --port 0
cortex row  split  --row 1 --split 3
```

**Questions:**

1. Is `block` a term the QC guide uses, or does it say "module"? The recovered schema calls the type `Model` and the message `ModuleStats`, which suggests the vendor's internal word is *module* while the UI may say *block*. **Which should we follow - what the user guide shows a player, or what the wire calls it?** My instinct is the user guide, since the CLI is for players, with the wire name noted in docs.
2. Is `row` right for the CLI, given the guide calls them rows but the wire is zero-based and the screen is 1-4? The current `--row` takes the SCREEN number, which I think is correct for a player-facing tool.
3. **How much breakage is acceptable?** Nothing is released, so a clean break costs nothing but your muscle memory. Alternatively I can keep the old names as hidden aliases for a while. Which?
4. Does `cortex connect` stay as-is? It is not a noun primitive of the device - it is ours - so it may want a different grouping, e.g. `cortex session start/status/stop`.

## 2. Command reference: where do the examples live?

You want syntax plus an example per command rather than raw `--help` output.

Syntax can stay generated from clap, so it cannot drift. Examples cannot be generated - they need authoring.

**Question:** where should they live?

- **(a)** In the clap definitions themselves, as `after_help`. They then show in `cortex <cmd> --help` too, which is a real benefit, and one source feeds both.
- **(b)** In a separate `docs/examples.toml` keyed by command, merged in at generation time. Keeps `--help` terse.

I prefer **(a)**: a player running `--help` is exactly the person who wants an example, and it keeps one source of truth. Confirm?

## 3. MCP demos: how much autonomy, and what safety posture?

The demo you describe - "give me a basic 1987 GnR Slash tone", agent researches, agent builds the preset - crosses several boundaries at once: web research, model selection, many grid writes, and a save.

The MCP safety surface in AGENTS.md says reads and recalls are free, saves are always explicitly confirmed, and writes are restricted to a scratch range.

**Questions:**

1. For a demo, is **building into the live grid without saving** the right target? That needs no save confirmation, is fully reversible by recalling, and still demonstrates the capability.
2. Should the agent be allowed to browse the web as part of the flow, or should the demo use a fixed, cited set of references so it is reproducible? A demo that depends on live search results will not reproduce, and will age badly in docs.
3. Is the scratch slot still **2B**, and is that the only slot an agent may write?

## 4. GUI: confirming the stack and the always-visible elements

You want a left sidebar showing the preset directory at all times, and CPU% visible at all times.

**Questions:**

1. Still Tauri 2 + React + Mantine as AGENTS.md says?
2. Should the sidebar show setlists as a tree (folder then slots), or a flat list of the current setlist with a setlist picker above it? The device has 399 folders but only a couple are non-empty, so a tree may look emptier than it is.
3. CPU%: total only, or the per-row/per-column breakdown too? We now read both, including which DSP core each column is on. The per-core detail explains why a preset does not fit, which is the question a player actually has - but it is more UI.
