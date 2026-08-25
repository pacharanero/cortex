---
afx: true
type: SPEC
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["overview", "cortex-rs", "rust", "usb-hid", "quad-cortex", "nano-cortex", "traceability"]
---

# cortex-rs - Project Overview

> Governing spec. Defines the spec taxonomy, traceability rules, and the routing index that maps every owned source surface to the zone spec that documents it.

## References

- [Public protocol reference](../../docs/protocol.md) - the authoritative, repository-local protocol facts.
- [Prior-art map](../prior-art.md) - what each reference project knows and where to read it.
- [pyquadcortex protocol docs](https://github.com/stokes-audio/pyquadcortex/blob/main/docs/protocol.md) - the MIT-licensed Python reference implementation.
- [deskop-nano-cortex spec tree](https://github.com/rixrix/deskop-nano-cortex/tree/main/docs/specs) - the AFX spec convention this tree mirrors.
- [AGENTS.md](../../AGENTS.md) - agent instructions and protocol invariants.
- [README.md](../../README.md) - setup and project overview.

## Problem Statement

cortex-rs is an unofficial, Linux-first Rust toolkit for the Neural DSP Quad Cortex and Nano Cortex. The core deliverable is a low-level leaf crate that speaks the Cortex Control USB HID protocol and exposes honest device-specific domain models: presets, grids, blocks and scenes for the Quad; a fixed signal chain for the Nano. A shared `cortex-host` daemon IPC boundary serves three host surfaces: the hardware-verified `cortex` CLI; the hardware-verified, non-persistent `cortex-mcp` server; and a Tauri desktop GUI with explicit fixture and daemon-backed modes plus non-persistent working-state edits. The GUI target is cross-platform, while Linux is the only verified host today.

The project is a Rust port of the protocol behaviour established by the MIT-licensed `stokes-audio/pyquadcortex` Python library, with the implemented core paths re-verified against a real Quad Cortex on Linux running CorOS 4.0.1. It is not affiliated with or endorsed by Neural DSP.

The spec tree must stay a living, 1:1 map of the as-built code so that a future agent making a surgical change can find the owning zone spec, its owned files, and its tests before reading implementation code.

## User Stories

### Primary Users

Maintainers, AI coding agents, and downstream crate consumers.

### Stories

**As an** AI agent
**I want** to resolve any source file to its governing spec via a `@see` link
**So that** I can change behaviour from the right living document instead of grepping the tree.

**As a** crate consumer
**I want** to build `cortex-rs` with `default-features = false` and get a pure protocol/domain decode surface with no HID dependency
**So that** I can embed it in analysis tools, tests, or a different transport.

**As a** CLI user
**I want** `cortex device version` to read the real device firmware over USB
**So that** I can confirm the device is talking and the protocol version is supported.

**As a** maintainer
**I want** spec folders numbered by category with insertion gaps
**So that** new zones slot in without renumbering existing specs.

## Requirements

### Functional Requirements

| ID | Requirement | Priority |
| --- | --- | --- |
| FR-1 | Every owned Rust source file (`crates/**/*.rs`, `gui/src-tauri/**/*.rs`) carries a top-level `@see` doc-comment linking to its governing zone. Settled under ENG-004.1: the TypeScript frontend (`gui/src/`), the `s/` scripts, and configuration files (manifests, CI workflows, build/lint config) are explicitly out of scope for `@see` - they are governed instead by the SPDX header requirement (zone 900, FR-2) and each zone's `spec.md` "Owned source" line, which already names them without an `@see` link. | Must Have |
| FR-2 | Spec folders use 3-digit ranged numbering by category (see Appendix), spaced to allow insertion without renumbering. | Must Have |
| FR-3 | Each source surface has one primary owning zone; the routing index below is the authoritative owner map. Cross-cutting files may be consumed by several zones. | Must Have |
| FR-4 | Node IDs are zone-local: `[FR-x]`/`[NFR-x]` restart per `spec.md`, `[DES-*]` anchors are unique within a `design.md`. | Must Have |
| FR-5 | Project progress is tracked only in `spec/roadmap.md` and `spec/completed.md`; zone folders contain no `tasks.md`. | Must Have |
| FR-6 | Cross-cutting living behaviour uses numbered `900-999` specs; one-off decisions use `docs/adr/` (none yet). | Should Have |
| FR-7 | Code/spec alignment is bidirectional: existing Rust `@see` links resolve (`s/check-traceability`, ENG-004.2), and every zone `spec.md` lists its owned files - Rust source via `@see`, non-Rust surfaces via the "Owned source" line alone (FR-1). | Must Have |
| FR-8 | Provisional surfaces (Nano Cortex specifics, MCP safety surface, unverified message types) are labelled as such in code, spec, UI, and release notes. | Must Have |
| FR-9 | The crate is a leaf: `default-features = false` builds only the protocol/domain surface (no hidapi, no async runtime). | Must Have |
| FR-10 | The same crate drives the CLI, MCP server, and Tauri backend; none reimplements protocol or domain logic. | Must Have |

### Non-Functional Requirements

| ID | Requirement | Target |
| --- | --- | --- |
| NFR-1 | `@see` targets resolve to existing document paths and node IDs. | Enforced in review/CI |
| NFR-2 | Zone specs are scannable before source reading (Agent Entry Map). | Required for agent DX |
| NFR-3 | The tree reflects as-built code, not aspirational future work. | Living-doc invariant |
| NFR-4 | The crate compiles with `cargo build --no-default-features` on a machine with no HID hardware. | CI-enforced |

## Acceptance Criteria

- [x] Every owned `.rs` file resolves to exactly one primary zone and carries a valid `@see`, CI-gated by `s/check-traceability` (ENG-004.1, ENG-004.2). Non-Rust surfaces are explicitly out of scope for `@see` (FR-1).
- [x] Inserting a zone between `100` and `110` uses `105`, never renumbers.
- [x] `001-overview` is the singleton routing/rules doc.
- [x] Provisional surfaces are flagged by capability in code and spec without labelling hardware-verified device support provisional as a whole.
- [x] `cargo build --no-default-features -p cortex-rs` succeeds without HID hardware.

## Non-Goals

- Feature requirements (each zone spec owns its own).
- GUI interaction and presentation requirements (owned by zone `400-gui`).
- Full Nano Cortex support. Hardware verification covers shared USB HID framing, the Nano-specific envelope/domain, held-daemon runtime, typed state, raw amp, bypass and raw FX parameter operations, but wider operations and untested host surfaces remain active work under NANO-001.
- On-device builds (the `qc-stomp-tools` ioctl route; not in scope for this USB-first project).
- Extending `@see` traceability to the TypeScript frontend, `s/` scripts, or configuration files. Settled under ENG-004.1: those surfaces are SPDX-headered (zone 900, FR-2) and named in their zone's "Owned source" line, which is judged sufficient for glue/tooling and UI code that does not carry protocol or safety behaviour. A future change proposing `@see` for one of those surfaces needs its own roadmap item with the full contract (NFR-1 resolution, `s/check-traceability` coverage) rather than a narrower one grafted onto FR-1.

## Dependencies

- The AFX code-traceability convention (`@see spec/<zone>/spec.md [FR-x]`).
- Toolchain + CI conventions live in `500-dx-tooling` and `600-ci-release`.
- The MIT-licensed `pyquadcortex` .proto schema, vendored into `crates/cortex-rs/proto/`.

## Progress tracking {#progress-tracking}

**Progress is tracked in [roadmap.md](../roadmap.md) and [completed.md](../completed.md), and nowhere else.** A zone folder holds `spec.md` (what the surface must do) and `design.md` (how it is built and why). It does NOT hold a `tasks.md`.

The split is presentational, not a second record: `roadmap.md` carries what is outstanding, and a finished item moves verbatim to `completed.md`. `s/progress` counts both, so the totals do not change when something moves.

Finished items are **moved, not deleted**. Many carry the measurement that settled a question - why the keepalive is 1 s, why `CPULoad` is asked for with `CREATE`, what the handshake actually costs - and those are exactly the entries needed again when something regresses or an outside contributor asks why.

This is a deliberate divergence from the AFX layout that `deskop-nano-cortex` uses and that this tree was modelled on. It is recorded here rather than left as silent drift.

### Why

The project ran both a per-zone `tasks.md` and a stable-ID roadmap for a while, and the duplication did not survive contact with the work:

- `001-overview/tasks.md` claimed no zone spec and no roadmap existed, long after all thirteen zones and the roadmap were written.
- `140-session/tasks.md` showed **0 of 41** tasks done while the session layer was built, tested, and hardware-verified.
- `150-client/tasks.md` showed **0 of 121** done and stated that "no method here has been exercised against a device", after most of the client had been verified against one.

Every one of those was wrong in the same direction: the roadmap got updated because it is what gets read, and the zone task files quietly rotted. A progress record that is confidently wrong is worse than none, because it is consulted and believed.

The finer granularity was not paying for itself either. The task files also carried `<!-- files: ... -->` annotations pointing at paths that were never created (`session/mod.rs`, `client/mod.rs`), because the implementation diverged from the plan and only the code moved.

### What was kept

Deleting the files did not mean deleting their content:

- **Task state** was reconciled into `roadmap.md`, including outstanding items that existed nowhere else - notably the governance decisions tracked as ENG-003.1 to ENG-003.6.
- **Design divergences** moved to the owning zone's `design.md`, which is where a decision about how something is built belongs: see [DES-SES-DIVERGENCE](../140-session/design.md) and [DES-CLIENT-DIVERGENCE](../150-client/design.md).

### Consequence for agents

To answer "where are we up to", read `roadmap.md`. To answer "what must this do" or "why is it built this way", read the zone's `spec.md` and `design.md`. Do not create a `tasks.md`; add an item to the roadmap under the zone's ID prefix instead.

## Appendix

### Spec Numbering Ranges

```text
001        - overview singleton: taxonomy, traceability rules, routing index
100-199    - crate layers (cortex-rs leaf crate)
  100      - USB HID transport (hidapi, write STALL, exclusive access)
  110      - framing (report IDs, flags, reassembly, encode/decode)
  120      - proto schema (vendored .proto files, prost build, message types)
  130      - domain model (DeviceKind, presets, grid, blocks, catalog)
  140      - session (connect handshake, keepalive, subscription, correlation)
  150      - client (the ergonomic QuadCortex API surface)
200-299    - CLI (cortex binary)
  200      - CLI surface (commands, held daemon, output, completions)
300-399    - MCP server (cortex-mcp binary)
  300      - MCP safety surface and tool list
400-499    - GUI (cross-platform Tauri desktop app; first draft in progress)
  400      - Tauri 2 + React + Mantine + Vite (fixture/daemon working-state editor)
500-599    - DX (linting, formatting, testing)
600-699    - CI / release
900-999    - reserved for cross-cutting living behaviour
```

### Routing Index (authoritative owner map)

| Zone | Spec | Owns (primary source) | Status |
| --- | --- | --- | --- |
| [100-transport](../100-transport/spec.md) | USB HID transport | `crates/cortex-rs/src/transport.rs` | Implemented and hardware-verified |
| [110-framing](../110-framing/spec.md) | HID frame codec | `crates/cortex-rs/src/framing.rs` | Implemented |
| [120-proto-schema](../120-proto-schema/spec.md) | Protobuf schema | `crates/cortex-rs/{build.rs,proto/}` | Implemented |
| [130-domain-model](../130-domain-model/spec.md) | Domain model and pure grid builders | `crates/cortex-rs/src/{device,message,catalog,grid,view,safety}.rs` | Partial; core typed views and builders implemented |
| [140-session](../140-session/spec.md) | HID link seam, session and subscribed state | `crates/cortex-rs/src/{link,session,state}.rs` | Implemented; core paths hardware-verified |
| [150-client](../150-client/spec.md) | Client API | `crates/cortex-rs/src/client.rs` | Partial; implemented core read/edit/save paths hardware-verified |
| [200-cli](../200-cli/spec.md) | CLI and shared host boundary | `crates/cortex-cli/src/{main,connect,decode}.rs`, `crates/cortex-host/src/` | Linux usable pre-alpha; Windows IPC adapter planned |
| [300-mcp](../300-mcp/spec.md) | MCP server | `crates/cortex-mcp/src/{main,server,transport}.rs`, process tests | Non-persistent tools hardware-verified; save/delete absent |
| [400-gui](../400-gui/spec.md) | Tauri GUI | `gui/` | Interactive non-persistent editor; fixture and daemon-backed modes |
| [500-dx-tooling](../500-dx-tooling/spec.md) | DX/tests | `s/`, `.editorconfig`, lint configs | Partial |
| [600-ci-release](../600-ci-release/spec.md) | CI/release | `.github/workflows/`, `dependabot.yml` | Partial |
| [900-project-governance](../900-project-governance/spec.md) | Governance | `AGENTS.md`, `NOTICE`, `THIRD-PARTY-NOTICES.md`, `LICENSE` | Implemented |

### Traceability Contract

All spec-driven source files carry a top-level doc-comment:

```rust
//! USB HID transport for the Quad Cortex.
//!
//! @see spec/100-transport/spec.md [FR-1]
//! @see spec/100-transport/design.md [DES-STALL]
```

At least one `@see` MUST point at a `spec.md` or `design.md` under `spec/`.

**Scope (settled under ENG-004.1):** this contract covers owned Rust source under `crates/` and `gui/src-tauri/`. It deliberately excludes the TypeScript frontend (`gui/src/`), the `s/` scripts, and configuration files (manifests, CI workflows, build/lint config) - see Non-Goals.

### Glossary

| Term | Definition |
| --- | --- |
| Zone spec | A `spec/XXX-name/` folder with `spec.md` and `design.md`; progress lives in the repository-wide roadmap and completed record |
| Living document | `spec.md`/`design.md` representing current truth, not logs |
| `@see` | Doc-comment linking a source file to its governing zone spec |
| Honest state | Each operation and host path is labelled according to this project's evidence; prior-art evidence alone does not make this implementation verified |
| Leaf crate | `cortex-rs` with `default-features = false`: no hidapi, no async runtime |
| Write STALL | The benign USB status-stage stall on every `SET_REPORT`; `hid_write()` returns `-1` on a write that worked |
| Trailer | The 8-byte suffix on a reassembled message carrying the `CortexMessageType` tag as a LE u16 |
| Connect handshake | The paced sequence: ResetCommsBuffers; Version READ/cache and UPDATE; ModelRepo READ/wait; Connection; 22 subscribe READs; CPULoad CREATE; adaptive settle |
| Provisional | Not yet verified against real hardware by this project; may work but is not confirmed |
