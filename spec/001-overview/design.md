---
afx: true
type: DESIGN
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["overview", "architecture", "rust", "usb-hid", "leaf-crate"]
spec: spec.md
---

# cortex-rs - Architecture Overview

## [DES-OVR] Overview

A Rust workspace with a leaf crate (`cortex-rs`) owning the Cortex Control USB HID protocol and typed domain model, plus thin binary surfaces (`cortex-cli`, `cortex-mcp`) and a planned Tauri GUI (`gui/`). The crate is a port of the protocol behaviour established by the MIT-licensed `stokes-audio/pyquadcortex` Python library, re-verified against a real Quad Cortex on Linux.

State honesty is the central invariant: the USB HID protocol for the Quad Cortex is hardware-verified (via pyquadcortex and re-verified on this machine); anything we reverse-engineer ourselves or extend (unknown message types, the MCP safety surface, Nano Cortex specifics) is labelled provisional until verified against real hardware.

## [DES-ARCH] System Context and Flow Map

```text
[Flow.CLI]      cortex binary (200) -- calls crate API
[Flow.MCP]      cortex-mcp binary (300) -- calls crate API, gated saves
[Flow.GUI]      Tauri backend (400) -- calls crate API via Tauri commands
      |
      v
[Flow.Client]   QuadCortex client (150) -- ergonomic API, builds protobuf messages
      |
      v
[Flow.Session]  Session (140) -- connect handshake, keepalive, correlation
      |
      v
[Flow.Transport] Transport (100) -- hidapi open/read/write, STALL swallow
      |
      v
[Flow.Framing]  Framing (110) -- report IDs, flags, reassembly, encode/decode
      |
      v
[Flow.Schema]   Proto schema (120) -- prost-generated types from .proto files
      |
      v
hidapi -> /dev/hidraw7 -> Quad Cortex (USB, interface 5)
```

| Map ID | Owner zone | Owned files |
| --- | --- | --- |
| `[Flow.Transport]` | 100 | `transport.rs` |
| `[Flow.Framing]` | 110 | `framing.rs` |
| `[Flow.Schema]` | 120 | `build.rs`, `proto/` |
| `[Flow.Domain]` | 130 | `device.rs`, `message.rs` (and future `preset.rs`, `grid.rs`, `catalog.rs`) |
| `[Flow.Session]` | 140 | `session.rs` (planned) |
| `[Flow.Client]` | 150 | `client.rs` (planned) |
| `[Flow.CLI]` | 200 | `crates/cortex-cli/src/main.rs` |
| `[Flow.MCP]` | 300 | `crates/cortex-mcp/src/main.rs` |
| `[Flow.GUI]` | 400 | `gui/` (planned) |

## [DES-DEC] Cross-Cutting Key Decisions

| Decision | Choice | Rationale |
| --- | --- | --- |
| Leaf crate | `cortex-rs` with `default-features = false` depends only on serde/bytes/flate2/prost | Embeddable in CLI, MCP, Tauri, and crates.io; no hidapi without the `hid` feature |
| Protocol source | Port from `pyquadcortex` (MIT) | Hardware-verified, excellent docs, recovered .proto files; not re-deriving from scratch |
| Protobuf | `prost` with vendored .proto files | Compile-time typed Rust; no runtime protobuf dependency |
| HID backend | `hidapi` crate (hidraw on Linux) | Cross-platform; the same backend pyquadcortex uses |
| Transport model | Synchronous `request()` for CLI; background RX thread for session/client | The CLI's `version` command is fire-and-forget; the full client needs correlation + broadcast waiting |
| Planned multi-device model | `DeviceKind::{QuadCortex, NanoCortex}` with Nano failing closed | The schema has QC=0 and ATMA=1, but that does not establish a shared transport; the Nano variant retains a non-matching PID until hardware proves compatibility |
| License | AGPL-3.0-or-later (code), CC-BY-SA-4.0 (content) | Not available for proprietary subsumption; MIT/Apache prior art ported with attribution |

## [DES-FILES] Repository Map

```text
crates/
  cortex-rs/    Leaf crate: transport, framing, proto, domain, session, client
  cortex-cli/   The `cortex` binary: thin main.rs over the crate
  cortex-mcp/   The `cortex-mcp` MCP server: safety-surface-gated tools
spec/           AFX zone specs (this tree)
s/              Repo scripts: s/test, s/lint
gui/            Planned: Tauri 2 desktop app (deferred)
docs/           Planned: protocol notes, runbooks, GUI docs
```

## [DES-LAYERS] Crate Layer Map

The crate is layered bottom-up. Each layer depends only on the layers below it:

```text
Layer 6: Client (150)      - QuadCortex struct, ergonomic methods (version, recall, read_preset, ...)
Layer 5: Session (140)     - connect handshake, keepalive, request_id correlation, broadcast waiting
Layer 4: Domain (130)      - DeviceKind, Message, Preset, Grid, Block, Scene, Catalog (typed model)
Layer 3: Proto (120)       - prost-generated types from .proto files (compile-time)
Layer 2: Framing (110)     - encode_message/decode, Frame, FrameReassembler, Flags, ReportId
Layer 1: Transport (100)   - Transport::open/write/read/request, STALL swallow, hidapi wrapper
```

Layers 1-4 are implemented (scaffold). Layers 5-6 are planned. The CLI currently calls layer 1 directly (`Transport::request`) for the `version` command; once layer 5-6 exist, the CLI will call `QuadCortex::version()` instead.

## [DES-TEST] Testing Strategy

Owned in detail by [500-dx-tooling](../500-dx-tooling/spec.md):

- **Unit tests** for framing, message parsing, domain model (no hardware needed).
- **Hardware smoke tests** for transport, session, and client (manual runbook; CI has no hardware).
- **Conformance reference**: `pyquadcortex` offline test suite and recovered .proto files.
- Agent-generated tests must not be the sole basis for accepting protocol or safety-surface behaviour.
