---
afx: true
type: TASKS
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["proto-schema", "tasks", "traceability"]
spec: spec.md
design: design.md
---

# cortex-rs - Protobuf Schema Tasks (Zone 120)

> Implementation and maintenance tasks for the vendored `.proto` schema and the `prost` build script. Schema edits are protocol-version events - record them here, do not slip them in silently.

## Phase 1 - Vendoring and build glue (DONE)

### 1.1 Vendor the recovered schema

- [x] Copy `Preset.proto` and `ProductionAutomation.proto` from `pyquadcortex` into `crates/cortex-rs/proto/`.
- [x] Add SPDX-FileCopyrightText / SPDX-License-Identifier (MIT, Stokes) + provenance note to both files.
- [x] Add `package cortex_protobuf_v2;` to `Preset.proto` so cross-file `Model` references resolve.
- [x] Record attribution in `NOTICE` and `THIRD-PARTY-NOTICES.md`.

### 1.2 Wire up prost-build

- [x] Add `prost` (runtime) and `prost-build` (build-time) to `Cargo.toml` via the workspace.
- [x] Write `crates/cortex-rs/build.rs` with a single `compile_protos` call for both files.
- [x] Emit `cargo::rerun-if-changed` for both `.proto` files.
- [x] Expose the generated module as `cortex_rs::proto` via `include!` in `lib.rs`, with `#[allow(missing_docs, clippy::all, clippy::pedantic)]`.

### 1.3 CI readiness

- [x] Confirm `cargo build -p cortex-rs --no-default-features` succeeds with `protoc` installed and no HID hardware.
- [ ] Confirm CI installs `protobuf-compiler` (owned by [600-ci-release](../600-ci-release/spec.md)).
- [ ] Confirm `reuse lint` passes for the `.proto` files and `build.rs`.

## Phase 2 - Schema hygiene (LIVING)

### 2.1 Verify against CorOS 4.0.1

- [x] Confirm the 72 `CortexMessageType` variants match the device observed on CorOS 4.0.1 / firmware `d14e`.
- [x] Confirm the 9 `MessageAction` variants.
- [x] Confirm `VersionMessage.DeviceType` carries `QC=0` and `ATMA=1`.

### 2.2 Record deliberate modifications

- [x] Document the `package cortex_protobuf_v2` addition to `Preset.proto` in [DES-DELTA] of `design.md`.
- [x] Document the SPDX header addition in [DES-DELTA].

## Phase 3 - Future schema events (PLANNED)

### 3.1 CorOS update protocol-version event

When a CorOS update adds `CortexMessageType` variants or new message fields:

- [ ] Diff the device's emitted schema against the vendored copy (via `pyquadcortex` or a fresh recovery).
- [ ] Update the `.proto` file(s) with the minimum diff needed for interoperability.
- [ ] Record the CorOS version, the diff, and the verification status in a new row under Work Sessions.
- [ ] Re-run the domain-layer (130) tests against the new schema.
- [ ] Bump the `version` frontmatter on `spec.md` and `design.md`.

### 3.2 Optional prost annotations

If the domain layer (130) needs `prost` type attributes (e.g. base64 for `bytes` fields, custom string newtypes):

- [ ] Add the attribute to `build.rs` via `Config::new().type_attribute(".", "...")`.
- [ ] Document the attribute and its rationale in [DES-BUILD].

## Open Questions

- Should we pin a specific `protoc` version in CI to avoid a silent protobuf-version drift? Tracked in [600-ci-release](../600-ci-release/spec.md).
- Should the generated `cortex_protobuf_v2.rs` be checked in for `crates.io` consumers who lack `protoc`? Current answer: no - `protoc` is a documented build requirement; checking in generated code blurs the provenance boundary.

## Work Sessions

| Date | Task | Action | Files Modified | Agent | Human |
| --- | --- | --- | --- | --- | --- |
| 2026-08-01 | 1.1, 1.2, 2.1, 2.2 | Vendored schema, wired prost-build, verified against CorOS 4.0.1 | `crates/cortex-rs/{build.rs,proto/*,src/lib.rs}`, `NOTICE`, `THIRD-PARTY-NOTICES.md` | [x] | [x] |