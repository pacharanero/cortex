# Nano Cortex HID transport

This page records the first hardware-verified Nano Cortex exchange over USB HID and the boundary it establishes for independent clients. It deliberately separates what the Nano shares with the Quad Cortex from what remains Nano-specific.

!!! warning "Capability-specific verification"

    Report geometry, framing, multi-report reassembly, cross-transport ownership, typed state, held-daemon integration, amp, bypass and raw FX parameter operations are hardware-verified on Linux. Gate-reduction writing is implemented across the same surfaces but remains provisional because the connected unit omitted the independently readable original value needed for a reversible hardware test.

## USB identity

| Property | Nano Cortex |
| --- | --- |
| Vendor ID | `0x152A` |
| Product ID | `0x88E7` |
| HID interface | 5 |
| Input report ID | `0x01` |
| Output report ID | `0x02` |
| Report body | 64 bytes |
| Report at the hidapi boundary | 65 bytes including report ID |
| Frame data capacity | 62 bytes after `len` and `flags` |

The Nano is also a composite device with USB Audio, bidirectional USB MIDI and HID interfaces. HID is interface 5. Neither a five-second passive HID read nor a simultaneous passive USB MIDI observation produced traffic; the device answers active requests.

On Linux, identify the device and interface rather than assuming the observed `/dev/hidrawN` number will remain stable. The repository's `70-neural-dsp-cortex.rules` grants access to explicit Quad and Nano products without opening every device from the vendor.

```udev
KERNEL=="hidraw*", ATTRS{idVendor}=="152a", ATTRS{idProduct}=="88e7", MODE="0660", TAG+="uaccess"
```

## Report descriptor

The Nano HID report descriptor is byte-for-byte the Quad Cortex descriptor except for report count:

| Descriptor field | Input | Output |
| --- | --- | --- |
| Report ID | `0x01` | `0x02` |
| Report size | 8 bits | 8 bits |
| Report count | 64 | 64 |
| Logical range | 0 to 255 | 0 to 255 |

That similarity correctly predicted a shared frame state machine, but it does not imply a shared application envelope or domain model.

## Host writes

A Linux hidraw write of one 65-byte output report returned 65 and produced a response. The Quad Cortex's deliberate status-stage write STALL was not observed in these Nano probes. A shared implementation should therefore retain the Quad's error-swallowing behaviour only where required and must not assume that every Neural DSP HID write reports failure.

## Shared frame shape

Nano reports use the same report-level layout and flags as the Quad:

```text
[report_id][len][flags][data ... zero padded to 65 bytes]
```

| Flag | Meaning |
| --- | --- |
| `0x40` | first frame |
| `0x00` | middle frame |
| `0x80` | last frame |
| `0xC0` | complete single-frame message |

The Nano carries at most 62 meaningful data bytes per report. The verified current-state response used a flag-driven multi-report sequence: one `FIRST`, zero or more middle frames, and one `LAST`. The original 2026-08-11 probe measured nine reports (one FIRST, seven middles, one LAST) totaling 546 bytes; a 2026-08-12 re-read measured ten reports (one FIRST, eight middles, one LAST) totaling 574 bytes. The state body size varies with device content (presets, captures, assignments), so the report count is not fixed.

Always stop at `len`. Padding beyond the declared body contained readable stale device and user strings during probing. It is not part of the message, must not enter a decoder, and is another reason raw captures must remain private.

## Nano application envelope

The Nano BLE application frame maps directly onto HID rather than being nested inside it. For the hardware-verified read-only current-state request, the BLE frame:

```text
0C C0 08 03 18 01 20 01 28 01 01 00 00 00
```

becomes this HID report:

```text
[02] [0C] [C0] [08 03 18 01 20 01 28 01 01 00 00 00] [zero padding]
 id   len  flag                    data
```

The reassembled response is a Nano state protobuf followed by a command-specific four-byte footer. The measured state response ended in footer `02 00 00 00`. It is not the Quad Cortex's eight-byte `CortexMessageType` trailer.

The existing Nano decoder successfully recovered firmware presence, four of five amp controls present in that message, capture and IR assignments, five bypass values and all five FX model IDs. The five ids are now resolved through a Nano-specific static table to product-facing model names; this is deliberately not the Quad runtime catalog because at least one shared numeric id has different product semantics. Unknown ids remain explicit and unresolved for forward compatibility. A read-only 2026-08-26 check through the protocol-v20 held daemon resolved all five populated FX slots on the connected unit without an unknown id; the filtered check excluded Capture/IR names. No real identifiers or user-assigned names from that response are published here.

`cortex-rs::nano::decode_current_state` now implements this boundary as a strict, serialisable model with eight ordered roles: Gate, Pre FX 1-2, Capture, IR/Cab and Post FX 1-3. Missing protobuf fields remain absent rather than becoming confident zero values. A fresh hardware read on 2026-08-18 reassembled 10 reports / 594 meaningful bytes and decoded all eight roles with all five amp controls present. The varying size reinforces that clients must use frame flags and declared lengths, never a fixed report count.

Repeated reads need pacing. On the held USB connection, issuing another current-state request immediately after the startup read timed out after five seconds; the next request then succeeded, producing a deterministic fail/succeed pattern when every CLI call wrote afresh. Waiting six seconds before the next request succeeded. The held daemon therefore retains the last decoded snapshot and refreshes from hardware no more often than every five seconds. Faster GUI and CLI polls read that snapshot rather than sending duplicate application requests. This is a measured Nano rule, not the Quad's push-cache behavior.

