# Agent Instructions

`cortex-rs` is the Rust workspace for an unofficial, Linux-first toolkit for
the Neural DSP Quad Cortex (and, in time, the Nano Cortex). The core deliverable
is a low-level leaf crate that speaks the Cortex Control USB HID protocol and
exposes a typed domain model - presets, the grid, blocks, and active-scene state. On top of the
crate sit a CLI (`cortex`), an MCP server (`cortex-mcp`) for agentic patch
editing, and a Tauri desktop GUI whose first draft is now scaffolded. This project is not affiliated with
or endorsed by Neural DSP; it is an interoperability client for hardware whose
vendor ships no Linux editor.

This file is the entry point for AI coding agents working in this Rust
workspace. Read it before changing anything. The parent workspace at
`/home/marcus/code/neuraldsp/` holds the vendored prior-art repos (gitignored,
reference-only) and the protocol research note that established the facts this
crate encodes.

## Read First

- [README.md](README.md) - setup (udev rule, build, run) and project overview.
- [docs/protocol.md](docs/protocol.md) - the public implementer-facing wire reference; correct it in the same change whenever evidence changes a protocol claim.
- [spec/prior-art.md](spec/prior-art.md) - what each reference project already knows, the exact files to read, and which negative results are worth re-testing. Check it before capturing hardware traffic.
- [NOTICE](NOTICE) and [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) - attribution for the MIT- and Apache-2.0-licensed prior art this project ports from.
- [/home/marcus/code/house-style/AGENTS.md](/home/marcus/code/house-style/AGENTS.md) - cross-repo standards (the source of truth for CI, distribution, licensing, docs, etc.).
- [/home/marcus/code/house-style/licensing.md](/home/marcus/code/house-style/licensing.md), [library-extraction.md](/home/marcus/code/house-style/library-extraction.md), [rust-cli.md](/home/marcus/code/house-style/rust-cli.md), [tauri-gui.md](/home/marcus/code/house-style/tauri-gui.md) - the specific standards this project follows.

## Prior art and licensing (read before reusing anything)

Vendored reference repos live at the parent workspace root
(`/home/marcus/code/neuraldsp/<repo>/`) and are gitignored; they are pinned
shallow clones for study, not part of this build. Their licenses govern what
may be taken from them.

| Repo | License | Use |
| --- | --- | --- |
| `pyquadcortex` (stokes-audio) | MIT | **Port freely with attribution.** The primary protocol source; the recovered `.proto` files are vendored into `crates/cortex-rs/proto/` with their own SPDX header. The framing, write-STALL, trailer envelope, session/client design, and most planned operations originate here. Read `pyquadcortex/docs/protocol.md`'s operation-coverage table and method docstrings before tracing the same operation. Record any derivation in `NOTICE` / `THIRD-PARTY-NOTICES.md`. |
| `deskop-nano-cortex` (rixrix) | Apache-2.0 | **Adapt with attribution.** The architectural precedent for the Tauri app (Rust device I/O, honest verified-vs-provisional labelling, AFX spec layout, release/dx tooling) and source for the adapted Nano current-state decoder/field map. |
| `qc-stomp-tools` (VanIseghemThomas) | MIT | Adapt with attribution. On-device footswitch/rotary/LED ioctls - relevant only if we ever target on-device builds. |
| `OpenCortex` (VanIseghemThomas) | **No repository-wide licence; mixed file-level notices** | **Reference only.** There is no root licence; some decryptor files carry GPL notices, while the rest remains unlicensed. Read for understanding of the device-rooting route; do **not** copy code, scripts, data, or docs into this repo. |
| `qc-extras` (roelj) | **No repository-wide licence; GPL-3.0-or-later source headers** | **Reference only.** The source headers grant GPL-3.0-or-later but there is no root licence defining the repository-wide scope. Cross-compilation notes only; do not copy. |
| `quad-cortex-usb-re-notes` (hsaastamoinen) | **None declared** | **Reference only.** Independent USB recon corroboration; do not copy prose or captures verbatim. |
| `toneparse` (vian21) | **None declared** | **Reference only, and not Quad Cortex prior art.** It parses Neural DSP desktop-plugin presets and Logic Pro channel strips, not Quad Cortex protobuf presets. Do not lift code or its bundled third-party preset content. |

The MIT- and Apache-2.0-licensed material is compatible with the project
license below. Anything copied or derived from `pyquadcortex`,
`deskop-nano-cortex`, or `qc-stomp-tools` must carry its upstream copyright and
a NOTICE entry. The four reference-only repos lack a clear repository-wide
licence, so none of their code, scripts, data, or prose may be committed here -
cite findings in our own words and link out.

`deskop-nano-cortex` credits its BLE field map to the non-vendored,
MIT-licensed `choldy/nano-cortex-web-editor`; adapting that decoder would
require attribution to both projects.

## Project license

