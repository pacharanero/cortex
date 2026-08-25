---
afx: true
type: SPEC
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["transport", "usb-hid", "hidapi", "quad-cortex", "stall", "exclusive-access"]
---

# cortex-rs - USB HID Transport

> Owns the `hidapi` wrapper that puts reports on the wire and pulls them back. It encapsulates the benign write STALL. Practical single ownership is a host/session invariant because neither hidapi nor the device reliably rejects a second opener.

## References

- [Public protocol reference](../../docs/protocol.md) - the authoritative, repository-local protocol facts.
- [pyquadcortex protocol docs](https://github.com/stokes-audio/pyquadcortex/blob/main/docs/protocol.md) - the MIT-licensed reference this behaviour is ported from.
- [001-overview spec](../001-overview/spec.md) - governing spec, traceability contract, routing index entry for this zone.
- [001-overview design](../001-overview/design.md) [DES-ARCH] - the system context flow map this layer sits at the bottom of.
- [110-framing spec](../110-framing/spec.md) - the frame codec this layer hands raw reports to (`Frame::parse`) and receives encoded reports from (`encode_message`).
- [130-domain-model spec](../130-domain-model/spec.md) - owns `DeviceKind`, which `Transport::open` takes as its argument.
- [140-session spec](../140-session/spec.md) - the implemented `HidLink` seam and background session over this transport.
- [README](../../README.md) - the udev rule and setup walkthrough a Linux user runs before `Transport::open` can succeed.
- Owned source: `crates/cortex-rs/src/transport.rs`.

## Problem Statement

The Cortex Control protocol runs over USB HID. Two of its wire behaviours are non-obvious enough that, if they were not encapsulated in one place, every caller (the CLI, the MCP server, the future Tauri backend) would have to know them and would get them wrong:

1. **The benign write STALL.** Every host-to-device `SET_REPORT` is acted upon by the device and *then* deliberately stalled at the USB status stage, so `hid_write()` returns `-1` on a write that worked. A naive caller treats that as a hard error and aborts. The correct response is to swallow it and use a read timeout as the dead-device signal.
2. **Single effective ownership.** A second process can open the same interface without error and then wedge the existing owner on its next request. The host must claim ownership before the handshake and route every ordinary command through that owner.

This zone owns the `Transport` struct that wraps `hidapi::HidDevice` and encodes both behaviours, so that every layer above can treat USB as a synchronous request/response pipe without knowing either gotcha.

## Verification Basis

| Claim | Status | Evidence |
| --- | --- | --- |
| VID:PID `152A:880A`, interface 5 | Hardware-verified | Real Quad Cortex on Linux running CorOS 4.0.1 |
| Input report ID `0x01`, output report ID `0x02`, 128-byte body | Hardware-verified | Matches `pyquadcortex/docs/protocol.md` and re-confirmed by `cortex device version` round-trip on this machine |
| The benign write STALL (`hid_write` returns `-1` on success) | Hardware-verified | Observed on this machine; documented in `pyquadcortex` |
| Swallow Quad write errors, detect its dead device via read timeout | Hardware-verified | `cortex device version` succeeds despite `-1` writes; a powered-off Quad surfaces as `Error::ReadTimeout` |
| `Transport::request` gzip-decompresses frame-level payloads starting `1f 8b` | Hardware-verified | Observed on RecallPreset pushes from `pyquadcortex`; the `version` round-trip does not compress |
| Nano Cortex transport | Partly hardware-verified and partly implemented | Real Nano on Linux confirmed VID:PID `152A:88E7`, interface 5, 65-byte reports, shared length/flag framing, multi-report state transfer, cross-transport BLE ownership, typed state, amp, bypass and raw FX parameter operations through a separate Nano codec and held daemon. A timeout or malformed response requires the held path to discard and reopen the transport before another request, invalidate the old generation, and complete a fresh state read before serving live data. Gate reduction and wider application operations remain provisional |

The `pyquadcortex` offline test suite is a conformance reference but not a substitute for a hardware smoke run. Agent-generated tests must not be the sole basis for accepting transport behaviour.

## User Stories

### Primary Users

CLI users, the MCP server, the future Tauri GUI backend, and downstream crate consumers.

### Stories

**As a** CLI user
**I want** `Transport::request` to swallow the write STALL and return the reassembled reply
**So that** `cortex device version` works without me knowing the device stalls every write.

**As an** MCP server author
**I want** the host to claim effective ownership before `Transport::open`
**So that** one owning process serves all tool calls even though the OS permits a damaging second open.

**As a** downstream crate consumer
**I want** to build `cortex-rs` with `default-features = false` and get no `hidapi` dependency
**So that** I can decode captures and run unit tests on a machine with no HID hardware.

**As a** maintainer
**I want** the dead-device signal to be a read timeout, not a write error
**So that** I do not misdiagnose the benign STALL as a device failure in logs or tool output.

## Requirements

### Functional Requirements

| ID | Requirement | Priority |
| --- | --- | --- |
| FR-1 | `Transport::open(DeviceKind)` opens the first matching device on the bus by VID:PID and retains that handle until dropped. It does not claim or guarantee process exclusivity. | Must Have |
| FR-2 | `Transport::write(&[u8])` is the raw synchronous write path. It frames with the selected device geometry, rejects short writes, swallows only the Quad's benign status-stage STALL, and propagates Nano write errors. Logical application envelopes remain above it. | Must Have |
| FR-3 | `Transport::read(Duration)` reads one selected-device input report (129 bytes Quad, 65 Nano) and returns `Error::ReadTimeout` on timeout. | Must Have |
| FR-4 | `Transport::request(message_type, payload, timeout)` performs synchronous request/response: encode, write (swallowing the STALL), read frames, reassemble, strip the 8-byte trailer, gzip-decompress if the body starts with `1f 8b`, return a `Message`. | Must Have |
| FR-5 | The transport module is gated behind the `hid` feature flag; `cargo build --no-default-features -p cortex-rs` succeeds with no `hidapi` dependency. | Must Have |
| FR-6 | `Transport` holds a single `hidapi::HidDevice`; it does not open a new connection per call. Drop releases that handle. Host-level `LocalClaim` enforces effective process ownership. | Must Have |
| FR-7 | `Transport::open` returns `Error::DeviceNotFound` when no matching device is enumerated; permission/backend failures may surface as HID errors. User-facing diagnostics belong to the host surfaces. | Must Have |
| FR-8 | `Transport::request` returns the first reassembled message, not one correlated by `request_id` (READ replies carry none). Correlation is a later concern owned by the session layer (140). | Should Have |
| FR-9 | Framing owns the currently implemented Quad constants `HID_BODY_LEN = 128` and `HID_REPORT_LEN = 129`; transport re-exports them for compatibility. Device-dependent geometry must preserve those values for Quad and use 64/65 for Nano. `DEFAULT_READ_TIMEOUT = 2s` remains transport-owned. | Must Have |
| FR-10 | `Transport::open(DeviceKind::NanoCortex)` uses hardware-verified PID `0x88E7`, retains Nano geometry, and provides only raw framed read/write. Quad-envelope `request` and `Session::open` reject Nano before device I/O; Nano application operations use their separate codec and held-daemon path. | Should Have |
| FR-11 | Quad write errors remain swallowed because its status-stage STALL is hardware-verified. Nano successful writes return normally and Nano write errors propagate; neither path retries a report. | Must Have |

### Non-Functional Requirements

| ID | Requirement | Target |
| --- | --- | --- |
| NFR-1 | The crate compiles with `cargo build --no-default-features` on a machine with no HID hardware and no `libhidapi` installed. | CI-enforced |
| NFR-2 | The Quad write STALL is swallowed at the transport boundary; Nano write errors and short writes are not hidden. No report is retried because an errored Quad write may already have landed. | Review-enforced |
| NFR-3 | A dead or unresponsive device surfaces as `Error::ReadTimeout` within the requested timeout, not as a spurious write error or a hang. | Hardware smoke runbook |
| NFR-4 | The transport layer depends only on `hidapi` (behind the feature) and the crate's own framing/domain modules; it pulls in no async runtime and no host application. | Leaf-crate discipline |
| NFR-5 | `Transport::request` does not block longer than the caller-supplied timeout; a `deadline` is derived from `Instant::now() + timeout` and the per-read timeout shrinks as the deadline approaches. | Review + test |
| NFR-6 | The udev rule and the `/dev/hidraw*` permission requirement are documented in the README setup section, not assumed in code. | Docs-enforced |

## Acceptance Criteria

- [x] `Transport::open(DeviceKind::QuadCortex)` finds and opens the device at `152A:880A` on a machine with the udev rule installed.
- [x] `Transport::write` swallows the `-1` return from `hid_write` and returns `Ok(())` for a message that the device acted on.
- [x] `Transport::read` with a 2s timeout returns `Error::ReadTimeout` when the device is powered off or unplugged.
- [x] `Transport::request(MessageType::Version, &[], DEFAULT_READ_TIMEOUT)` returns a `Message` whose body decodes to a `VersionMessage` with `device_type == QC` and a non-empty firmware string, on this machine.
- [x] `cargo build --no-default-features -p cortex-rs` succeeds without `hidapi` in the dependency graph.
- [x] The `hid` feature gates `Transport`; the transport-neutral `Session::over` path and framing/domain layers build without it. `Session::open` legitimately uses transport when `hid` is enabled.
- [x] A gzip-compressed frame-level body (payload starting `1f 8b`) is transparently decompressed by `Transport::request` before it is returned.
- [x] The manual and scripted hardware smoke runbooks exercise this layer.

## Non-Goals

- **Frame encoding and reassembly logic.** Owned by zone `110-framing` (`framing.rs`). The transport layer calls `encode_message` and `Frame::parse` / `FrameReassembler`; it does not reimplement them.
- **Protobuf decode and the `CortexMessageType` tag.** Owned by zone `120-proto-schema` and `130-domain-model` (`message.rs`). The transport layer strips the 8-byte trailer via `Message::parse`; it does not own the tag enum.
- **`request_id` correlation and broadcast waiting.** Owned by the session layer (`140-session`). `Transport::request` returns the first reassembled message, full stop.
- **The paced connect handshake (ResetCommsBuffers; Version READ/cache and UPDATE; ModelRepo READ/wait; Connection; 22 subscribe READs; CPULoad CREATE; adaptive settle).** Owned by `140-session`.
- **Background RX thread.** Owned by the session layer (`140`); the raw transport remains synchronous and blocking.
- **Nano Cortex application behaviour.** The Nano codec and held-daemon operations are separate from this raw transport zone; Nano does not use the Quad message envelope, handshake, registry, or grid domain.
- **On-device / ioctl access.** The `qc-stomp-tools` route is out of scope; this project uses the USB HID route exclusively.

## Dependencies

- **`hidapi` crate** (Linux: hidraw backend) - the transport-specific external dependency, gated behind the `hid` feature.
- **`flate2`** - for frame-level gzip decompression in `Transport::request`. Available unconditionally (not behind the feature) because the decode path may also need it.
- **Zone `110-framing`** - `encode_message`, `Frame::parse`, `FrameReassembler`, `Flags`, `ReportId`.
- **Zone `130-domain-model`** - `DeviceKind` (open argument), `Message` (request return type).
- **System udev rule** on Linux: `/etc/udev/rules.d/70-neural-dsp-cortex.rules` granting the logged-in user `0660` + `uaccess` on `hidraw*` for explicit Quad `152a:880a` and Nano `152a:88e7` products. Without the matching entry, enumeration/open fails as either `DeviceNotFound` or a backend permission error.

## Linux Setup (udev rule)

`/dev/hidraw*` is root-only by default. Install a udev rule granting the locally logged-in user access, or `Transport::open` will return `Error::DeviceNotFound`:

```sh
sudo install -m 0644 70-neural-dsp-cortex.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo udevadm trigger --action=add --subsystem-match=hidraw
```

Re-plug the device. The interface number's `hidraw` path is assigned dynamically; see the installation guide for checks that do not assume a numbered node. Nano state, raw amp, bypass and raw FX parameter operations use this transport through the held daemon; that does not imply support for the wider Nano application protocol.

## Future

- **Protocol compatibility probe.** Device identity is cached, but the wire exposes no explicit protocol version. A CorOS update can still break compatibility silently.
- **Wider Nano Cortex application implementation.** Typed current-state, Bluetooth-ownership, raw amp, bypass and raw FX parameter paths are implemented. Save, preset/model replacement, Capture/IR selection and other application operations remain outstanding under NANO-001.

## Glossary

| Term | Definition |
| --- | --- |
| Write STALL | The benign USB status-stage stall on every `SET_REPORT`; `hid_write()` returns `-1` on a write that worked |
| Effective HID ownership | One host process must own the interface; the OS/device does not reliably enforce this and a second open can wedge the first owner |
| HID body | The report payload excluding the 1-byte report ID: 128 bytes on Quad Cortex, 64 on Nano Cortex |
| HID report | The unit at the hidapi boundary: 129 bytes on Quad Cortex, 65 on Nano Cortex |
| Input report | Device-to-host, report ID `0x01` |
| Output report | Host-to-device, report ID `0x02` |
| Dead-device signal | A read timeout (`Error::ReadTimeout`), not a write error, because writes are deliberately stalled |
| Trailer | The 8-byte suffix on a reassembled message carrying the `CortexMessageType` tag as a LE u16 |
| Hardware-verified | Confirmed by this project against a real Quad Cortex on CorOS 4.0.1 |
| Provisional | Not yet verified against real hardware by this project; may work but is not confirmed |
