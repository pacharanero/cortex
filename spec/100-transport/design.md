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

> Design for the synchronous `Transport` wrapper. The normal client path is `Session` over `HidLink`; `Transport::request` remains a minimal diagnostic path.

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

The device requires one effective owner, but neither Linux nor hidapi reliably enforces it. Hardware testing showed that a second process can open successfully and wedge the held owner on its next request.

### Design choice: one `Transport`, one `HidDevice`, held for the lifetime of the owner

`Transport` owns a single `hidapi::HidDevice`:

```rust
pub struct Transport {
    device: hidapi::HidDevice,
}
```

`Transport::open` enumerates the bus, finds the first device matching `DeviceKind::vid_pid`, and opens it by path. The `HidDevice` is held in the struct; drop releases the interface. There is no `reopen`, no `close`, no per-call connection.

### Design choice: first matching device on the bus

`Transport::open` does not filter by interface number or serial. It returns the first device whose VID:PID matches `DeviceKind::vid_pid`. Multi-device scenarios are deferred; the API shape does not preclude a future serial selector.

```rust
let device = api
    .device_list()
    .find(|info| info.vendor_id() == vid && info.product_id() == pid)
    .ok_or_else(|| crate::Error::DeviceNotFound(...))?;
let device = api.open_path(device.path())?;
Ok(Self { device })
```

### Implications for the MCP server

The MCP server opens no transport. It sends typed requests through `cortex-host` to the held `cortex session` daemon, which owns one `Session` and therefore one link.

### Implications for the CLI

Ordinary CLI commands route through the daemon when present and otherwise use one bounded direct session. Direct paths claim `LocalClaim` before opening HID and refuse while another owner is active.

### Alternatives considered

- **Connection pool.** Rejected: the HID interface does not multiplex. A pool of one is just a held `Transport`.
- **Reopen per call.** Rejected: it deadlocks or races with another owner (the MCP server's own tool calls, or a concurrently-running CLI).
- **Rely on `hidapi` open.** Rejected by hardware: a second open can succeed and damage the existing session. `cortex-host::LocalClaim` provides the atomic host-level exclusion.

## [DES-REQUEST] Synchronous Request/Response

### Behaviour

`Transport::request` is the full transport stack in one call: encode the message, write it (swallowing the STALL), read frames back, reassemble, strip the 8-byte trailer, gzip-decompress if the body starts with the gzip magic, and return a `Message`.

It is a synchronous, blocking diagnostic path. The normal client path uses the background session so replies and unsolicited pushes are correlated and reduced safely.

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

The pure reassembler can resynchronise when a new `FIRST` arrives mid-partial. At session level this proves that a message may have been lost, so the state cache is invalidated before processing continues:

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

The device compresses some payloads at the frame level (the reassembled body starts with `1f 8b`). Decompression happens in `Message::decode` (`130-domain-model`'s message envelope, not framing), which `Transport::request` calls after reassembly, because the gzip wraps the protobuf body, not the trailer that `Message::parse` strips first.

`Message::decode` bounds the decompressed size at `MAX_DECOMPRESSED_MESSAGE_LEN` (8 MiB, comfortably above the 150,008-byte largest reassembled body measured on `CorOS` 4.0.1). It reads all concatenated members through `Read::take(limit + 1)`, so a malformed or hostile gzip stream contributes at most one output byte beyond the limit before the bound is checked, and returns `Error::Decode` rather than retaining expanded output without bound:

```rust
pub(crate) fn bounded_gunzip(
    reader: impl std::io::Read,
    limit: usize,
    context: &str,
) -> crate::Result<Vec<u8>> {
    let read_limit = u64::try_from(limit)
        .ok()
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| {
            crate::Error::Decode(format!("{context}: decompression limit is too large"))
        })?;
    let mut decoder = flate2::read::MultiGzDecoder::new(reader).take(read_limit);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)
        .map_err(|e| crate::Error::Decode(format!("{context}: gzip: {e}")))?;
    if decompressed.len() > limit {
        return Err(crate::Error::Decode(format!(
            "{context}: decompressed size exceeds {limit}-byte limit"
        )));
    }
    Ok(decompressed)
}
```

Field-level gzip (inside protobuf `bytes` fields) is a separate concern owned by the domain layer (`130`), and bounds its own decompressed size independently (`MAX_DECOMPRESSED_CATALOG_LEN`) using the same `bounded_gunzip` helper - see [`130-domain-model`](../130-domain-model/spec.md).

### Design choice: return the first reassembled message

`request` returns the first complete `Message` the reassembler yields. It does not inspect the message type or `request_id`. This is correct for the `version` round-trip (one request, one reply) and for any fire-and-forget CLI command. It is not correct for the full client, which must correlate replies and dispatch unsolicited pushes - that is the session layer's job (`140`), not the transport's.

### Alternatives considered

- **Correlate by `request_id` in the transport.** Rejected: READ replies carry no `request_id`; correlation belongs to the session layer which sees the full message stream.
- **Decompress in the framing layer.** Rejected: the gzip wraps the body after the trailer is stripped, and the framing layer operates on raw frames, not stripped messages. Decompression here keeps the framing layer pure.
- **Non-blocking / async read loop.** Rejected for now: the crate is a leaf with no async runtime. The background RX thread for the session layer (`140`) will own the non-blocking read loop; `request` stays synchronous.

## [DES-CONSTS] Constants

| Constant | Value | Purpose |
| --- | --- | --- |
| `HID_BODY_LEN` | `128` | Public Quad compatibility constant. `DeviceKind::report_geometry` selects 128 for Quad and 64 for Nano. |
| `HID_REPORT_LEN` | `129` | Public Quad compatibility constant. Device-dependent raw transport selects 129 for Quad and 65 for Nano. |
| `DEFAULT_READ_TIMEOUT` | `2s` | The default liveness budget. Chosen to exceed the device's observed reply latency for a `version` round-trip while still failing fast on a dead device. |

The Quad compatibility constants and closed `HidReportGeometry` values are defined in `framing.rs`; transport re-exports the constants and retains `DeviceKind` to select live geometry. The read timeout remains transport-owned.

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

- **The `Transport::request` diagnostic is synchronous and uncorrelated.** Concurrent and subscribed operation uses the implemented session layer.
- **First matching device only.** Multi-device scenarios are deferred; `open` does not filter by interface number or serial.
- **Nano Cortex application runtime support remains partial.** Low-level transport uses hardware-established PID `0x88E7`, 65-byte reports and shared flag framing. The separate four-byte-footer codec and held daemon implement typed state plus hardware-verified amp, Gate reduction, bypass and raw FX parameter operations; wider application operations remain provisional. After a timeout or malformed response, the held path discards and reopens the transport before another request, invalidates the old generation, and requires a fresh state read before serving live data. Quad-envelope requests and sessions reject Nano before USB I/O.
- **No protocol compatibility negotiation.** Session caches identity/version, but a CorOS update can still silently change the wire format.
