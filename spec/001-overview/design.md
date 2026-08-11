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

A Rust workspace with a leaf crate (`cortex-rs`) owning the Cortex Control USB HID protocol and typed domain model, a shared daemon IPC crate (`cortex-host`), and three host surfaces: the usable `cortex-cli`, the hardware-verified non-persistent `cortex-mcp`, and an interactive read-only Tauri GUI with explicit fixture and daemon-backed modes. The crate is a port of the protocol behaviour established by the MIT-licensed `stokes-audio/pyquadcortex` Python library, re-verified against a real Quad Cortex on Linux. The GUI target is cross-platform; Linux is the only verified host today.

State honesty is the central invariant: verification is attached to each operation and host path. The implemented core Quad Cortex paths are hardware-verified. Nano Cortex HID framing and one read-only state exchange are hardware-verified, while Nano runtime integration, other operations, untested edge cases, and new host platforms remain provisional.

## [DES-ARCH] System Context and Flow Map

```text
[Flow.CLI]      cortex binary + held daemon (200)
[Flow.MCP]      cortex-mcp binary (300) -- daemon-backed non-persistent tools
[Flow.GUI]      Tauri backend (400) -> cortex-host -> held daemon (read-only)
      |
      v
[Flow.Host]     cortex-host (200) -- typed daemon contract + platform local IPC
      |
      v
[Flow.Client]   QuadCortex client (150) -- ergonomic API, builds protobuf messages
      |
      v
[Flow.Session]  Session (140) -- connect handshake, keepalive, correlation
      |
      v
[Flow.Link]     HidLink (140) -- transport-neutral report read/write seam
      |
      v
[Flow.Transport] Transport (100) -- hidapi open/read/write, STALL swallow
      |
      v
[Flow.Framing]  Framing (110) -- report IDs, flags, reassembly, encode
      |
      v
[Flow.Schema]   Proto schema (120) -- prost-generated types from .proto files
      |
      v
hidapi -> Quad Cortex HID interface 5
```

| Map ID | Owner zone | Owned files |
| --- | --- | --- |
| `[Flow.Transport]` | 100 | `transport.rs` |
| `[Flow.Framing]` | 110 | `framing.rs` |
| `[Flow.Schema]` | 120 | `build.rs`, `proto/` |
| `[Flow.Domain]` | 130 | `device.rs`, `message.rs`, `catalog.rs`, `grid.rs`, `view.rs`, `safety.rs` |
| `[Flow.Link]` / `[Flow.Session]` | 140 | `link.rs`, `session.rs`, `state.rs` |
| `[Flow.Client]` | 150 | `client.rs` |
| `[Flow.Host]` / `[Flow.CLI]` | 200 | `crates/cortex-host/src/`, `crates/cortex-cli/src/{main,connect,decode}.rs` |
| `[Flow.MCP]` | 300 | `crates/cortex-mcp/src/{main,server,transport}.rs`, process tests |
| `[Flow.GUI]` | 400 | `gui/` (explicit fixture and daemon-backed read modes) |

## [DES-DEC] Cross-Cutting Key Decisions

| Decision | Choice | Rationale |
| --- | --- | --- |
| Leaf crate | `cortex-rs` with `default-features = false` has protocol/domain dependencies only | Embeddable in CLI, MCP, Tauri, and crates.io; no hidapi or host IPC without the `hid` feature |
| Protocol source | Port from `pyquadcortex` (MIT) | Hardware-verified, excellent docs, recovered .proto files; not re-deriving from scratch |
| Protobuf | `prost` with vendored .proto files | Compile-time typed Rust; no runtime protobuf dependency |
| HID backend | `hidapi` crate (hidraw on Linux) | Cross-platform; the same backend pyquadcortex uses |
| Transport model | One background session owns HID; ordinary commands route through transport-neutral local IPC or one bounded direct session | The device does not enforce exclusivity; opening twice wedges the held owner. Unix uses an owner-only domain socket; Windows will use a current-user named pipe behind `cortex-host`'s endpoint/listener/connection facade |
| Planned multi-device model | `DeviceKind::{QuadCortex, NanoCortex}` with Nano failing closed | Hardware established shared HID framing but different report geometry, application envelopes, and domain models; the Nano variant retains a non-matching PID until those distinctions are implemented |
| License | AGPL-3.0-or-later (code), CC-BY-SA-4.0 (content) | Not available for proprietary subsumption; MIT/Apache prior art ported with attribution |

## [DES-FILES] Repository Map

```text
crates/
  cortex-rs/    Leaf crate: transport, framing, proto, domain, session, client
  cortex-cli/   The `cortex` binary: thin main.rs over the crate
  cortex-host/  Shared typed daemon contract and synchronous local IPC facade
  cortex-mcp/   Hardware-verified non-persistent agent tools over the daemon
spec/           AFX zone specs (this tree)
s/              Repo scripts: test, lint, hardware smoke, docs, GUI dev, release
gui/            Tauri 2 + React + Mantine read-only fixture/daemon first draft
docs/           Protocol reference, runbooks, CLI reference and GUI notes
```

## [DES-LAYERS] Crate Layer Map

The crate is layered bottom-up. Each layer depends only on the layers below it:

```text
Layer 7: Client (150)      - QuadCortex ergonomic methods and validated operations
Layer 6: State (130/140)   - subscribed cache, typed snapshots and revisions
Layer 5: Session (140)     - handshake, keepalive, correlation and HidLink ownership
Layer 4: Domain (130)      - typed views, catalog, grid builders and save safety
Layer 3: Proto (120)       - prost-generated wire types
Layer 2: Framing (110)     - reports, flags, reassembly and encoding
Layer 1: Transport (100)   - hidapi open/read/write and minimal synchronous diagnostic
```

All six layers are implemented for the core operations, and those paths passed the 42-check hardware smoke. The client remains intentionally incomplete relative to the device's wider API; each unimplemented operation is tracked in the roadmap rather than implied by the existence of the layer.

## [DES-TEST] Testing Strategy

Owned in detail by [500-dx-tooling](../500-dx-tooling/spec.md):

- **Unit tests** for framing, message parsing, domain model (no hardware needed).
- **Hardware smoke tests** for transport, session, and client (manual runbook; CI has no hardware).
- **Conformance reference**: `pyquadcortex` offline test suite and recovered .proto files.
- Agent-generated tests must not be the sole basis for accepting protocol or safety-surface behaviour.
