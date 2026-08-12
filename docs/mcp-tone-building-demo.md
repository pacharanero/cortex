# MCP Tone-Building Demo

This is a reproducible worked demonstration of why the local `cortex-mcp` server exists: an agent can turn a plain-English sound brief into an unsaved Quad Cortex grid by researching the reference, querying the device's current catalog, and verifying each live edit. It is unofficial and unaffiliated with Neural DSP Technologies.

## Brief

> Build a basic 1987 Guns N' Roses Slash-style tone.

The reference research should be performed live, not copied from this page. In the recorded run, independent music-gear reporting described the *Appetite for Destruction* recording chain as a Les Paul-style guitar into a modified Marshall Super Lead and Marshall 4x12 with Greenback speakers, with effects used sparingly. The exact studio amp is disputed, so this is a tonal starting point rather than a claim of historical replication. See [Guitar.com](https://guitar.com/features/artist-rigs/the-gear-used-by-slash-guns-n-roses-appetite-for-destruction/) and [Boost Guitar Pedals](https://www.boostguitarpedals.co.uk/blogs/gear-of-the-gods/slashs-guitar-gear-on-appetite-for-destruction).

## Safety boundary

- Use a confirmed-empty slot in a USER setlist. Never use the factory library.
- Recall discards the current unsaved working copy and changes the audible sound.
- The MCP server has no save or delete tool. This demo changes only the live working grid.
- The final recall is unconditional cleanup. It leaves the selected empty slot loaded and removes the demonstration grid.
- Rows are zero-based in MCP: row `0` is the top hardware row.

## Run

Start from a connected Quad Cortex, with Cortex Control closed. Replace `6B` with an empty USER slot you have explicitly approved for the demonstration.

```sh
cortex session start
CORTEX_HARDWARE_SMOKE_SLOT=6B \
  cargo test -p cortex-mcp --test server-process \
  hardware_smoke_builds_and_restores_a_live_grid -- --ignored --nocapture
```

The ignored integration test starts the real stdio MCP server, not a test-only substitute. It performs these operations through MCP:

1. Checks daemon status, device version, CPU data, and searches the live catalog.
2. Recalls the nominated empty USER slot.
3. Places `Brit Plexi 100 Bright` (catalog ID `1091`) in row `0`, column `0`, raises its `GAIN` to `6.0`, and proves parameter and bypass write/read-back behavior.
4. Places `412 Brit 60B GB 71 (M)` (catalog ID `12006`) in row `0`, column `1` and confirms both models in a complete live-grid read.
5. Exercises typed routing and split controls, removes the two blocks, and verifies an existing device Capture selection through the MCP library tools.
6. Recalls the nominated empty slot whether the run passes or fails.

The amp and cab are selected from the Quad's current catalog, whose vendor-provided attribution describes the first as a Marshall Super Lead 100 Bright model and the second as a Marshall 1960B with Celestion Pulsonic Greenback drivers. This avoids maintaining a stale hand-written model map across CorOS releases.

## Result

The run on CorOS 4.0.1 passed on 2026-08-12 in 14.93 seconds. It confirmed the Plexi and Greenback models in the fresh live-grid read-back, then restored the empty slot. No preset was saved or overwritten.

Tone still depends on the guitar, pickups, monitoring, and playing. This run demonstrates safe, verifiable agent control and a researched starting point, not a byte-for-byte recreation of a record.
