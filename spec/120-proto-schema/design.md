---
afx: true
type: DESIGN
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["proto-schema", "protobuf", "prost", "build-script", "design"]
spec: spec.md
---

# cortex-rs - Protobuf Schema Design (Zone 120)

## [DES-OVR] Overview

This zone is a thin build-time shim: two vendored `.proto` files plus a `prost` build script that compiles them into typed Rust exposed as `cortex_rs::proto`. `cortex_protobuf_v2` is the protobuf package and generated filename, not a nested public module.

The schema is vendored, not re-derived. We do not run a recovery pass against Cortex Control binaries in this project; we trust the `pyquadcortex` recovery and re-verify the protocol behaviour against real hardware at the transport/session layer (see [100-transport](../100-transport/spec.md), [140-session](../140-session/spec.md)). A schema change here is a protocol-version event, not a routine edit.

## [DES-FILES] Owned Files

| File | Role | License |
| --- | --- | --- |
| `crates/cortex-rs/build.rs` | `prost_build` invocation, `rerun-if-changed` emissions | AGPL-3.0-or-later (Dr Marcus Baw) |
| `crates/cortex-rs/proto/Preset.proto` | `BinaryPreset`, `Chain`, `Model`, `Param`, `ParamValue`, `Bypass`, scene/bypass/expression/MIDI types | MIT (Stokes) |
| `crates/cortex-rs/proto/ProductionAutomation.proto` | `CortexMessageType`, `MessageAction`, `VersionMessage`, `GridMessage`, `FileMessage`, and the full production-automation message set | MIT (Stokes) |
| `crates/cortex-rs/src/lib.rs` (the `proto` module block) | `include!` of the generated file into `cortex_rs::proto` | AGPL-3.0-or-later (Dr Marcus Baw) |

The generated `cortex_protobuf_v2.rs` lives in `$OUT_DIR` and is not committed; it is rebuilt on every `cargo build` that touches a `.proto` file.

## [DES-BUILD] Build Script Design

`build.rs` is deliberately minimal:

```rust
prost_build::Config::new()
    .out_dir(&out_dir)
    .compile_protos(
        &[proto_dir.join("Preset.proto"),
          proto_dir.join("ProductionAutomation.proto")],
        &[proto_dir],
    )?;
println!("cargo::rerun-if-changed=proto/Preset.proto");
println!("cargo::rerun-if-changed=proto/ProductionAutomation.proto");
```

- Both files are passed to a single `compile_protos` call so `prost` sees the cross-file `import "Preset.proto"` and resolves `Model` references in `ProductionAutomation.proto`.
- `out_dir` is `$OUT_DIR` (Cargo-provided), so the generated file is co-located with other build artefacts and never checked in.
- `rerun-if-changed` is emitted for both `.proto` files; without it, Cargo would not re-run the build script on a schema edit.
- No `protoc` flags or type attributes are set today. If the domain layer (130) needs `prost` annotations (e.g. custom string types, base64 helpers), they are added here, not in `lib.rs`.

### System dependency: `protoc`

`prost-build` shells out to `protoc` (the Protocol Buffers compiler). On Linux this is the `protobuf-compiler` package. CI installs it; the installation guide documents the local requirement. This is the only system dependency of the no-HID leaf build, and it is build-time only. Default HID builds have additional platform prerequisites; the runtime `prost` dependency has no `protoc` requirement.

## [DES-PKG] Package and Import Design

Both `.proto` files declare `package cortex_protobuf_v2`. This matters because `ProductionAutomation.proto` contains `import "Preset.proto"` and references `BinaryPreset` and `Model` (e.g. in `GridMessage`, `RecallPresetMessage`, `DefaultParametersModel`, `NeuralCaptureMessage`). For `prost` to resolve those cross-file references, both files must share a package.

The original `pyquadcortex` recovery had `Preset.proto` without an explicit package; we added `package cortex_protobuf_v2` to it so the generated Rust types land in the same module and cross-references resolve. This is the only deliberate modification to the vendored schema; it is recorded in [DES-DELTA] below.

