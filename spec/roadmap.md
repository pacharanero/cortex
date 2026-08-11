---
afx: true
type: SPEC
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["roadmap", "planning"]
---

# cortex-rs Roadmap

> Stable IDs, never renumbered - reference them in commits, PRs, and release notes.
>
> Status legend: `[x]` done, `[~]` in-progress, `[ ]` planned. Future items live under `## Future` and promote to Planned when scheduled.
>
> Completed parent items move to `completed.md`; completed sub-items remain beside unfinished siblings while an active milestone is mixed. `s/progress` counts both files. Once a CHANGELOG exists, this roadmap becomes a pure backlog.
>
> **`[x]` means the code exists and passes the local gate. It does NOT mean hardware-verified.** Anything touching the wire stays provisional until its hardware smoke item passes against a real Quad Cortex.
>
> Hardware-verified so far (through 2026-08-10, CorOS 4.0.1): transport through the implemented typed client; the held daemon, live cache and auto-managed idle release; read, navigation, grid, tempo, STOMP/expression/MIDI, file, capture/IR selection, global-setting and I/O paths; pinning/favourites; physical reconnect; and the native GUI's fail-closed reconnect rendering. Individual items below identify what remains provisional or unimplemented.
>
> **Overnight routing:** `Night: ready` marks a bounded task that can be completed without USB hardware. `Night: slice` permits only the explicitly named offline subset and requires a PR `Hardware follow-up` section where applicable. Unmarked items are not available to the overnight routine. The copy-paste routine prompt and its safety rules live in [overnight-routine.md](overnight-routine.md).

## Next Milestone: Daemon-backed GUI Reads

Connect the existing Tauri first draft to the same held-session contract already proven by CLI and MCP, without widening the write surface:

1. [Done] Write the as-built GUI design and make `cortex-host` the explicit Tauri backend boundary.
2. [Done] Replace fixture-only status, preset directory, live grid, active scene and CPU data with typed daemon snapshots and revisions.
3. [Done] Surface connected/reconnecting/failed state and never render a stale generation as live.
4. [Done] Keep fixture mode as a deliberate development/test adapter rather than hidden fallback behavior.
5. [In progress] Boundary-focused Rust tests, both frontend builds, browser fixture smoke, the real-device dashboard boundary and physical unplug/reconnect pass. Automated native DOM/IPC checks through Tauri MCP remain.

Saving remains outside this milestone. GUI save needs exact-target preparation and confirmation UX, restoration semantics, typed failures and its own hardware smoke. In parallel, CLI-004.4 remains the next distribution milestone: a Linux x86_64 preview containing both `cortex` and `cortex-mcp`.

## Where each ID lives

This file is the single place to see where the project is up to. Each ID names work; the zone spec beside it says what that work must do and how it is designed.

| ID prefix | Zone | What it covers |
| --- | --- | --- |
| PROT-001 | [100-transport](100-transport/spec.md) | USB HID open, read/write, the benign write STALL |
| PROT-002 | [110-framing](110-framing/spec.md) | Report framing, flag-driven reassembly, the trailer envelope |
| PROT-003 | [120-proto-schema](120-proto-schema/spec.md) | Vendored `.proto`, `prost` build, message types |
| PROT-004 | [130-domain-model](130-domain-model/spec.md) | `DeviceKind`, `Row`, `Catalog`, and the typed preset/grid views |
| PROT-005 | [140-session](140-session/spec.md) | Handshake, keepalive, correlation, broadcast waiting |
| PROT-006 | [150-client](150-client/spec.md) | The ergonomic `QuadCortex` API |
| PROT-007 | [150-client](150-client/spec.md) | Capture and IR export/import (investigation) |
| CLI-00x | [200-cli](200-cli/spec.md) | The `cortex` command surface |
| MCP-00x | [300-mcp](300-mcp/spec.md) | The MCP server and its safety boundary |
| GUI-00x | [400-gui](400-gui/spec.md) | The Tauri desktop app |
| DOCS-00x | - | The documentation site and the agent-facing model reference |
| ENG-001 | [500-dx-tooling](500-dx-tooling/spec.md) | Scripts, lint, test |
| ENG-002 | [600-ci-release](600-ci-release/spec.md) | CI and release |
| ENG-003 | [900-project-governance](900-project-governance/spec.md) | Licensing, attribution, legal hygiene |
| ENG-004 | [001-overview](001-overview/spec.md) | Traceability |
| ENG-005 | - | `s/usb-trace`, for observing the official client on the wire |

!!! tip "Check [prior-art.md](prior-art.md) before implementing a protocol item"

    Many unticked PROT items already have an implementation, captured wire shape, or documented investigation in `pyquadcortex`, and several carry silent-failure traps that cost that project real time. [prior-art.md](prior-art.md) records what the prior-art projects know that we do not, the verification level of those findings, and which negative results are worth re-testing. Items below cite it where it applies.

