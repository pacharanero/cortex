---
afx: true
type: SPEC
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["proto-schema", "protobuf", "prost", "build-script", "cortex-rs", "quad-cortex", "nano-cortex"]
---

# cortex-rs - Protobuf Schema (Zone 120)

> Owns the vendored Cortex Control `.proto` files and the `prost` build script that compiles them into the public `cortex_rs::proto` module. The protobuf package and generated filename are `cortex_protobuf_v2`; that name is not an additional public Rust module layer.

## References

- [001-overview/spec.md](../001-overview/spec.md) - taxonomy, traceability rules, routing index.
- [Public protocol reference](../../docs/protocol.md) - authoritative protocol facts.
- [pyquadcortex](https://github.com/stokes-audio/pyquadcortex) - the MIT-licensed Python reference implementation whose recovered `.proto` files are vendored here.
- [NOTICE](../../NOTICE) and [THIRD-PARTY-NOTICES.md](../../THIRD-PARTY-NOTICES.md) - attribution for the MIT-licensed schema.
- [prost-build docs](https://docs.rs/prost-build) - the build-time code generator.
- [AGENTS.md](../../AGENTS.md) - protocol invariants and prior-art licensing.

## Problem Statement

The Cortex Control USB HID protocol carries protobuf-encoded messages inside a trailer-tagged envelope (see [110-framing](../110-framing/spec.md)). To decode and encode those messages in Rust without a runtime protobuf dependency, this zone vendors the recovered `.proto` schema and compiles it to typed Rust at build time via `prost`. The generated types are the wire-shape contract that every other layer depends on; this zone owns that contract and nothing above it.

The schema was recovered from Neural DSP's Cortex Control desktop application by the MIT-licensed `stokes-audio/pyquadcortex` project. It is vendored under the MIT license's distribution terms, with attribution; the `.proto` files carry their own SPDX header (copyright (c) 2026 Stokes, MIT). We do not redistribute Neural DSP binaries, firmware, or artwork - only the schema definitions needed for interoperability.

## User Stories

### Primary Users

Maintainers, AI coding agents, and downstream crate consumers who decode or encode Cortex Control messages.

### Stories

**As an** AI agent
**I want** to look up a `CortexMessageType` value and find the corresponding message struct in the generated `proto` module
**So that** I can write a typed decode for a new message flow without re-deriving the schema.

**As a** crate consumer
**I want** `cortex-rs` to require `protoc` only while building
**So that** I can embed the generated `prost` types without shipping `protoc` or a dynamic schema parser.

**As a** maintainer
**I want** the `.proto` files to carry their own MIT SPDX header and provenance note
**So that** the attribution survives copying into derivative projects and `reuse lint` passes.

**As a** maintainer
**I want** Cargo to recompile the generated module when a `.proto` file changes
**So that** stale generated types never cause a silent protocol mismatch.

## Requirements

### Functional Requirements

| ID | Requirement | Priority |
| --- | --- | --- |
| FR-1 | The vendored `.proto` files live at `crates/cortex-rs/proto/{Preset,ProductionAutomation}.proto` and declare `package cortex_protobuf_v2`. | Must Have |
| FR-2 | `crates/cortex-rs/build.rs` compiles both `.proto` files via `prost_build::Config` into `$OUT_DIR/cortex_protobuf_v2.rs`. | Must Have |
| FR-3 | The generated module is exposed as `cortex_rs::proto` via `include!(concat!(env!("OUT_DIR"), "/cortex_protobuf_v2.rs"))` in `lib.rs`. | Must Have |
| FR-4 | `ProductionAutomation.proto` carries the `CortexMessageType.Enum` with 72 variants: `Undefined=0` through `NumberOfMessageTypes=71`. | Must Have |
| FR-5 | `ProductionAutomation.proto` carries the `MessageAction.Enum` with 9 variants: `CREATE=0`, `UPDATE=1`, `DELETE=2`, `READ=3`, `MOVE=4`, `COPY=5`, `UPLOAD=6`, `DOWNLOAD=7`, `SWAP=8`. | Must Have |
| FR-6 | `VersionMessage.DeviceType` carries `QC=0` and `ATMA=1` (`ATMA` is the Nano Cortex codename). | Must Have |
| FR-7 | `Preset.proto` declares `package cortex_protobuf_v2` so that `ProductionAutomation.proto`'s `import "Preset.proto"` resolves cross-file `Model` references. | Must Have |
| FR-8 | `build.rs` emits `cargo::rerun-if-changed=proto/Preset.proto` and `cargo::rerun-if-changed=proto/ProductionAutomation.proto`. | Must Have |
| FR-9 | Both `.proto` files carry an SPDX-FileCopyrightText / SPDX-License-Identifier header (MIT, copyright 2026 Stokes) and a provenance note pointing to `pyquadcortex`. | Must Have |
| FR-10 | `build.rs` itself carries the project AGPL-3.0-or-later SPDX header and a module doc-comment linking to this spec. | Must Have |
| FR-11 | The build script does not require network access; `protoc` is its only system dependency. Default HID builds have separate platform prerequisites owned by zone 100. | Must Have |
| FR-12 | `Preset.proto` defines `BinaryPreset`, `Chain`, `Model`, `Param`, `ParamValue`, `Bypass`, `ColBypass`, `SceneBypass`, `SplitControlPoints`, `Expression`, `ExpressionBypassInfo`, `MidiMessageInfo`, `LegacyStompModeStompData`, `StompModeAssignment`, `SlotNotification`. | Must Have |
| FR-13 | `ProductionAutomation.proto` defines the full message set referenced by `CortexMessageType`: `VersionMessage`, `GridMessage`, `RecallPresetMessage`, `SceneMessage`, `FileMessage`, `IOSettingsMessage`, `DiagnosticsMessage`, `ModeMessage`, `KeepAliveMessage`, `ConnectionMessage`, `ModelRepoMessage`, `ResetCommsBuffersMessage`, `SuspendConnectionMessage`, and the remaining production-automation messages. | Must Have |
| FR-14 | A compile-time Rust registry maps every concrete operational `CortexMessageType` value 1 through 70 to exactly one generated protobuf struct and exposes a typed decoded enum, `decode_registered`, `registered_name`, reverse message type lookup, and an ordered registry table from one macro source. | Must Have |
| FR-15 | `Undefined=0` and `NumberOfMessageTypes=71` are rejected as sentinels. Unknown future numeric trailer tags are rejected while retaining their original `u16`; they are never coerced to `Undefined`. | Must Have |

### Non-Functional Requirements

| ID | Requirement | Target |
| --- | --- | --- |
| NFR-1 | `cargo build -p cortex-rs` succeeds on a system with `protoc` installed and no HID hardware. | CI-enforced |
| NFR-2 | The generated module compiles with `#![forbid(unsafe_code)]` and `clippy::pedantic` at the crate level (suppressed inside `proto` via `#[allow(...)]`). | CI-enforced |
| NFR-3 | The `.proto` files are not modified beyond the `package` line added to `Preset.proto` and the SPDX header; any further change is a deliberate protocol-version event recorded in [roadmap.md](../roadmap.md) under PROT-003. | Review-enforced |
| NFR-4 | `reuse lint` passes for the `.proto` files (MIT-licensed, copyright Stokes) and `build.rs` (AGPL-licensed, copyright Dr Marcus Baw). | CI-enforced |
| NFR-5 | Adding a generated `CortexMessageType` variant makes the registry's exhaustive matches fail to compile until the new concrete type or sentinel is classified. Registry decoding remains opt-in and does not decode or allocate every inbound message on the session hot path. | Compile-time / architectural invariant |

## Acceptance Criteria

- [x] `crates/cortex-rs/proto/Preset.proto` and `crates/cortex-rs/proto/ProductionAutomation.proto` exist with SPDX headers.
- [x] `crates/cortex-rs/build.rs` compiles both files via `prost_build` and emits `rerun-if-changed` for both.
- [x] `cortex_rs::proto::CortexMessageType` exposes 72 generated variants (`Undefined=0` through `NumberOfMessageTypes=71`).
- [x] `cortex_rs::proto::MessageAction` exposes 9 generated variants.
- [x] `cortex_rs::proto::version_message::DeviceType` exposes `Qc=0` and `Atma=1`.
- [x] `cargo build -p cortex-rs --no-default-features` succeeds with `protoc` present and no HID hardware.
- [x] `reuse lint` passes for the proto directory and the build script.
- [x] The Rust registry contains exactly the 70 concrete values, decodes an empty default body for each generated struct, and round-trips every decoded variant to its generated enum value.
- [x] Both sentinels and unknown future numeric values are rejected without collapsing their tags.

## Non-Goals

- Operational meaning or support claims for registered messages; the registry records only the schema's tag-to-struct relationship.
- The typed domain model that wraps these proto types (owned by [130-domain-model](../130-domain-model/spec.md)).
- Frame-level gzip decompression (transport/session) and field-level catalog decompression (zone 130).
- On-the-fly schema recovery or runtime `.proto` parsing; the schema is compile-time only.
- Re-deriving the schema from Cortex Control binaries. The schema is vendored as-is from `pyquadcortex`; this zone only maintains the build glue.

## Dependencies

- **System**: `protoc` (the Protocol Buffers compiler). CI installs `protobuf-compiler`; local setup is documented in the installation guide.
- **Rust**: `prost` (runtime) and `prost-build` (build-time), both via the workspace.
- **Upstream**: the MIT-licensed `stokes-audio/pyquadcortex` recovered schema. Recorded in `NOTICE` and `THIRD-PARTY-NOTICES.md`.
- **Downstream**: [130-domain-model](../130-domain-model/spec.md) wraps the generated `BinaryPreset` / `Model` / `Param` types; [140-session](../140-session/spec.md) and [150-client](../150-client/spec.md) decode envelope bodies against the generated message structs.

## Appendix

### CortexMessageType variants (72)

The enum in `ProductionAutomation.proto` is the authoritative list. Values are stable identifiers, not array indices: a CorOS update can add values, so callers must handle unknown values gracefully (see [DES-VERSION](design.md#des-version)).

```text
Undefined=0  Grid=1  SetlistPosition=2  IOSettings=3  File=4  IOMeter=5
Tuner=6  Diagnostics=7  MIDISettings=8  GeneralSettings=9  Version=10
ProductionAutomationMode=11  GridMove=12  Scene=13  Mode=14  RecallPreset=15
EnableCaptureOut=16  MasterVolume=17  CloudLogin=18  DefaultParameters=19
RecentsFavorites=20  UndoRedo=21  SceneCopy=22  SceneLabel=23  ShowGigView=24
Screenshot=25  CPULoad=26  ShowTuner=27  Looper=28  ProductForward=29
BackupsForward=30  LogsForward=31  KeepAlive=32  GlobalTempo=33
PresetDirty=34  ModuleStats=35  NeuralCapture=36  GridModelMeter=37
GlobalEQ=38  RecentSearches=39  LocalBackup=40  CloudBackup=41
CompilerInhibitedModules=42  SystemTimeSync=43  Logs=44
ProcessDownloadsQueue=45  CloudProduct=46  Confirmation=47  SceneColor=48
Connection=49  NewModels=50  ModelRepo=51  ResetCommsBuffers=52
SuspendConnection=53  PinnedModels=54  GigViewButton=55  GenericError=56
BulkOperation=57  License=58  PresetSpeedTest=59  Updater=60
UpdaterForward=61  GainCalibration=62  NeuralCapture2=63  Serialization=64
TestFarm=65  ProductionTest=66  LoadAutomatedTestPreset=67
SetTestPresetInputOutputPorts=68  SetTestPresetSplitMixPoints=69
GenerateTestPreset=70  NumberOfMessageTypes=71
```

### MessageAction variants (9)

```text
CREATE=0  UPDATE=1  DELETE=2  READ=3  MOVE=4  COPY=5  UPLOAD=6  DOWNLOAD=7  SWAP=8
```

### VersionMessage.DeviceType variants (2)

```text
QC=0      Quad Cortex (primary verification target, CorOS 4.0.1 / firmware d14e)
ATMA=1    ATMA variant (identified by prior art as the Nano codename; proves only that this schema names it)
```

### Routing Index entry

| Zone | Spec | Owns (primary source) | Status |
| --- | --- | --- | --- |
| [120-proto-schema](./spec.md) | Protobuf schema and typed message registry | `crates/cortex-rs/{build.rs,proto/}`, `crates/cortex-rs/src/registry.rs` | Implemented |

### Glossary

| Term | Definition |
| --- | --- |
| `cortex_protobuf_v2` | The protobuf package name and generated filename; public Rust types are exposed directly under `cortex_rs::proto`. |
| `CortexMessageType` | The 72-variant enum tagging every reassembled message in the trailer. |
| `MessageAction` | The 9-variant CRUD-ish enum carried by most request/response messages. |
| `DeviceType` | The 2-variant enum on `VersionMessage` distinguishing Quad Cortex from Nano Cortex. |
| `BinaryPreset` | The top-level preset protobuf message: chains, scenes, bypass, tags, metadata. |
| Vendored | Copied into this repo's tree under the upstream license, as opposed to a dependency. |