## [DES-INCLUDE] Module Inclusion

`lib.rs` exposes the generated types as:

```rust
#[allow(missing_docs, clippy::all, clippy::pedantic)]
pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/cortex_protobuf_v2.rs"));
}
```

The `#[allow]` block is intentional: `prost`-generated code does not carry rustdoc, and `pedantic` flags it. The crate-level `#![warn(missing_docs)]` and `#![warn(clippy::pedantic)]` apply everywhere else; the `proto` module is the single exception, narrowly scoped.

## [DES-VERSION] Version Drift Design

There is no version field on the wire (see AGENTS.md). A CorOS update can add `CortexMessageType` variants or new fields to existing messages without bumping anything we can detect. The design response:

- **Unknown enum field values remain raw integers.** Prost stores enum-valued fields as `i32`; converting with `try_from` can fail. Unknown trailer message tags remain raw `u16` in `Message` and are logged or skipped by higher layers.
- **Unknown fields are ignored; known absent fields default.** Proto3 decode remains forward-compatible with additional fields, while optional and oneof accessors are `None` only when a known field is absent.
- **Schema edits are events.** Any change to a `.proto` file (beyond the SPDX header and the `package` line on `Preset.proto`) is a protocol-version event and must be recorded in [roadmap.md](../roadmap.md) under PROT-003, with the CorOS version that introduced it.

## [DES-LICENSING] Licensing and Attribution Design

The `.proto` files are MIT-licensed material vendored under the MIT distribution terms. They carry their own SPDX header so `reuse lint` attributes them correctly to Stokes, not to Dr Marcus Baw:

```text
// SPDX-FileCopyrightText: 2026 Stokes (https://github.com/stokes-audio)
// SPDX-License-Identifier: MIT
//
// Recovered from Cortex Control.app by the MIT-licensed
// stokes-audio/pyquadcortex project (https://github.com/stokes-audio/pyquadcortex).
// Vendored into cortex-rs under the MIT license's distribution terms;
// see NOTICE and THIRD-PARTY-NOTICES.md at the repo root for attribution.
```

`build.rs` carries the project AGPL header. The two licenses are compatible: MIT material can be incorporated into an AGPL project, and the MIT-licensed files keep their own terms (we are not relicensing them). `NOTICE` and `THIRD-PARTY-NOTICES.md` record the derivation.

## [DES-DELTA] Deliberate Modifications to the Vendored Schema

| File | Modification | Reason |
| --- | --- | --- |
| `Preset.proto` | Added `package cortex_protobuf_v2;` | So `ProductionAutomation.proto`'s `import "Preset.proto"` resolves cross-file `Model`/`BinaryPreset` references in `prost`. |
| Both files | Added SPDX header + provenance note | License attribution and `reuse lint` compliance. |

No other modifications. We do not rename messages, renumber enum values, or trim fields. If a future CorOS version requires a schema update, the diff against `pyquadcortex` upstream is recorded in [roadmap.md](../roadmap.md) under PROT-003.

## [DES-LAYERS] Layer Map (cross-reference)

```text
Layer 3: Proto (120)       - prost-generated types from .proto files (compile-time)
       ^
       |  (cortex_rs::proto::* types)
       |
Layer 4: Domain (130)      - Message, DeviceKind, Preset, Grid, Block, Catalog; Scene planned
```

This zone is Layer 3. It has no dependency on any higher layer; the dependency arrow points *down* into the generated types from the domain model.

## [DES-TEST] Testing Strategy

- **No unit tests in this zone.** The generated types are tested by use: the domain layer (130) and the framing layer (110) exercise them in decode tests.
- **Build-time check.** `cargo build -p cortex-rs --no-default-features` is the smoke test for this zone: if `protoc` is missing or a `.proto` file is malformed, the build fails.
- **Conformance reference.** The `pyquadcortex` offline test suite exercises the same schema; cross-checking against it is done at the domain layer, not here.
