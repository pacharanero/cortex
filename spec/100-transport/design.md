---
afx: true
type: DESIGN
status: Living
owner: "@pacharanero"
version: "1.0"
created_at: "2026-08-01T17:30:00.000Z"
updated_at: "2026-08-01T17:30:00.000Z"
tags: ["transport", "usb-hid", "hidapi", "stall", "exclusive-access", "request-response"]
spec: spec.md
---

# cortex-rs - Transport Design

> Design for the `Transport` struct and its four methods. The interesting parts are not the I/O itself but the two behaviours that make Cortex Control unlike a normal HID device: the benign write STALL and exclusive interface ownership.

## References

- [spec.md](spec.md) - the requirements this design satisfies.
- [001-overview design](../001-overview/design.md) [DES-ARCH] - the flow map this layer sits at the bottom of (`[Flow.Transport]`).
- [110-framing design](../110-framing/design.md) - the frame codec this layer calls into.
- Owned source: `crates/cortex-rs/src/transport.rs`.

## [DES-STALL] The Benign Write STALL

### Behaviour

Every host-to-device `SET_REPORT` is acted upon by the Quad Cortex and *then* deliberately stalled at the USB status stage. The practical consequence: `hid_write()` returns `-1` (an error) on a write that the device received and processed successfully.

This is not a bug in the device firmware, and it is not fixable from the host. It is the single most important gotcha in the protocol, because a naive caller that treats `hid_write` returning `-1` as a hard error will abort every command.

### Design choice: swallow at the transport boundary

`Transport::write` calls `let _ = self.device.write(&report);` and discards the return value. It returns `Ok(())` unconditionally (the `Result` signature is kept for forward compatibility - e.g. explicit frame-size validation).

```rust
// transport.rs
pub fn write(&self, message: &[u8]) -> crate::Result<()> {
    // ... split into 128-byte frames, set flags ...
    // The write is acted upon and then stalled at the status stage,
    // so hid_write returns an error on a write that worked. Swallow
    // it; a dead device surfaces as a read timeout on the next read.
    let _ = self.device.write(&report);
    // ...
    Ok(())
}
```

`Transport::request` does the same when sending its encoded reports:

```rust
for report in &reports {
    let _ = self.device.write(report);
}
```

### Design choice: read timeout is the dead-device signal

Because writes never produce a reliable error, the only way to detect a dead or unresponsive device is a read timeout. `Transport::read` returns `Error::ReadTimeout(Duration)` when `hidapi`'s `read_timeout` returns `0` bytes within the caller-supplied duration. `DEFAULT_READ_TIMEOUT` (2s) is the crate-wide default.

This contract is encoded in the `Error` enum:

```rust
/// The device read timed out. Because every write is deliberately
/// stalled at the USB status stage, a read timeout is the only
/// reliable signal of a dead or unresponsive device.
#[error("read timeout after {0:?}")]
ReadTimeout(std::time::Duration),
```

### Why not retry or probe?

We do not retry writes on `-1`, because the write succeeded. We do not issue a probe after a swallowed write, because that would add a round-trip to every command and the read side already serves as the liveness check. The contract is: **write is fire-and-forget, read is the liveness signal.**

### Alternatives considered

- **Surface write errors to the caller and let it decide.** Rejected: every caller would have to know the STALL, which defeats the point of encapsulating it. The STALL is a device quirk, not a protocol fact callers should reason about.
- **Retry the write N times before giving up.** Rejected: the write worked; retrying sends a duplicate command. The device is not stateless between commands (it tracks session state), so a duplicate write is not always benign.
- **Distinguish STALL from a real write failure.** Not possible from the host: the USB status-stage stall is indistinguishable from a genuine error at the `hid_write` level.

## [DES-EXCLUSIVE] Exclusive HID Access

### Behaviour

Linux (and the other OSes) allow one owning process per HID interface. A second `open_path` on the same node either fails or silently shares, depending on backend; either way, the MCP server opening one `Transport` per tool call would deadlock against itself or against the CLI.

### Design choice: one `Transport`, one `HidDevice`, held for the lifetime of the owner

`Transport` owns a single `hidapi::HidDevice`:

```rust
pub struct Transport {
    device: hidapi::HidDevice,
}
```

`Transport::open` enumerates the bus, finds the first device matching `DeviceKind::vid_pid`, and opens it by path. The `HidDevice` is held in the struct; drop releases the interface. There is no `reopen`, no `close`, no per-call connection.

### Design choice: first matching device on the bus

`Transport::open` does not filter by interface number or serial. It returns the first device whose VID:PID matches `DeviceKind::vid_pid`. On a machine with one Quad Cortex this is unambiguous (the device presents one HID interface at `/dev/hidraw7` on this machine). Multi-device scenarios are deferred; the API shape (take a `DeviceKind`, return one `Transport`) does not preclude a future `open_nth` or `open_all`.

```rust
let device = api
    .device_list()
    .find(|info| info.vendor_id() == vid && info.product_id() == pid)
    .ok_or_else(|| crate::Error::DeviceNotFound(...))?;
let device = api.open_path(device.path())?;
Ok(Self { device })
```

### Implications for the MCP server

The MCP server (zone `300`) must construct a single `Transport` at startup and hold it for the process lifetime; every tool call reuses it. This is why the safety surface design (AGENTS.md) lists "single owning process for the USB interface" as a built-in invariant. Opening a transport per tool call is a bug, not a pattern.

### Implications for the CLI

The CLI (zone `200`) opens a `Transport` for the duration of one command (e.g. `cortex device version`) and drops it on exit. This is fine: the process is short-lived, and there is no long-lived owner to contend with.