Working-state writes differ by operation family. On 2026-08-18, all five raw amp controls accepted a typed write on one held connection and independently read back the exact value after six seconds; the reversible Gain test also restored its original value. Amp writes do not save anything. The daemon therefore verifies every amp write with a separately paced current-state request before returning success.

Gate reduction uses application body `18 0B 20 <varint(percent + 108)> 28 00 1A 00 00 00` for integer percentages from 0 through 100. The raw value crosses the one-byte varint boundary between 19% and 20%; callers must encode the complete varint and reject out-of-range input rather than silently clamp it. Current-state field 53 is a little-endian fixed32 float decoded as `round(value * 255 - 108)`. Field 53 is optional: omission means unknown, not 0%. On 2026-08-22 and again on 2026-08-26 the connected unit omitted field 53, so the reversible hardware smoke refused before writing because it could not retain a trustworthy original value. The builder, range validation, daemon read-back contract and host surfaces are implemented and offline-tested, but the write itself remains hardware-provisional until a readable original can be changed, read back, restored and verified.

Gate/FX bypass is now exposed with the same read-back discipline as amp writes. The device sends an immediate 8-byte acknowledgement with footer `73 00 00 00` after each bypass write, and a fresh state read after the measured six-second settle confirms the new value. The earlier observation (2026-08-18) that a second bypass write on the same HID handle was ignored could not be reproduced on 2026-08-20 across Pre FX 1, Pre FX 2, Post FX 1 and Gate, including a rapid-fire test sending all four toggles in quick succession. The daemon verifies every bypass write with a separately paced current-state request before returning success. One decoder caveat: the Gate's "on" state is represented by the absence of field 54, so the decoder reports `bypassed = None` when the gate is on rather than `Some(false)`; this is a decoder limitation, not a write failure.

FX parameter refresh uses `08 03 18 <slot> 89 00 00 00`; the response shape is `08 06 22 <byte-length> <little-endian f32 values> 8A 00 00 00`. The response has neither a requested slot nor a request ID. A client must validate the exact footer, finite normalized values and the measured `Device is busy!` confirmation, and serialize refreshes with other Nano operations. Serialization alone cannot distinguish a delayed response from an earlier refresh of another slot: after a refresh times out or returns malformed traffic, close and reopen the HID transport before issuing another request so the stale reply cannot satisfy it. Normalized write values outside 0.0-1.0 or non-finite values must be rejected before device I/O rather than clamped. **Hardware-verified 2026-08-21:** all five editable slots returned non-empty vectors of finite normalized values through the held daemon. A direct reversible test changed one interior Pre FX 1 parameter by `0.01`, independently read back the new value after two seconds, reopened the transport, restored the original and verified restoration. Daemon-backed CLI and Tauri-backend same-value writes also passed fresh read-back. An official MCP client then discovered every Nano tool and passed typed state plus same-value amp, bypass and FX write/read-back checks. Finally, the rendered native Linux GUI loaded a Pre FX 2 vector, changed one slider by `0.001`, showed the device-confirmed result, restored the original through the same control and showed device/draft equality. These operations change heard working state and do not save a preset.

## Bluetooth and USB ownership

The editor channel is exclusive across transports. While another Bluetooth client was making changes, USB requests received a valid Nano confirmation whose message was `Device is busy!`. The same current-state request succeeded immediately after the Bluetooth session disconnected.

The confirmation is now measured and decoded strictly: it is a 23-byte application message with footer `85 00 00 00`, protobuf field 1 varint `3`, and field 4 containing the exact message `Device is busy!`. `cortex-rs` validates that complete shape before returning the typed `DeviceBusy` error; it does not search arbitrary response bytes for the phrase. With the phone app connected on 2026-08-18, the hardware smoke returned that typed error in 0.07 seconds and `cortex session start --device nano` surfaced the ownership conflict directly.

A host surfaces this as an ownership conflict, not a timeout or missing device. The held daemon coordinates local USB clients through the shared endpoint and explains when a phone or tablet owns the Nano remotely.

## Not the Quad protocol with a smaller report

A Quad-shaped `Version READ` carrying the Quad eight-byte trailer received no reply after Bluetooth was clear. The successful Nano state read required no Quad `ResetCommsBuffers`, version announcement, model catalog read or subscription burst.

The implementation boundary is therefore:

- Share USB discovery, one-owner lifecycle, report IDs, device-dependent frame geometry and the flag-driven reassembler.
- Keep the Quad eight-byte message envelope, handshake, message registry, grid and scenes in the Quad protocol/domain layer.
- Give the Nano its own four-byte-footer codec, fixed-chain domain model and typed operations.
- Reuse the same daemon, CLI, MCP and GUI host infrastructure above those device adapters.

This boundary is what makes one cross-platform, dual-device toolkit realistic without pretending the two products are identical.

## Evidence and privacy

The measurements came from bounded read-only probes with usbmon capture. Captures are not committed because protobuf bodies and even bytes beyond declared frame length can contain serials, user preset names, capture names and other identifying strings. Public examples must use fictional data.

The Nano application command, field map and FX model-name table are adapted from [`rixrix/deskop-nano-cortex`](https://github.com/rixrix/deskop-nano-cortex) (Apache-2.0); its decoder in turn credits [`choldy/nano-cortex-web-editor`](https://github.com/choldy/nano-cortex-web-editor) (MIT). Both are attributed in `NOTICE` and `THIRD-PARTY-NOTICES.md`; the transport and decoder measurements on this page are this project's own hardware observations.
