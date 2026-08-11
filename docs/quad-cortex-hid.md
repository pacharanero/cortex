# Quad Cortex HID transport

This page is the device-level USB HID reference for the Neural DSP Quad Cortex. It describes the transport an independent client must establish before it can exchange Cortex Control messages. See [The protocol](protocol.md) for the message registry, handshake behaviour, operations and domain semantics above this layer.

!!! success "Hardware-verified"

    The transport, framing, envelope and connect handshake described here have been exercised by this project against a real Quad Cortex running CorOS 4.0.1 on Linux. Almost all foundational protocol work originated in the MIT-licensed [`stokes-audio/pyquadcortex`](https://github.com/stokes-audio/pyquadcortex) project and has been re-verified here.

## USB identity

| Property | Quad Cortex |
| --- | --- |
| Vendor ID | `0x152A` |
| Product ID | `0x880A` |
| HID interface | 5 |
| Input report ID | `0x01` |
| Output report ID | `0x02` |
| Report body | 128 bytes |
| Report at the hidapi boundary | 129 bytes including report ID |
| Frame data capacity | 126 bytes after `len` and `flags` |

The Quad is a composite USB device. Interfaces 0 through 4 are USB Audio class interfaces and interface 5 is HID. Audio and HID can operate concurrently because they are separate interfaces; the one-owner rule below concerns the editor HID channel, not the class-compliant audio streams.

On Linux, identify the interface by VID:PID and interface number rather than assuming a stable `/dev/hidrawN` path. The kernel assigns that number dynamically.

## Report descriptor

The HID report descriptor declares one 128-byte input report and one 128-byte output report:

| Descriptor field | Input | Output |
| --- | --- | --- |
| Report ID | `0x01` | `0x02` |
| Report size | 8 bits | 8 bits |
| Report count | 128 | 128 |
| Logical range | 0 to 255 | 0 to 255 |

There is one interrupt IN endpoint. Host-to-device traffic does not use an interrupt OUT endpoint; it is sent as HID `SET_REPORT` on the control pipe.

## Host writes and the benign STALL

Below hidapi, the host write is a class-interface control request with request type `0x21`, request `0x09` (`SET_REPORT`), value `0x0202` (output report 2), interface index `0x0005`, and length 129.

Every Quad Cortex `SET_REPORT` is acted upon and then deliberately stalled at the USB status stage. `hid_write()` therefore returns `-1` for a write that succeeded. A client must swallow write errors at this boundary and use a bounded read timeout to detect a dead device. Treating the write result as ordinary failure makes every working Quad session appear broken.

## Frame shape

Every hidapi report has this shape:

```text
[report_id][len][flags][data ... zero padded to 129 bytes]
```

`len` is the number of meaningful data bytes, not the padded report length. `flags` describes the frame's position in a logical message:

| Flag | Meaning |
| --- | --- |
| `0x40` | first frame |
| `0x00` | middle frame |
| `0x80` | last frame |
| `0xC0` | complete single-frame message |

There are no sequence numbers, offsets or total-message-length fields. A receiver starts a buffer on `FIRST`, appends middles, and emits on `LAST`; a `COMPLETE` frame emits immediately. Missing middle frames can only be detected later through an invalid protobuf or state-continuity policy.

## Quad message envelope

The reassembled frame data is:

```text
[protobuf body][8-byte trailer]
```

The first two trailer bytes are the little-endian `CortexMessageType`; the remaining six bytes are currently unused by this client. The message type is not a header. Some protobuf bodies or nested `bytes` fields are gzip-compressed, so stripping the trailer and decompressing happen at distinct layers.

This eight-byte trailer is Quad-specific. The Nano Cortex shares the HID frame state machine but uses a different application footer and domain protocol; see [Nano Cortex HID transport](nano-cortex-hid.md).

## Session establishment

Opening the HID node does not make the Quad push state. The verified Cortex Control-compatible sequence is:

1. `ResetCommsBuffers` with a fresh session ID.
2. `Version` READ.
3. `Version` UPDATE announcing the client version.
4. `ModelRepo` READ, waiting for the large reply before continuing.
5. `Connection{connected: true}`.
6. The subscription reads.
7. `CPULoad` CREATE.
8. A bounded settle period.

The `ModelRepo` request and pacing are load-bearing. Removing the request or flooding later subscriptions behind its transfer causes apparently healthy connections whose subsequent reads time out. See [The Quad Cortex connect handshake](protocol.md#the-quad-cortex-connect-handshake) for measured timing and failure modes.

## Effective ownership

One process must effectively own the HID interface. Linux may allow a second process to open it; that second open can silently wedge the first session and only reveal the collision on the next request. Claim host ownership before starting the handshake, retain one HID handle, and explicitly destroy it before reconnecting.

The `cortex session` daemon is this project's owner. CLI, MCP and GUI clients use its local IPC boundary rather than each opening HID.

## Implementation status

The Quad transport, framing, session and broad read/write client surface are implemented in `cortex-rs` and hardware-verified on Linux. Windows and macOS host paths remain planned until local IPC, packaging and real-device behaviour are tested on each platform.

Raw USB captures can contain serials, paths, preset names and capture names. Keep captures private and publish decoded structural findings in your own words.