### Alternatives considered

- **Connection pool.** Rejected: the HID interface does not multiplex. A pool of one is just a held `Transport`.
- **Reopen per call.** Rejected: it deadlocks or races with another owner (the MCP server's own tool calls, or a concurrently-running CLI).
- **OS-level advisory lock.** Not needed: `hidapi` open already takes the interface; a second open fails. We rely on that, not on a lockfile.

## [DES-REQUEST] Synchronous Request/Response

### Behaviour

`Transport::request` is the full transport stack in one call: encode the message, write it (swallowing the STALL), read frames back, reassemble, strip the 8-byte trailer, gzip-decompress if the body starts with the gzip magic, and return a `Message`.

It is the path the CLI's `version` command uses today. It is synchronous and blocking. It does not correlate by `request_id` (READ replies carry none); it returns the first reassembled message.

### Design choice: deadline-shrinking read loop

The caller supplies a single `timeout`. `request` derives a deadline (`Instant::now() + timeout`) and shrinks the per-read timeout as the deadline approaches, so a slow device does not overshoot the budget:

```rust
let deadline = std::time::Instant::now() + timeout;
loop {
    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
    if remaining.is_zero() {
        return Err(crate::Error::ReadTimeout(timeout));
    }
    let report = self.read(remaining)?;
    // ... feed to reassembler ...
}
```

### Design choice: reassembler reset on a new FIRST frame

The device interleaves unsolicited pushes (e.g. RecallPreset, parameter changes) with replies. A new `FIRST` frame arriving mid-partial-message drops the stale buffer and starts a new message. This is routine, not an error:

```rust
if frame.flags.is_first() {
    reassembler.reset();
}
if let Some(body) = reassembler.feed(&frame)? {
    let msg = crate::message::Message::parse(&body)?;
    // ...
}
```

### Design choice: frame-level gzip decompression here, not in framing

The device compresses some payloads at the frame level (the reassembled body starts with `1f 8b`). Decompression is done in `Transport::request` after `Message::parse` strips the trailer, because the gzip wraps the protobuf body, not the trailer:

```rust
if msg.body.starts_with(&[0x1f, 0x8b]) {
    use std::io::Read;
    let mut decoder = flate2::read::GzDecoder::new(&msg.body[..]);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)
        .map_err(|e| crate::Error::Decode(format!("gzip: {e}")))?;
    msg.body = bytes::Bytes::from(decompressed);
}
```

Field-level gzip (inside protobuf `bytes` fields) is a separate concern owned by the domain layer (`130`), not here.

### Design choice: return the first reassembled message

`request` returns the first complete `Message` the reassembler yields. It does not inspect the message type or `request_id`. This is correct for the `version` round-trip (one request, one reply) and for any fire-and-forget CLI command. It is not correct for the full client, which must correlate replies and dispatch unsolicited pushes - that is the session layer's job (`140`), not the transport's.

### Alternatives considered

- **Correlate by `request_id` in the transport.** Rejected: READ replies carry no `request_id`; correlation belongs to the session layer which sees the full message stream.
- **Decompress in the framing layer.** Rejected: the gzip wraps the body after the trailer is stripped, and the framing layer operates on raw frames, not stripped messages. Decompression here keeps the framing layer pure.
- **Non-blocking / async read loop.** Rejected for now: the crate is a leaf with no async runtime. The background RX thread for the session layer (`140`) will own the non-blocking read loop; `request` stays synchronous.

## [DES-CONSTS] Constants

| Constant | Value | Purpose |
| --- | --- | --- |
| `HID_BODY_LEN` | `128` | The payload portion of a report, excluding the 1-byte report ID. Bounds the chunk size in `write` and the buffer in `read`. |
| `HID_REPORT_LEN` | `129` | Total hidapi report length (body + report ID). The buffer size passed to `read_timeout`. |
| `DEFAULT_READ_TIMEOUT` | `2s` | The default liveness budget. Chosen to exceed the device's observed reply latency for a `version` round-trip while still failing fast on a dead device. |

These are exported from `transport.rs` and re-used by the framing layer's size assertions and the CLI's timeout default. They are the single source of truth: no other module hard-codes `128`, `129`, or `2_000ms`.

## [DES-FEATURE] Feature Gating

The entire `transport` module is behind the `hid` feature:

```rust
// lib.rs
#[cfg(feature = "hid")]
pub mod transport;
#[cfg(feature = "hid")]
pub use transport::Transport;
```

The `Hid` and `DeviceNotFound` `Error` variants are likewise `#[cfg(feature = "hid")]`. This keeps the leaf-crate discipline: `cargo build --no-default-features -p cortex-rs` produces a pure protocol/domain decode surface with no `hidapi` dependency, suitable for tests, analysis tools, and schema introspection on a machine with no HID hardware.

The dependency arrow points *into* the crate: the CLI, MCP server, and future Tauri backend all depend on `cortex-rs`; none of them depend on `hidapi` directly. The transport is the only place that touches `hidapi`.

## [DES-LIMITS] Known Limitations

- **Synchronous only.** No background RX thread; `request` blocks the caller. The session layer (`140`) will introduce one.
- **No `request_id` correlation.** `request` returns the first reassembled message. A concurrent command stream needs the session layer.
- **First matching device only.** Multi-device scenarios are deferred; `open` does not filter by interface number or serial.
- **Nano Cortex product ID is a placeholder.** `0xFFFF` until hardware-verified.
- **No protocol-version probe.** A CorOS update can silently break the wire format; the session layer will surface a probe.