!!! note

    Progress is tracked HERE and nowhere else. Zone folders hold `spec.md` (what it must do) and `design.md` (how it is built and why), but no `tasks.md` - see [001-overview/spec.md](001-overview/spec.md#progress-tracking) for why that convention was dropped.

## Protocol and Crate (PROT)

The bottom-up port of the Cortex Control USB HID protocol from `pyquadcortex` (MIT) into a Rust leaf crate.

### PROT-006: Client API (zone 150)

The ergonomic `QuadCortex` struct - the Rust equivalent of pyquadcortex's 60+ methods.

!!! warning "Residual verification for PROT-006.11 and PROT-006.12"

    These two items remain partial only for the exact hardware and visual checks named below. Their implemented operations and the completed PROT-006.7-.10 and PROT-006.13-.15 work retain per-operation evidence in [completed.md](completed.md) and [prior-art.md](prior-art.md#pyquadcortex---the-one-to-check-first); do not promote a residual from offline coverage alone.

    Two traps apply throughout. **A nested submessage write replaces the whole submessage** - send one flag of a group and its siblings go false, which in one upstream case quietly stopped the Master Volume knob governing most outputs. Read, merge, write. And **the action field is load-bearing**: some operations need `CREATE`, some `READ`, some no action at all, and the wrong one is ignored in silence rather than refused.

- [~] **PROT-006.11**: Capture and IR selection are HARDWARE-VERIFIED on CorOS 4.0.1. Capture selection preserved the exact reference and a post-selection parameter; user-IR selection preserved the exact key/name, and a timed on-unit inspection confirmed no warning. Both working-copy tests restored by recall. The safe residual replaces the public Boolean dialog footgun with a false-only decline flow: it waits for `try_to_show_dialog: true` before responding, never exposes true, and has exact offline race/shape/rejection/continuation coverage. **Exact residual:** run the ignored operator-assisted hardware test to verify that false is a graceful unsupported response, produces no capture state/progress/A-B preparation, leaves the session healthy, and does not strand the disconnected on-unit fallback. This is unsupported-feature handling, not host capture support. Positive v1/v2 host capture moved to PROT-006.17; payload transfer remains PROT-007
- [~] **PROT-006.12**: The automated restoration-first General Settings, Master Volume assignment, global Cab/IR bypass, Global EQ, mode/cycle and Tuner settings test passed in full on CorOS 4.0.1, including complete final baseline restoration. Value 9 and every power/reset/updater/internal-clock shape remain structurally absent. **Exact residual:** Gig View and show/hide Tuner are visual-only methods with no readable baseline and have not been visually verified; do not claim either UI method hardware-verified
- [~] **PROT-006.16**: Hardware smoke coverage is expanded through every implemented non-UI PROT-006 path on CorOS 4.0.1. The 2026-08-10 runs added wider reads; row controls; all eight tempo/metronome methods; STOMP/expression and persistent MIDI; generated-setlist file composition; capture and user-IR selection; automated global settings; the complete I/O matrix and mutations; and pin/Favorite restoration. All mutable runs restored their baselines and cleaned generated storage. **Exact residuals:** the false-only capture-dialog decline and visual-only Gig View and Tuner visibility methods remain unverified, so this umbrella item stays partial
- [ ] **PROT-006.17**: Implement the positive Neural Capture host workflow only with a complete v1/v2 host UI and explicit end-to-end design. Until then `show_dialog: true` remains structurally unavailable from the public client and absent from CLI, MCP and GUI. Establish the v1 `NeuralCapture` and v2 `NeuralCapture2` state machines, cancellation/recovery, calibration, A/B, progress, errors and save behavior before exposing acceptance; do not infer `show_dialog_fail_reason` semantics from its field name

### PROT-007: Capture and IR export / import

Neural Captures and user IRs live only on the unit and in Neural's cloud. No existing tool - official or community - can export a capture to a local file, so a player's own captures cannot currently be backed up, version-controlled, or moved between units. They are the player's own data and remain a legitimate interoperability target, but export/import is lower priority than completing the Linux control surfaces.

Status: **deferred, not a release blocker.** Linux CLI, MCP and GUI coverage over the live wire protocol takes priority; this feature should not consume implementation time or block those surfaces. Resume only after the primary Linux product is complete or when a concrete user need justifies the encryption work. The investigation remains recorded: a Cortex Control 4.0.1 trace proves that CorOS 4.0.1 returns a complete opaque whole-unit backup through chunked `LocalBackup{UPDATE}` messages after a `LocalBackup{CREATE}` request. Its high-entropy payload is consistent with encryption but the cipher remains unestablished. The trace does not prove that one capture can be extracted or that the backup contains every capture and IR. The file category is independently solved: `FileMessage.type` is 0 for presets, 1 for IRs, and 2 for captures. `FileMessage` carries `ir_payload` and `preset_payload` `bytes` fields, but no per-capture payload read has been observed.

!!! warning "Known constraints before tracing"

    Upstream hardware work mapped a candidate IR-import request, established that `total_bulk_create_count` is mandatory, and round-tripped an outbound write spanning 26 HID reports. Its imports still produced no IR, so payload content, action choice, and other request semantics remain unknown; outbound framing itself is proven. Repeated multi-kilobyte attempts coincided with the USB link dying until a power cycle; space destructive probes out ([prior-art.md](prior-art.md#on-captures-and-irs-prot-007)).

    Separately, reference-only pre-CorOS-3 prior art without a repository-wide licence reports captures as encrypted protobufs and local captures as serial-number-keyed. That is a provisional lead to test, not source material or a current-format fact; if true, cross-unit import may be impossible without a portable representation ([prior-art.md](prior-art.md#opencortex---one-high-value-fact-and-a-minefield)).

- [~] **PROT-007.1**: A complete whole-unit backup is HARDWARE-MEASURED over `LocalBackup` on CorOS 4.0.1. Cortex Control sends `CREATE` with only a request id; the device returns ordered `UPDATE` chunks carrying JSON text and an explicit final-chunk flag. One backup delivered 1,016,596 JSON bytes in seven messages over 8,072 HID reports. **Exact residual:** establish whether the opaque backup actually contains every Neural Capture and user IR, or observe a separate per-item payload route; whole-backup transport alone does not prove individual capture export
- [~] **PROT-007.2**: The outer container is identified: concatenated `LocalBackup.backup_json` is a metadata JSON object with a Base64 `payload` and 64-hex `payload_hash`. The measured Base64 decoded to 762,184 opaque bytes with no gzip/archive signature, almost no printable text and 7.999772 bits/byte Shannon entropy, consistent with encryption. The documented pre-CorOS-3 no-serial decrypt path did not decode this CorOS 4.0.1 payload. **Exact residual:** establish the current cipher and key scope, validate `payload_hash`, decrypt the content, then determine whether compression exists inside the encrypted layer
- [ ] **PROT-007.3**: `export_capture(key, path)` - write one capture to a local file
- [ ] **PROT-007.4**: `import_capture(path)` - write a capture back to the unit. **Destructive; gate behind confirmation and the MCP safety surface.** Do not assume a capture exported from one serial can be imported by another until the encryption lead is resolved
- [ ] **PROT-007.5**: Same for user IRs. First establish a safe `list_irs` completion rule, then start from the upstream candidate import request and its mandatory bulk-count field; do not repeat failed payload encodings without new evidence
- [ ] **PROT-007.6**: `cortex capture export` / `import` CLI surface
- [ ] **PROT-007.7**: Decide and document a container format. Prefer something self-describing that records the source unit, CorOS version, and capture metadata, so a file is still meaningful years later

Do not ship import before export has been round-tripped on hardware: writing a malformed capture to the unit is the most plausible way this project could damage a user's data.

### PROT-009: Correctness gaps found in review, 2026-08-05

Raised by a review of the state-cache/prepared-save/daemon-routing change (`a603bce`). The tree was green - fmt, clippy, `reuse`, and 149 tests all passed - so every item here is a behavioural gap that the existing tests did not reach. All claims have now been checked; completed items state their verification level.

Ordered by what would hurt a user most.

- [~] **PROT-009.14**: **Regression coverage.** Offline coverage now includes explicit link-drop with retained session references; exclusivity-aware responsive-gap reconnect; prepared-save phase matrix and generation/storage epoch; concurrent public-flow File list/save/delete waiters with wrong targets and reply-ordered completion; exhaustive malformed 256-entry listing rejection (missing, negative, 256+, duplicate, wrong action/type/folder and combined malformed shapes); recall/edit/save ordering; stale-empty target backup; partial File invalidation; malformed envelopes; spawned older/newer daemon protocol skew with cross-version shutdown; parameter kinds and real-unit conversion. Tests and protocol docs pin two irreducible limits without claiming a fix: delayed acknowledgements from an earlier identical File operation are wire-indistinguishable because no usable request ID exists, and the final listing-check/write interval has no revision or compare-and-swap guard. No `File SWAP` is implemented absent evidence. **Exact residual:** hardware confirmation that a naturally occurring malformed report or envelope triggers replacement-handshake recovery and a healthy next request. Do not inject malformed traffic into hardware; if no natural fault is observed, retain this as unexercised. Rerun strict concurrent File operations on hardware only if future CorOS behavior suggests broadcasts differ from the measured shapes. Parent remains partial because its original acceptance included hardware fault recovery

## Docs (DOCS)

### DOCS-002: Agent manual - factory preset reference

A per-device, per-CorOS reference of the factory presets: what each is modelled on, and how to get a usable sound out of it. Aimed at agents driving the MCP server, who otherwise have no idea that "Brit 2203" is a Marshall-style voicing.

- [ ] **DOCS-002.1**: Generate the raw preset inventory from the device rather than transcribing it - `cortex preset list --setlist "/opt/neuraldsp/Factory Library"` already emits all 256 names. Keep it a build step so a CorOS update regenerates it
- [ ] **DOCS-002.2**: Key the reference by device AND CorOS version; factory content changes between releases and a stale mapping is worse than none
- [ ] **DOCS-002.3**: Setup tips per preset - the genuinely additive part, since the gear mapping is no longer ours to write (see below)
- [ ] **DOCS-002.4**: Expose it to the MCP server so an agent can resolve intent ("something like a cranked Plexi") to a slot

**This item shrank substantially on 2026-08-02.** The device's own catalog carries a `tm` attribute holding **Neural DSP's own attribution** for each model - `Based on Marshall(R) JCM800(R)`, `Based on ProCo(R) Rat(R)`, `Based on Universal Audio(R) 1176(R)` - on 318 of 533 models. So the "what is this modelled on" mapping does not need writing at all: it ships with the unit, in the vendor's own carefully-worded form, and `cortex catalog --search marshall` already surfaces it.

That changes the plan in three ways:

1. **Never paraphrase the `tm` string.** Reproduce it verbatim. It is Neural DSP's statement about other companies' marks; rewording it, or presenting our own mapping as authoritative, is both less accurate and less defensible. The crate surfaces it as `Model::based_on` and the CLI prints it unchanged.
2. **The remaining work is genuinely additive only** - setup tips, and mapping *presets* (which the `tm` data does not cover) to the *models* they contain, which the catalog does let us resolve.
3. **This is a runtime lookup, not a document to write.** An agent can already resolve "something like a cranked Plexi" by searching the live catalog, which stays correct across CorOS updates for free.

Remaining constraints:

- **Trademark caveating is still mandatory for anything WE write.** Fender, Marshall, Mesa/Boogie, Vox and the rest are other companies' marks. Follow the industry norm every modelling vendor and community site uses: describe what a preset is *evocative of*, never imply endorsement, licensing, or that the model IS the amp. Neural DSP's own naming is deliberately oblique ("Brit 2203", not "Marshall JCM800") and our docs should not undo that by publishing a decode table presented as authoritative.
- **Preset NAMES are factual interoperability information; preset DATA is Neural DSP's.** Listing names and our own commentary is fine. Committing extracted factory preset payloads into this repo is not - see the legal hygiene section of AGENTS.md.

## CLI (CLI)

The `cortex` command-line surface over the crate.

### CLI-002: Format and output

- [ ] **CLI-002.3**: `--schema` / `--print-schema` - JSON Schema of a command's inputs

### CLI-003: Preset and scene commands

The implemented preset/scene command surface is hardware-verified against CorOS 4.0.1. `scene switch|label|unlabel|color|copy|swap` use noun-then-verb naming; the shipped `scene --index` switch shorthand remains accepted.

- [ ] **CLI-003.9**: `cortex capture` / `cortex ir` - export and import (blocked on PROT-007)

### CLI-004: Distribution

**Decision (2026-08-07):** the primary user channel is a Linux x86_64 preview containing both `cortex` and `cortex-mcp`, installed without Rust or `protoc`, followed by one guided udev and agent-harness setup. Local stdio remains the MCP transport and the existing daemon remains the sole USB owner. Do not add a remote MCP service or systemd dependency. Homebrew/crates.io are secondary developer channels; add a `.deb` only after the release archive has user evidence, because package-level udev installation is its main additional value.

- [ ] **CLI-004.3**: crates.io publish workflow (PROT layers must be complete first)
- [ ] **CLI-004.4**: Linux x86_64 cargo-dist release pipeline. Publish `cortex` and `cortex-mcp` together as the supported product surface, attach `SHA256SUMS`, licences/notices and the udev rule, and expose a checksum-verifying `install.sh` from the docs-site root. The release workflow must be `workflow_call`-able from `auto-tag.yml`; generated actions must be replaced with verified SHA pins. Add Linux aarch64 only after build and HID behaviour are verified; defer macOS/Windows until their local IPC, process lifecycle and hardware paths are implemented. **Night: slice.** Configure and test the non-publishing pipeline in a PR after reading house-style distribution/CI standards; do not tag, release, publish, or merge
- [ ] **CLI-004.9**: Guided `cortex setup` for non-developers. Diagnose architecture, USB presence, udev installation/effective hidraw access, Cortex Control contention, daemon health and MCP registration. Support Claude Code first with an absolute stdio command; print harness-specific configuration for others. Privilege escalation must be explicit and narrowly scoped
- [ ] **CLI-004.10**: Test the published install path in a clean Linux environment: checksum verification, both binaries on `PATH`, completions, MCP process discovery, upgrade replacement of both binaries, and actionable no-device/udev diagnostics
- [ ] **CLI-004.11**: Add a `.deb` only after user demand is established. Its purpose is to install both binaries and the udev rule in conventional system locations; RPM remains demand-led rather than default scope
- [~] **CLI-004.12**: Keep the Windows door open. Done: `cortex-host` now exposes transport-neutral `LocalEndpoint`/`LocalListener`/`LocalConnection`; Unix socket paths, stale cleanup and owner permissions live only in its Unix adapter; daemon and MCP tests use the facade; `cortex-host` and `cortex-mcp` cross-check for `x86_64-pc-windows-gnu`. Outstanding: choose a maintained safe named-pipe dependency, implement current-user pipe ACLs and duplex byte streams, abstract detached process creation, then hardware-test the HID/session path on Windows
- [ ] **CLI-004.13**: Add changelog generation before the first public release. Resolve whether `git-cliff` or cargo-dist owns release notes, generate `CHANGELOG.md` in the version-bump flow, and make release automation consume that one source

### CLI-007: Shared machine contract

- [ ] **CLI-007.1**: Add a typed command/input registry and `cortex --schema` output shared by CLI and MCP. MCP currently owns explicit bounded JSON Schemas because the CLI contract promised by MCP NFR-3 does not yet exist; migrate both surfaces to one registry rather than allowing those schemas to drift. **Night: ready.** Keep one registry shared by both surfaces without pulling host/runtime concerns into `cortex-rs`, preserve bounded schemas, and make this satisfy CLI-002.3 rather than adding a second schema implementation

## MCP (MCP)

The `cortex-mcp` MCP server for agentic patch editing. Its first non-persistent slice is implemented and hardware-verified through the held-session daemon; no save or delete tool is exposed.

### MCP-001: Safety surface

- [ ] **MCP-001.1**: Read and recall are free; saving is always explicitly confirmed
- [ ] **MCP-001.2**: Never write to the factory setlist; require one explicitly named USER target (1A-32H) and authorise only that target for the prepared operation
- [ ] **MCP-001.3**: Prepare every target and retain any backup **before working-copy edits begin**. A listing cannot prove emptiness because storage is eventually consistent; calling `read_preset` after edits recalls the target and destroys the grid being saved
- [ ] **MCP-001.6**: Implement and hardware-verify a restoration path before describing retained backup bytes as rollback. Candidate routes are device-side copy/import or keyed replay; an unkeyed whole-preset write is known to do nothing

### MCP-003: Show what the MCP server is for

The reason this project has an MCP server at all is that an agent can do things no editor UI can: take a plain-English brief, research what it means, and build the preset. That has to be demonstrated, not asserted.

- [ ] **MCP-003.1**: A worked demo - a brief like "a basic 1987 GnR Slash tone" taken through research, model selection and grid construction to a working tone. **Decided:** build into the LIVE grid and stop short of saving, starting from an empty preset. That needs no save confirmation, is reversible by recalling, and still shows the capability. **The agent researches the web live rather than following a fixed recipe** - the research is the part worth demonstrating, and a canned recipe would show nothing an editor cannot already do. The demo therefore will not reproduce byte-for-byte, which is accepted.
- [ ] **MCP-003.2**: A second demo for the reverse direction - "why does this preset not fit" - using the per-core CPU breakdown, which is a question the official editor answers poorly. The useful answer is not just a number but a strategy: which blocks could move to the other core, row or column.

### MCP-002: Tool surface

- [ ] **MCP-002.4**: Destructive tool: `save_preset`. Core PROT-009 correctness blockers and typed daemon failures are closed; remaining work is an MCP-held exact-target preparation-token registry, explicit confirmation, restoration semantics and an MCP save hardware smoke
- [ ] **MCP-002.7**: Replace raw routing port integers with typed input/output enums and add post-write read-back for bypass, remove, routing, split and parameter writes. The device accepts meaningless output IDs silently, while those daemon acknowledgements currently prove only that a write was sent. Measured 2026-08-10: immediately after `set_bypass`, a cache-backed grid read returned the old value; the explicit full read before a subsequent block move observed the new value, proving dispatch succeeded but cache convergence had not

## GUI (GUI)

The typed read/edit, live-cache, health, reconnect, and shared prepared-save foundations are in place. The interactive read-only Tauri first draft now has explicit fixture and daemon-backed modes with generation-checked status, grid, scene, CPU and populated preset directory reads. Its production Rust boundary and physical unplug/reconnect behavior have passed against a real held session in the native Linux window; automated Tauri DOM/IPC checks remain outstanding. Save controls remain disabled until the GUI implements exact-target preparation and confirmation UX, restoration semantics, typed failures and its own write-path hardware smoke. See [400-gui/spec.md](400-gui/spec.md).

**The prepared-save API is now enforced by the daemon and CLI.** Fixed in `743692a` (PROT-009.2): the daemon holds exact-target preparations under opaque tokens, and the CLI is two-phase with confirmation. A GUI that wants target backup and revalidation should call the same API.

The visual design goal is a **hardware-faithful rendering of the Quad Cortex front panel** - 10 footswitch/encoder positions, the colour OLED grid, scene LEDs, and the context strip - with wrapper panels (patch browser, block palette, parameter inspector, scene manager, IR/capture loader) alongside. Use Tauri MCP to tighten the feedback loop during GUI development.

### GUI-001: Scaffold and Tauri MCP

- [ ] **GUI-001.3**: Wire Tauri MCP for the dev feedback loop - drive the GUI from the MCP server to test Tauri commands without manual clicking
- [ ] **GUI-001.4**: Remove the temporary `RUSTSEC-2024-0429` audit exception when stable Tauri moves its Linux runtime from the unmaintained GTK3/glib 0.18 stack to GTK4/glib 0.20 or later. Tauri 2.11.5 is the latest stable release and still pins GTK3; upstream migration is tracked in [tauri-apps/tauri#12562](https://github.com/tauri-apps/tauri/issues/12562) and [PR #14684](https://github.com/tauri-apps/tauri/pull/14684). Do not force two incompatible glib generations or ship the GUI from an unreleased Tauri branch
- [~] **GUI-001.6**: Rust boundary tests cover readiness, Rust-owned scene conversion and no fixture fallback; both strict TypeScript modes build; browser fixture interaction was inspected at 800x600 and 1280x800. The production dashboard test passes against a real CorOS 4.0.1 held session. HARDWARE-VERIFIED 2026-08-09: unplugging hid the live grid and directory within one second while generation 1 became invalid; automatic recovery restored the same eight-block preset after about 10 seconds under generation 2. A second run exposed attempt/error details, accepted **Reconnect now** at attempt 4 and restored generation 2 in about three seconds; an offline timing test proves the signal interrupts a 10-second wait in under one second. Outstanding: use Tauri MCP from a client that exposes it for automated DOM/IPC checks

### GUI-002: Hardware-faithful control surface

- [ ] **GUI-002.1**: Render the Quad Cortex front panel: 10 footswitch/encoder positions, OLED grid, scene LEDs, context strip
- [ ] **GUI-002.2**: Footswitch interaction: click-to-press (toggle bypass / recall / navigate), drag-to-turn / scroll (adjust parameter), keyboard equivalents
- [ ] **GUI-002.3**: Mode-aware footswitch labels - reflect the current device mode (Preset / Stomp / Scene / Looper / Tuner)
- [ ] **GUI-002.4**: The OLED grid mirrors the device's live state (signal chain, block icons, bypass, active scene) from the crate's read paths
- [ ] **GUI-002.5**: Honest state - render what the device reports, not what the GUI thinks it sent

### GUI-005: Always-visible preset directory and CPU load

Stack confirmed as Tauri 2 + React + Mantine, as AGENTS.md says, unless a concrete reason to change appears.

Improve on the Cortex Control appearance while staying familiar enough to navigate without relearning.


### GUI-006: Screen-reader accessibility

[OpenCortex issue #10](https://github.com/VanIseghemThomas/OpenCortex/issues/10) records the concrete need for complete, independent Quad Cortex editing by a blind musician because the official visual editor does not expose a usable screen-reader surface. Treat accessibility as an architectural constraint from the first GUI scaffold, not a later audit. The hardware-faithful panel is one visual presentation of the domain model, never the only route to a control; keyboard equivalents alone are not acceptance.

- [ ] **GUI-006.1**: Build the interaction model from semantic controls. Every control exposes an accessible name, role, current state, value, units, and available action; the signal chain has an ordered nonvisual representation with explicit row, column, routing, block type, and bypass state. Nothing depends only on colour, position, pointer drag, hover, or an icon
- [ ] **GUI-006.2**: Provide screen-reader feature parity across the whole editing surface: browse and recall presets; inspect, place, move, replace, bypass, and remove blocks; edit every parameter in real units; manage scenes, routing, I/O and global settings; and review and confirm saves. Do not ship a reduced "accessible mode"
- [ ] **GUI-006.3**: Make asynchronous device behaviour understandable nonvisually. Announce device-originated state changes, command completion, refusals and silent-failure safeguards without flooding the screen reader; keep focus deterministic across dialogs, live updates and destructive confirmations; expose undo/dirty state wherever the protocol supplies it
- [ ] **GUI-006.4**: Make accessibility part of the release gate: automated semantic/accessibility checks plus manual end-to-end runs with actual screen readers and blind users. Establish Orca on the supported Linux webview as the Linux-first baseline during GUI scaffolding, then add NVDA and VoiceOver when Windows and macOS builds become supported; record the tested app, webview and screen-reader versions

### GUI-003: Wrapper panels

- [ ] **GUI-003.1**: Patch browser - setlist/slot grid for quick preset switching, with search and favourites
- [ ] **GUI-003.2**: Block palette - searchable list of available models from the `Catalog`, drag onto a grid cell
- [ ] **GUI-003.3**: Parameter inspector - form-based editor for the selected block's parameters, showing real units (dB, ms, Hz) via catalog range conversion
- [ ] **GUI-003.4**: Scene manager - copy/swap/relabel/recolor scenes without the footswitch mode dance. **Night: slice.** Build one typed non-persistent scene-manager interaction against the fixture/Tauri API boundary, preserve zero-based API vs A-H display, and request next-day hardware read-back
- [ ] **GUI-003.5**: IR / Capture loader - file-browser-style access to the device's captures and IRs

### GUI-004: Safety surface and governance

- [ ] **GUI-004.1**: Reuse the MCP safety surface (factory refusal, exact target, pre-edit target preparation/backup, explicit confirmation, trap-surfacing) for save actions. If an occupied target was not prepared before the grid became dirty, require the user to choose and prepare another target rather than recalling it and losing the edits
- [ ] **GUI-004.2**: Label hardware-verified vs provisional surfaces in the UI. Model this as a typed capability matrix (`confirmed-readable`, `confirmed-writable`, `inferred`, `unsupported`, `unverified`) whose default is tested to make no unsupported claims, rather than as ad-hoc copy ([prior-art.md](prior-art.md#the-idea-most-worth-stealing))
Version synchronization is completed and tracked canonically under ENG-001.2.
- [ ] **GUI-004.4**: `docs/gui/` explains how to use and run the GUI

## Engineering (ENG)

### ENG-001: DX and testing

- [ ] **ENG-001.3**: `s/install-hooks` and `.githooks/pre-commit`. **Night: ready.** Follow the house-style hook installer and keep installation explicit
- [ ] **ENG-001.4**: Markdown lint. **Night: ready.** Use the house-style maintained tool/config and avoid reflowing existing prose as drive-by cleanup
- [ ] **ENG-001.5**: Close the documented local/CI parity gap where practical: no-default workspace clippy/tests locally, while keeping platform cross-checks CI-only when native toolchains are unavailable. **Night: ready.** Prefer extending canonical `s/test`/`s/lint` over adding another script
- [ ] **ENG-001.6**: Add a CI check that Cargo, npm package/lock and Tauri versions match outside `s/version++` runs. **Night: ready.** Reuse the version source/order already encoded by `s/version++`

### ENG-002: CI

Release workflows live under CLI-004; this section tracks non-release CI gaps only.

- [ ] **ENG-002.2**: Add frontend `npm run check` and a Tauri build boundary to CI. Add native Windows/macOS jobs only as those hosts become supported. **Night: ready.** Audit the existing partial frontend check first, add only the missing boundary, and use verified SHA-pinned Actions

### ENG-003: Governance

- [ ] **ENG-003.3**: If a closed derivative ever needs to exist, add `DUAL-LICENSE.md` and the boilerplate. Requires approval
- [ ] **ENG-003.4**: SECURITY.md and CONTRIBUTING.md before the repo is public-facing
- [ ] **ENG-003.5**: If we ever target on-device builds, adapt `qc-stomp-tools` (MIT) with attribution and a NOTICE entry

### ENG-004: Traceability

- [ ] **ENG-004.1**: Add `@see` traceability headers to all owned source files linking to zone specs. **Night: slice.** Cover one complete zone per PR and leave the parent planned until every owned source file is covered
- [ ] **ENG-004.2**: CI gate for `@see` link resolution (optional, low priority). `deskop-nano-cortex/scripts/check-traceability.mjs` is a working reference: resolve document and node ID, fail broken links, warn missing back-references, and include an explicit config/workflow file list ([prior-art.md](prior-art.md#for-eng-004-and-eng-001))

### ENG-005: `s/usb-trace` - observe Cortex Control on the wire

A script that sets up passive USB observation of the official Cortex Control app driving the device, so its traffic can be decoded against our schema. This is the tool for questions of the form "how does the official client do X" - the answer to which is evidence about the wire, not inference about intent.

**It paid for itself before Cortex Control was ever traced.** The first capture was of our OWN client, run only to check whether the device's apparent silences were real or an artefact of our RX path. It showed the bus idle for 46.8 s in the middle of a handshake, with the device answering every write in 217 us - which identified writer starvation in `session.rs` (PROT-008.5, PROT-008.6.10) after a full day of attributing that variance to the device. Two features had already been built and withdrawn on the strength of the wrong explanation. Trace our own side first; it is cheap, needs no VM, and the bug is as likely to be ours.

**Named `usb-trace`, not `usb-record` or `usb-capture`, deliberately.** "Capture" already means a Neural Capture in this domain and "record" implies audio; either would suggest this script records sound, which it emphatically does not. `trace` is already the project's word for protocol observation (`CORTEX_TRACE`), so the two read as the same idea at different levels.

The method is documented in the [protocol reference](../docs/protocol.md#observing-the-wire): with the QC passed through to a Windows VM under QEMU, the **host** kernel still sees the traffic. So `modprobe usbmon` plus a capture of the relevant `usbmonN` interface on the Linux host records everything, without needing USBPcap inside Windows and without the macOS exclusive-access problem.

- [ ] **ENG-005.4**: Optionally a Wireshark Lua dissector, since Wireshark's built-in protobuf support can be pointed at our vendored `.proto` files - worth it only if the GUI earns its place alongside `s/usb-decode --live`
- [ ] **ENG-005.3**: Runbook: what to do in Cortex Control while tracing to answer a specific question, starting with capture export (PROT-007.1)

**Known obstacle.** The QEMU/Windows/Cortex Control setup on the development machine works but drops its connection regularly, so it is adequate for short targeted observations and not for sustained work. Plan traces as single short scripted actions - "open the app, export one capture, stop" - rather than long exploratory sessions, and expect to repeat them.

**Do not commit raw captures.** They contain readable preset, path, device, and build strings. Commit decoded findings in our own words, as the prior art does. See the legal hygiene section of AGENTS.md.

### ENG-006: `s/hardware-smoke` - scripted CLI smoke test with read-back assertions

- [~] **ENG-006.2**: Daemon start/status/stop and routed read/edit/save paths are scripted and hardware-verified. Physical unplug/replug was manually verified on 2026-08-07; only a safe automated reconnect substitute remains optional

## Future

- **FUTURE-001**: Nano Cortex hardware verification - third-party macOS observation records provisional VID:PID `152A:88E7` and 65-byte HID reports, not the Quad Cortex's 129. Its HID interface opened but emitted no passive reports; nobody has shown it speaking this protobuf/trailer protocol. Plug in a Nano, verify the transport and handshake rather than assuming a shared shape, then replace the `0xFFFF` sentinel and promote `DeviceKind::NanoCortex` only if the evidence supports it ([prior-art.md](prior-art.md#what-it-says-about-the-nano-cortex-and-one-contradiction))
- **FUTURE-002**: Nano Cortex BLE protocol - the Nano uses BLE for control telemetry; `deskop-nano-cortex` has a provisional decode (Apache-2.0) whose field map credits `choldy/nano-cortex-web-editor` (MIT). Any adaptation carries both attributions ([prior-art.md](prior-art.md#an-additional-project-not-vendored))
- **FUTURE-003 (promoted)**: Cross-platform GUI completion is now active under GUI-001 through GUI-006 and CLI-004.12; preserve this ID as the original umbrella rather than counting it as a separate future item
- **FUTURE-004**: On-device builds (qc-stomp-tools ioctl route) - only if there is a compelling reason; the USB route is preferred
- **FUTURE-005**: Protocol-version probe - surface a CorOS version check rather than hard-coding assumptions, since the protocol has no version field on the wire
- **FUTURE-006**: Conformance suite - port pyquadcortex's offline test suite as a Rust integration test reference
- **FUTURE-007**: Audio feedback loop - let the MCP server "hear" the unit. The Quad Cortex presents class-compliant USB **audio** interfaces that are separate from the HID interface we use, so a host could play a standardised stimulus (DI guitar phrase, sine sweep, impulse) through the chain and capture the processed result **without contending for the exclusive HID connection**.

  Confirmed on hardware 2026-08-02: of the unit's six USB interfaces, **0 through 4 are Audio class (class 1) and only interface 5 is HID (class 3)**. ALSA already enumerates the device as a working card (`USB-Audio - Quad Cortex`) with no driver work required, so the capture side of this needs no reverse engineering at all - it is an ordinary audio device that happens to also speak our HID protocol on a different interface. Comparing captured output against a dry reference would characterise what a chain is doing to the signal.

  Worth being precise about what this buys, because it is not what it first appears. An agent editing a patch already has **ground truth** available: `read_current_preset` returns the actual grid, so "did my edit land on the right block" is answerable today by read-back, and audio analysis is a strictly worse way to answer it. What audio adds is **aesthetic and perceptual judgement** - "is this too dark", "is the gain staging sensible", "does this sound like the reference tone" - which read-back cannot answer at all.

  So this is not a correctness or safety mechanism and should not be treated as one; it is what would let an agent iterate on *tone* rather than on *structure*. That is genuinely novel and nobody has built it, but it is a substantial subsystem (audio I/O, latency alignment, feature extraction, a perceptual similarity metric) and it should not start until the grid-edit surface it would be judging actually exists.

  Open questions: does the QC expose a usable dry/wet split over USB (there is a `dry_wet` field in `USBPortSettings`) so a dry reference can be captured simultaneously rather than in a separate pass; and what stimulus set is both compact and discriminating enough to be worth standardising.