Following house style: code is **AGPL-3.0-or-later**, written content is
**CC-BY-SA-4.0**. An SPDX header sits on every source file, naming the
copyright holder and the AGPL-3.0-or-later license identifier (with the
language's comment prefix); files that cannot carry an inline header are
covered by `REUSE.toml`. The vendored `.proto` files keep their own MIT SPDX
header (copyright (c) 2026 Stokes) above the original recovery note.

AGPL is a deliberate choice: the toolkit is not available for subsumption into
proprietary products. We can offer dual licensing if a closed derivative
genuinely needs to exist. The MIT/Apache prior art we port in remains under its
own terms; attribution is recorded, not relicensed.

## Architecture

```text
crates/
  cortex-rs/    The leaf crate (no host, no async runtime). USB HID transport
                (behind the `hid` feature), flag-driven framing, trailer-tagged
                message envelope, typed domain model, vendored protobuf schema
                built via prost.
  cortex-cli/   The `cortex` binary: thin main.rs, all behaviour in the crate.
  cortex-host/  Shared synchronous daemon contract and local IPC facade.
                 Unix sockets are implemented; Windows named pipes are planned
                 behind the same endpoint/listener/connection API.
  cortex-mcp/   The `cortex-mcp` MCP server: hardware-verified read, recall,
                scene and live-grid tools; no persistent save/delete tools.
gui/           Tauri 2 + React + Mantine first draft. Interactive, with
                explicit fixture and daemon-backed modes and no persistent save.
docs/          Protocol notes, runbooks, GUI docs.
spec/          Zone specs: spec.md (what it must do) + design.md (how, and why).
               NO tasks.md - progress lives in spec/roadmap.md (outstanding)
               and spec/completed.md (finished, moved verbatim), which is a
               deliberate divergence from the AFX layout and is explained in
               spec/001-overview/spec.md#progress-tracking.
s/             Repo scripts: s/test, s/lint, s/gui-dev, s/version++ ...
```

- **Leaf-crate discipline (house-style).** `cortex-rs` depends only on what it
  needs to encode the protocol and domain model (serde, bytes, flate2, prost,
  and optionally hidapi), never on the host app or an async runtime. Dependency
  arrows point *into* the crate. This is what lets the same crate drive the CLI,
  the MCP server, the Tauri backend, and eventually ship to crates.io. See
  [library-extraction.md](/home/marcus/code/house-style/library-extraction.md).
- **One implementation, many surfaces.** The CLI, MCP server, and Tauri backend
  all call the crate's public API; none reimplements protocol or domain logic.
  Follow the `clincalc`/`gitehr` precedent.
- **Rust owns behaviour; the webview owns interaction.** Tauri commands return
  typed serialisable data; the frontend renders it. See
  [tauri-gui.md](/home/marcus/code/house-style/tauri-gui.md).
- **Honest verified-vs-provisional labelling.** Borrow the
  `deskop-nano-cortex` product-truth discipline: apply evidence per operation
  and host path. The implemented core Quad Cortex paths are hardware-verified
  on CorOS 4.0.1; unimplemented operations, new host integrations, unknown
  message types, and Nano Cortex specifics remain provisional until verified.
  Label them as such in UI, docs, and release notes.
- **Nano Cortex transport is partly hardware-verified.** A real Nano on Linux
  confirmed VID:PID `152A:88E7`, HID interface 5, 65-byte reports, the same
  length/flag frame shape as the Quad, and a complete multi-report state read.
  Its BLE application payload maps directly onto HID, but its four-byte footer
  and fixed-chain domain differ from the Quad's eight-byte trailer and grid.
  The separate Nano codec and held-daemon path implement typed state plus
  hardware-verified amp, bypass and raw FX parameter operations; Gate reduction
  and wider application operations remain provisional. Quad session entry
  points still reject Nano before USB I/O so the envelopes cannot be conflated.

## Protocol invariants (do not break silently)

From `quad-cortex-linux-editor-and-protocol.md` and
`pyquadcortex/docs/protocol.md`, against CorOS 4.0.1:

- Transport: USB HID, VID:PID `152A:880A`, interface 5, input report ID
  `0x01`, output report ID `0x02`, 128-byte body + report-ID = 129 bytes at
  the hidapi boundary.
- Framing: `[report_id][len][flags][data...]`, flags `0x40` FIRST /
  `0x80` LAST / `0xC0` complete / `0x00` middle. No sequence numbers, no
  total-length field - reassembly is flag-driven.
- Envelope: reassembled message is `protobuf ++ 8-byte trailer`; the message
  type tag is a little-endian `uint16` `CortexMessageType` in the **trailer**,
  not a header.
- Compression: frame-level gzip (payload starts `1f 8b`) and field-level
  gzip inside protobuf `bytes` fields.
- **The benign write STALL:** every `SET_REPORT` is acted upon and *then*
  deliberately stalled at the status stage, so `hid_write()` returns `-1` on
  a write that worked. Swallow write errors; detect a dead device via read
  timeouts. This is the single most important gotcha and is encoded in
  `crates/cortex-rs/src/transport.rs`.
- Exclusive HID access: one owning process per device, not one connection per
  call. The held `cortex session` daemon owns that connection; MCP opens none.
- No version field on the wire: a CorOS update can silently break things.
  Surface a protocol-version probe, not a hard-coded assumption.

## MCP safety surface

The non-persistent MCP surface is implemented and hardware-verified; it opens
no HID connection and exposes no save/delete tools. The design that matters for
any future destructive tier is the safety boundary, not the tool count:

- CLI mutating commands execute when invoked and expose global `-n`/`--dry-run`
  as the non-mutating path. Future MCP and GUI persistent actions still require
  explicit confirmation because an agent or UI can invoke them indirectly.
- Never write to the factory setlist; require one explicitly named USER target
  for each persistent operation.
- Prepare every target and retain any backup before working-copy edits begin.
  A listing cannot prove emptiness, and reading a target after edits would
  recall it and destroy the grid being saved.
- Surface the row-numbering trap (zero-based in the API, 1-4 on screen; a
  wrong-row edit succeeds silently) in tool descriptions.
- Single owning process for the USB interface.

## Keep the protocol documentation current

[docs/protocol.md](docs/protocol.md) is the public write-up of the Cortex Control USB HID protocol, written for someone else building a client for this hardware. No such document existed when this project started.

**When you learn something new about the protocol, add it there in the same change** - not only in a code comment or a roadmap entry, which serve us rather than the reader. Specifically:

- Any device behaviour worth knowing in advance, including behaviour that turned out to be a client-side bug. "This looks like the device and is not" is among the most useful things on the page.
- Any measurement contradicting something the page claims. **Correct it rather than adding a caveat** - a wrong claim is worse than no claim, and several have survived on that page for months.
- Anything that fails silently: a request the device ignores without erroring, a collision that only surfaces on the next call, a field that turns out to matter. Those are what a newcomer cannot discover unaided.

Write it as reference, not as narrative: the facts and the numbers, not how they were arrived at. Label anything unverified against hardware as provisional, and give the `CorOS` version where a measurement might depend on it.

## Legal hygiene

Reverse engineering for interoperability is the established case (UK CDPA
s50B / s296A, EU Software Directive Art 6). Practical norms, following the
existing projects:

- Do not redistribute Neural DSP binaries, firmware, or artwork.
- Do not publish raw captures containing their strings (preset, path, device,
  build strings are readable).
- **Examples must use fictional device data.** Docs and specs are written from
  real hardware, so it is easy to paste in a serial number, MAC address,
  firmware checksum, or a player's own preset and Neural Capture names along
  with the useful part. None of that belongs in a public repo: the identifiers
  single out one person's unit and the names say what they play.
  `s/lint-no-device-data` enforces this and runs in `s/lint` and CI. Model
  names (`Brit 2203`, `Rodent Drive`) are Neural DSP's and identical on every
  unit, so those are fine - it is the owner-specific values that are not.
- Keep the recovered schema limited to what interoperability requires.
- State clearly that the work is unofficial and unaffiliated.
- Prefer the USB route over the device-rooting route (OpenCortex); the latter
  carries warranty risk the USB route does not.

## Workflow

- **Record missing capabilities.** If a development agent or an agent using the MCP server needs a `cortex` capability that does not exist, add it to `spec/roadmap.md` with a stable ID and enough context for another agent to implement it. Do not improvise around the missing operation or leave it only in chat.
- `s/test` - run `cargo test` across the workspace.
- `s/lint` - `cargo fmt --check`, clippy `-D warnings`, `reuse lint`.
- `s/gui-dev` - run the Tauri dev server from any working directory.
- `s/version++` - bump the canonical version across Cargo, npm package/lock,
  and Tauri configuration in one release commit.

## Before Every Commit

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
reuse lint
```

Frontend changes additionally: `npm run check` (lint + typecheck) inside
`gui/`. GUI changes additionally: the manual hardware smoke runbook against a
real Quad Cortex - CI has no hardware.

## Approval Required

Ask before publishing the crate to crates.io, cutting a release tag, changing
the license, editing `NOTICE`/`THIRD-PARTY-NOTICES.md` attribution,
force-pushing, or any externally visible action. Do not commit vendored
reference-repo content into this repo's tree.

## Assurance

- Treat agent output as a capable junior's: read the diff and run the
  validation above.
- For protocol behaviour, validate against real hardware where possible;
  until then mark features provisional. The `pyquadcortex` offline test suite
  is a useful conformance reference but is not a substitute for a hardware
  smoke run.
- Agent-generated tests must not be the sole basis for accepting protocol or
  safety-surface behaviour. Cross-check against the recovered `.proto` files
  and a real device.
