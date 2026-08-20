# Nano Cortex HID transport

This page records the first hardware-verified Nano Cortex exchange over USB HID and the boundary it establishes for independent clients. It deliberately separates what the Nano shares with the Quad Cortex from what remains Nano-specific.

!!! warning "Read-only codec verified; host integration pending"

    A real Nano Cortex answered read-only USB HID probes on Linux on 2026-08-11. Report geometry, framing, multi-report reassembly, cross-transport ownership and one complete state read are hardware-verified. The typed read-only codec was implemented and hardware-verified on 2026-08-18, but the held daemon and every CLI, MCP and GUI operation remain pending, so this is an implementer reference rather than a support claim.

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

The existing Nano decoder successfully recovered firmware presence, four of five amp controls present in that message, capture and IR assignments, five bypass values and all five FX model IDs. No real identifiers or user-assigned names from that response are published here.

`cortex-rs::nano::decode_current_state` now implements this boundary as a strict, serialisable model with eight ordered roles: Gate, Pre FX 1-2, Capture, IR/Cab and Post FX 1-3. Missing protobuf fields remain absent rather than becoming confident zero values. A fresh hardware read on 2026-08-18 reassembled 10 reports / 594 meaningful bytes and decoded all eight roles with all five amp controls present. The varying size reinforces that clients must use frame flags and declared lengths, never a fixed report count.

## Bluetooth and USB ownership

The editor channel is exclusive across transports. While another Bluetooth client was making changes, USB requests received a valid Nano confirmation whose message was `Device is busy!`. The same current-state request succeeded immediately after the Bluetooth session disconnected.

A host should surface this as an ownership conflict, not a timeout or missing device. A future held daemon must coordinate local USB clients exactly as it does for the Quad while also explaining that a phone or tablet may own the Nano remotely.

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

The Nano application command and field map is adapted from [`rixrix/deskop-nano-cortex`](https://github.com/rixrix/deskop-nano-cortex) (Apache-2.0), which in turn credits [`choldy/nano-cortex-web-editor`](https://github.com/choldy/nano-cortex-web-editor) (MIT). Both are attributed in `NOTICE` and `THIRD-PARTY-NOTICES.md`; the transport and decoder measurements on this page are this project's own hardware observations.
