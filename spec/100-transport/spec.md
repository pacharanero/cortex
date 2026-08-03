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

> Owns the `hidapi` wrapper that puts bytes on the wire to a Neural DSP device and pulls bytes back. Encapsulates the two non-obvious behaviours every consumer of the crate must be able to ignore: the benign write STALL and exclusive HID access. Hardware-verified against `pyquadcortex` (MIT) on a real Quad Cortex.

## References

- [Protocol research note](../../quad-cortex-linux-editor-and-protocol.md) - the authoritative protocol facts (at the parent workspace root).
- [pyquadcortex protocol docs](https://github.com/stokes-audio/pyquadcortex/blob/main/docs/protocol.md) - the MIT-licensed reference this behaviour is ported from.
- [001-overview spec](../001-overview/spec.md) - governing spec, traceability contract, routing index entry for this zone.
- [001-overview design](../001-overview/design.md) [DES-ARCH] - the system context flow map this layer sits at the bottom of.
- [110-framing spec](../110-framing/spec.md) - the frame codec this layer hands raw reports to (`Frame::parse`) and receives encoded reports from (`encode_message`).
- [130-domain-model spec](../130-domain-model/spec.md) - owns `DeviceKind`, which `Transport::open` takes as its argument.
- [140-session spec](../140-session/spec.md) - the planned session layer that will own the background RX thread over this transport.
- [README](../../README.md) - the udev rule and setup walkthrough a Linux user runs before `Transport::open` can succeed.
- Owned source: `crates/cortex-rs/src/transport.rs`.

## Problem Statement

The Cortex Control protocol runs over USB HID. Two of its wire behaviours are non-obvious enough that, if they were not encapsulated in one place, every caller (the CLI, the MCP server, the future Tauri backend) would have to know them and would get them wrong:

1. **The benign write STALL.** Every host-to-device `SET_REPORT` is acted upon by the device and *then* deliberately stalled at the USB status stage, so `hid_write()` returns `-1` on a write that worked. A naive caller treats that as a hard error and aborts. The correct response is to swallow it and use a read timeout as the dead-device signal.
2. **Exclusive HID access.** Linux (and the other OSes) allow one owning process per HID interface, not one connection per call. The MCP server especially must hold a single `Transport` for its lifetime; opening one per tool call deadlocks against itself.

This zone owns the `Transport` struct that wraps `hidapi::HidDevice` and encodes both behaviours, so that every layer above can treat USB as a synchronous request/response pipe without knowing either gotcha.

## Verification Basis

| Claim | Status | Evidence |
| --- | --- | --- |
| VID:PID `152A:880A`, interface 5 | Hardware-verified | Real Quad Cortex on this machine, CorOS 4.0.1 / firmware `d14e`, presenting at `/dev/hidraw7` |
| Input report ID `0x01`, output report ID `0x02`, 128-byte body | Hardware-verified | Matches `pyquadcortex/docs/protocol.md` and re-confirmed by `cortex device version` round-trip on this machine |
| The benign write STALL (`hid_write` returns `-1` on success) | Hardware-verified | Observed on this machine; documented in `pyquadcortex` |
| Swallow write errors, detect dead device via read timeout | Hardware-verified | `cortex device version` succeeds despite `-1` writes; a powered-off device surfaces as `Error::ReadTimeout` |
| `Transport::request` gzip-decompresses frame-level payloads starting `1f 8b` | Hardware-verified | Observed on RecallPreset pushes from `pyquadcortex`; the `version` round-trip does not compress |
| Nano Cortex VID:PID | Provisional | `0x152A:0xFFFF` placeholder in `device.rs`; product ID to be recorded once verified against real hardware |

The `pyquadcortex` offline test suite is a conformance reference but not a substitute for a hardware smoke run. Agent-generated tests must not be the sole basis for accepting transport behaviour.

## User Stories

### Primary Users

CLI users, the MCP server, the future Tauri GUI backend, and downstream crate consumers.

### Stories

**As a** CLI user
**I want** `Transport::request` to swallow the write STALL and return the reassembled reply
**So that** `cortex device version` works without me knowing the device stalls every write.

**As an** MCP server author
**I want** `Transport::open` to take the HID interface exclusively and hold it
**So that** one owning process serves all tool calls and I never deadlock against myself.

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
| FR-1 | `Transport::open(DeviceKind)` opens the first matching device on the bus by VID:PID and holds it exclusively until dropped. | Must Have |
| FR-2 | `Transport::write(&[u8])` splits a message into 128-byte HID frames with FIRST/LAST/COMPLETE/MIDDLE flag bytes and sends each via `hid_write`, swallowing the benign status-stage STALL. | Must Have |
| FR-3 | `Transport::read(Duration)` reads one 129-byte input report (report-ID + 128-byte body) and returns `Error::ReadTimeout` on timeout - the canonical dead-device signal. | Must Have |
| FR-4 | `Transport::request(message_type, payload, timeout)` performs synchronous request/response: encode, write (swallowing the STALL), read frames, reassemble, strip the 8-byte trailer, gzip-decompress if the body starts with `1f 8b`, return a `Message`. | Must Have |
| FR-5 | The transport module is gated behind the `hid` feature flag; `cargo build --no-default-features -p cortex-rs` succeeds with no `hidapi` dependency. | Must Have |
| FR-6 | `Transport` holds a single `hidapi::HidDevice`; it does not open a new connection per call. Drop releases the interface. | Must Have |
| FR-7 | `Transport::open` returns `Error::DeviceNotFound` when no matching device is on the bus, with a message pointing the user at the udev rule and README setup section. | Must Have |
| FR-8 | `Transport::request` returns the first reassembled message, not one correlated by `request_id` (READ replies carry none). Correlation is a later concern owned by the session layer (140). | Should Have |
| FR-9 | Constants `HID_BODY_LEN = 128`, `HID_REPORT_LEN = 129`, and `DEFAULT_READ_TIMEOUT = 2s` are exported as the single source of truth for frame and timeout sizing. | Must Have |
| FR-10 | `DeviceKind::NanoCortex` is accepted by `Transport::open` but labelled provisional; its product ID is a placeholder until hardware-verified. | Should Have |

### Non-Functional Requirements

| ID | Requirement | Target |
| --- | --- | --- |
| NFR-1 | The crate compiles with `cargo build --no-default-features` on a machine with no HID hardware and no `libhidapi` installed. | CI-enforced |
| NFR-2 | The write STALL is swallowed at the transport boundary; no caller above layer 1 ever sees a write error from a successful write. | Review-enforced |
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
- [x] The `hid` feature flag is the only gate on `transport.rs`; nothing in the framing/domain/session layers imports from it.
- [x] A gzip-compressed frame-level body (payload starting `1f 8b`) is transparently decompressed by `Transport::request` before it is returned.
- [ ] A manual hardware smoke runbook exists for this layer (owned by `500-dx-tooling`).

## Non-Goals

- **Frame encoding and reassembly logic.** Owned by zone `110-framing` (`framing.rs`). The transport layer calls `encode_message` and `Frame::parse` / `FrameReassembler`; it does not reimplement them.
- **Protobuf decode and the `CortexMessageType` tag.** Owned by zone `120-proto-schema` and `130-domain-model` (`message.rs`). The transport layer strips the 8-byte trailer via `Message::parse`; it does not own the tag enum.
- **`request_id` correlation and broadcast waiting.** Owned by the planned session layer (`140-session`). `Transport::request` returns the first reassembled message, full stop.
- **The connect handshake (ResetCommsBuffers + Version announce + Connection + 22 subscribe READs).** Owned by `140-session`.
- **Background RX thread.** A future concern for the session layer (`140`); the current transport is synchronous and blocking. See [Future](#future).
- **Nano Cortex-specific behaviour.** The protocol shape is shared; Nano-specific messages and BLE behaviour are provisional until verified against real hardware.
- **On-device / ioctl access.** The `qc-stomp-tools` route is out of scope; this project uses the USB HID route exclusively.

## Dependencies

- **`hidapi` crate** (Linux: hidraw backend) - the only external dependency, gated behind the `hid` feature.
- **`flate2`** - for frame-level gzip decompression in `Transport::request`. Available unconditionally (not behind the feature) because the decode path may also need it.
- **Zone `110-framing`** - `encode_message`, `Frame::parse`, `FrameReassembler`, `Flags`, `ReportId`.
- **Zone `130-domain-model`** - `DeviceKind` (open argument), `Message` (request return type).
- **System udev rule** on Linux: `/etc/udev/rules.d/70-quadcortex.rules` granting the logged-in user `0660` + `uaccess` on `hidraw*` for `152a:880a`. Without it, `Transport::open` returns `Error::DeviceNotFound` because the node is `crw------- root root`.

## Linux Setup (udev rule)

`/dev/hidraw*` is root-only by default. Install a udev rule granting the locally logged-in user access, or `Transport::open` will return `Error::DeviceNotFound`:

```sh
echo 'KERNEL=="hidraw*", ATTRS{idVendor}=="152a", ATTRS{idProduct}=="880a", MODE="0660", TAG+="uaccess"' \
  | sudo tee /etc/udev/rules.d/70-quadcortex.rules
sudo udevadm control --reload-rules
sudo udevadm trigger --action=add --subsystem-match=hidraw
```

Re-plug the Quad Cortex. On this machine the interface-5 node appears at `/dev/hidraw7` as `crw-rw----+` owned by the `hidraw` group with the user ACL applied by `uaccess`. See the README setup section for the full walkthrough.

## Future

- **Background RX thread.** The current transport is synchronous and blocking. The session layer (`140-session`) will introduce a background reader thread that owns the read loop, correlates replies by `request_id`, and broadcasts unsolicited pushes (e.g. parameter changes, preset recalls). `Transport` will expose a `read`-driven hook for that thread; the synchronous `request` path stays for the CLI's fire-and-forget commands.
- **Protocol-version probe.** There is no version field on the wire; a CorOS update can silently break things. The session layer will surface a version probe rather than a hard-coded assumption.
- **Nano Cortex hardware verification.** `DeviceKind::NanoCortex` carries a placeholder product ID (`0xFFFF`) until a real device is tested; the transport API shape is shared, the product ID is not.

## Glossary

| Term | Definition |
| --- | --- |
| Write STALL | The benign USB status-stage stall on every `SET_REPORT`; `hid_write()` returns `-1` on a write that worked |
| Exclusive HID access | One owning process per device interface, not one connection per call |
| HID body | The 128-byte payload portion of a report, excluding the 1-byte report ID |
| HID report | The 129-byte unit at the hidapi boundary: 1-byte report ID + 128-byte body |
| Input report | Device-to-host, report ID `0x01` |
| Output report | Host-to-device, report ID `0x02` |
| Dead-device signal | A read timeout (`Error::ReadTimeout`), not a write error, because writes are deliberately stalled |
| Trailer | The 8-byte suffix on a reassembled message carrying the `CortexMessageType` tag as a LE u16 |
| Hardware-verified | Confirmed against a real Quad Cortex on this machine (CorOS 4.0.1, firmware `d14e`) |
| Provisional | Not yet verified against real hardware by this project; may work but is not confirmed |