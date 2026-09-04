# The protocol

What we know about the Cortex Control wire protocol, and how we know it.

Almost all of this was established by [`stokes-audio/pyquadcortex`](https://github.com/stokes-audio/pyquadcortex) against real hardware. Findings marked **measured here** are this project's own, collected across several hardware runs against a Quad Cortex running CorOS 4.0.1.

For a device-first implementation guide, see [Quad Cortex HID transport](quad-cortex-hid.md) and [Nano Cortex HID transport](nano-cortex-hid.md). This page carries the shared framing model and the deeper Quad Cortex message/session/operation reference.

## Transport

| | |
| --- | --- |
| Bus | USB HID |
| VID:PID | `152A:880A` |
| Interface | 5 |
| Input report ID | `0x01` |
| Output report ID | `0x02` |
| Report size | 128-byte body + report-ID byte = 129 at the hidapi boundary |
| Endpoints | one interrupt IN only; host-to-device goes via `SET_REPORT` on the control pipe |

**Measured here:** of the unit's six USB interfaces, **0 to 4 are USB Audio class and only interface 5 is HID**. ALSA enumerates the device as an ordinary sound card at the same time as we hold the HID interface, which is why an audio-analysis feature could run alongside this crate without contending for the connection.

Independent USB reconstruction in [`quad-cortex-usb-re-notes`](https://github.com/hsaastamoinen/quad-cortex-usb-re-notes) records the complete host write setup packet: request type `0x21`, request `0x09` (`SET_REPORT`), value `0x0202` (output report 2), interface index `0x0005`, and length 129. It also publishes a decoded HID report descriptor matching the report geometry above. This project has independently confirmed the resulting transfers, not those setup fields below `hidapi`.

### The Quad Cortex benign write STALL

The single most important gotcha, and the one that would cost days to work out independently.

Every Quad Cortex host-to-device `SET_REPORT` is acted upon and *then* deliberately stalled at the USB status stage. `hid_write()` therefore returns `-1` on a write that worked perfectly. Cortex Control ignores this too - all 273 writes in a captured session stalled. Nano Cortex writes return normally, and a Nano write error is a real failure.

A Quad client must **swallow write errors** and detect a dead device via read timeouts instead. `cortex` applies this policy only to Quad writes in `Transport::write`; it propagates Nano write errors.

### One effective owner

The protocol requires one effective owner. Quit Cortex Control before using anything else, and expect the reverse while `cortex` has a session open.

**Nothing enforces this, and violating it fails silently.** On Linux a second process opens the device without error. The held session then stops working - every subsequent read on it times out - while the offending command appears to succeed. Nothing errors at the moment of collision; the damage shows on the *next* request.

A client should therefore refuse to open the device when it can tell another owner exists, rather than relying on the OS. A daemon holding a session should take its ownership claim (lock file, socket) **before** the handshake: the handshake is seconds long, and a claim registered afterwards leaves that window unguarded.

Reconnect has the same constraint. Stopping worker threads is not enough if another `Arc` still retains the underlying HID object. A session owner must join readers and explicitly destroy the old handle before attempting the replacement open; in-flight operations must drain behind the same recovery barrier. This implementation has both an exclusivity-aware fake proving that order and a CorOS 4.0.1 unplug/replug run in which the replacement handshake returned to `live` and its first grid read succeeded.

### Nano Cortex shares HID framing, not the Quad envelope

**Hardware-measured on a Nano Cortex on 2026-08-11.** The Nano presents as VID:PID `152A:88E7` on HID interface 5. Its report descriptor is byte-for-byte the Quad Cortex descriptor except that each report body is 64 bytes rather than 128, so hidapi reads and writes are 65 bytes rather than 129. Input remains report ID `0x01`, output remains `0x02`, and the `[report_id][len][flags][data]` layout and `FIRST`/middle/`LAST` reassembly below are shared. A passive five-second HID observation produced no reports.

The Nano's application envelope is different. The BLE current-state request bytes map directly onto one complete HID frame: its BLE length prefix becomes HID `len`, BLE `0xC0` becomes the HID complete flag, and the remaining bytes become frame data. The reply arrived in nine HID reports - one `FIRST`, seven middle, and one `LAST` - which reassembled to 546 meaningful bytes. The reassembled Nano body is its state protobuf followed by a four-byte command-specific footer, not the Quad's eight-byte message-type trailer. The existing Nano state decoder recovered firmware, four of five amp controls, capture and IR assignments, all five bypass states, and all five FX model IDs from that USB reply. A command response with footer `49 00 00 00` was later observed queued ahead of a requested current-state response, so a state reader must skip complete messages with other command footers until it receives footer `02 00 00 00` or the request deadline expires. Current-state requests on one held Nano connection must also be spaced by at least five seconds; this applies to forced pre-mutation safety refreshes as well as polling, because an immediate duplicate can time out without a response.

Nano writes need operation-specific verification rather than one generic assumption. All five raw amp controls accepted typed writes on one held HID connection and read back exactly after six seconds. FX bypass writes (Pre FX 1-2, Post FX 1-3 and Gate) also change and read back on the same held connection: the device sends an immediate 8-byte acknowledgement with footer `73 00 00 00` after each bypass write, and a fresh state read after the measured six-second settle confirms the new value. A second bypass write on the same handle was initially observed to be ignored (2026-08-18), but re-testing on 2026-08-20 could not reproduce that failure across Pre FX 1, Pre FX 2, Post FX 1 and Gate - including a rapid-fire test sending all four toggles in quick succession before waiting. Bypass is therefore exposed with the same read-back verification pattern the amp controls use. One decoder caveat remains: the Gate's "on" state is represented by the absence of field 54 in the state protobuf, so the decoder reports `bypassed = None` (unknown) when the gate is on rather than `Some(false)`; this is a decoder limitation, not a write failure.

Gate reduction uses body `18 0B 20 <varint(percent + 108)> 28 00 1A 00 00 00` for integer percentages from 0 through 100, with the varint growing from one byte to two between 19% and 20%. State field 53 is an optional little-endian fixed32 float decoded as `round(value * 255 - 108)`; absence means unknown, not 0%. On 2026-09-04 a readable 49.8% official-editor baseline decoded as 50%; a direct USB smoke changed it to 51%, independently read back 51%, restored encoded integer 50%, and read back 50%. A held-daemon smoke through an official MCP client then confirmed a same-value Gate write. The integer core USB and daemon/MCP paths are hardware-verified; no sub-percent write contract is claimed.

Raw FX parameter refresh uses `08 03 18 <slot> 89 00 00 00` and returns `08 06 22 <byte-length> <little-endian f32 values> 8A 00 00 00`. The array position is the zero-based write index; the wire carries no model identity, parameter names, units, ranges or enum choices. Before interpreting or writing an index, refresh typed current state when its model metadata is stale; a write should carry the model id the caller observed and compare it with that fresh state before issuing the raw parameter operation. Otherwise the same valid index can silently address a different parameter after the player changes the loaded model. All five editable slots returned non-empty vectors of finite normalized values in a 2026-08-21 hardware pass. One interior Pre FX 1 parameter was changed by `0.01`, independently read back within `0.001`, restored on a fresh transport and verified; a daemon-backed CLI same-value write also passed fresh read-back. The refresh response carries neither the requested slot nor a request ID. Serializing Nano operations prevents overlap but cannot prove whether a delayed response belongs to the current refresh or an earlier slot. After a refresh timeout or malformed response, discard and reopen the HID transport before another request so a late reply cannot satisfy the later operation. Decode the measured `Device is busy!` confirmation on this path, and reject non-finite or out-of-range normalized writes before I/O rather than clamping them. These edits affect heard working state and do not save a preset.

The editor channel is exclusive across transports. While another Bluetooth client was active, both Nano and Quad-shaped HID requests received a Nano confirmation carrying `Device is busy!`; the current-state request succeeded immediately after Bluetooth disconnected. A Quad `Version READ` with the Quad eight-byte trailer then received no reply, so the Quad handshake and message registry must not be applied to the Nano merely by changing PID and report size.

## HID framing

```text
[report_id][len][flags][data ... zero padded]
```

`flags` is `0x40` FIRST, `0x80` LAST, `0xC0` complete, `0x00` middle. These are four exact values, not an open bitmask: reject a byte such as `0x41`, `0x81`, or `0xC1` rather than treating it as FIRST, LAST, or complete because a known bit is present. The data capacity is device-dependent: 126 bytes per Quad report and 62 bytes per Nano report.

There are **no sequence numbers, no offsets, and no total-length field anywhere**. Reassembly is purely flag-driven: a FIRST frame starts a buffer, middles append, a LAST or COMPLETE emits.

A FIRST frame arriving mid-reassembly means the previous message was lost. The decoder can drop the stale partial and resynchronise, but a subscribed client must invalidate cached continuity because the missing body may have contained state.

Because there is no total-length field, a lost LAST frame leaves a client's reassembly buffer with no way to know it will never complete. A later FIRST resynchronises it, but continuation frames can otherwise keep growing the stale partial. `cortex` therefore caps its long-lived subscribed Quad session at 1 MiB; the largest observed reassembled body is a 150,008-byte `LocalBackup` chunk, far below the cap. The session rejects either an in-progress partial or a just-completed body that exceeds the limit before envelope decoding. Enforce a byte cap against the actual buffered length, not a report count multiplied by an assumed per-report size: nothing on the wire obliges a report to fill its declared capacity, so a count-based approximation can reject a legitimate body assembled from many short reports. The finite one-shot Quad and Nano readers are bounded by their request deadline rather than this subscribed-session cap.

## Quad Cortex message envelope

A reassembled message is `protobuf ++ 8-byte trailer`, and **the message-type tag lives in the trailer, not a header**: a little-endian `uint16` `CortexMessageType`. The generated enum has 70 concrete operational values numbered 1 through 70, bracketed by `Undefined=0` and the terminal `NumberOfMessageTypes=71` sentinel. Neither sentinel is a message. Tags outside the recovered enum must remain numeric unknown values rather than being coerced to `Undefined`.

Each concrete tag has a same-purpose generated protobuf struct (`Grid` to `GridMessage`, `IOSettings` to `IOSettingsMessage`, and so on). That structural registry says only how to decode a body; it does not establish that this project understands or safely supports the operation represented by every struct.

Two independent kinds of gzip are in play, and a client needs both in different places:

- **Frame-level**: the reassembled payload starts `1f 8b`. Decompress before protobuf decode.
- **Field-level**: gzip inside a protobuf `bytes` field. The model catalog is the notable case.

**Bound decompression, not just reassembly.** Capping the *compressed* input (this project's reassembly cap is 1 MiB, see the session layer) does not bound the *decompressed* output - gzip's compression ratio is attacker- or fault-influenced, so a small compressed body can still inflate arbitrarily large. This project's client caps frame-level decompression at 8 MiB (comfortably above the 150,008-byte largest body measured on `CorOS` 4.0.1) and field-level catalog decompression separately at 8 MiB (comfortably above the 558,592-byte catalog tar measured on the same firmware), each read through a bounded reader so a malformed or hostile stream cannot allocate past its limit before decoding returns an error. Implement an equivalent bound in any client that decompresses this wire format.

## The Quad Cortex connect handshake

The device **will not push state to a client that has merely opened the pipe**:

1. `ResetCommsBuffers` with a fresh 32-hex `session_id`, sent as a correlated request.
2. `Version` READ. Cortex Control does this before announcing its own, and doing it here rather than later avoids a race described below.
3. `Version` UPDATE announcing `cortex_control_version: "4.0.1"`. The device gates push behaviour on a valid version.
4. `ModelRepo` READ. Load-bearing - see below.
5. `Connection{connected: true}`.
6. 22 subscribe READs.
7. `CPULoad` **CREATE** - not a READ. See [Not everything is subscribed with a READ](#cpuload-create).
8. Settle.

Steps 5 to 7 are fire-and-forget: the device answers them as pushes rather than correlated replies. Steps 2 and 4 wait for their replies before continuing - see [Pace the handshake](#pace-the-handshake).

### The ModelRepo READ is load-bearing

**Measured here.** Step 4 looks like a gratuitous 46 KB fetch in the middle of a handshake, and is the obvious thing to optimise away. We removed it: the handshake dropped from ~35 s to **4.2 s**, and then *every* read failed - `active_scene`, `read_current_preset`, and `list_presets` all timed out. The device gates its push behaviour on that request.

### Handshake cost

A subscribed `cortex` connect completes consistently in **3.8-3.9 seconds**, measured on `CorOS` 4.0.1 after adding the Version READ and pacing the ModelRepo transfer.

Large delays or meaningful variance between identical runs indicate a client-side problem. The device answers control transfers in about 220 microseconds and does not throttle a handshake; a healthy subscribed connect remains below 10 seconds.

### Do not let a read loop starve your writer

A background thread holding the device lock across a blocking read, and reacquiring it immediately on release, will starve a foreground write. Neither `hidapi` nor the device reports anything.

On a capture it looks like this:

```text
48.799  OUT  ResetCommsBuffers    <- first write
48.807  IN   ResetCommsBuffers    <- answered in 8 ms
95.638  OUT  KeepAlive            <- 47 seconds of idle bus
```

The device answers control transfers in about 220 microseconds. Releasing the lock and yielding is not sufficient - the mutex is unfair, and with a backlog to read the reader wins the race. The writer must be able to make the reader stand aside.

Measured effect of fixing it in the intermediate handshake: a 2.2-102.7 s spread became a flat 2.2 s; preset listing changed from 5.4-18.2 s to 5.33-5.38 s. The current subscribed handshake is 3.8-3.9 s because it additionally reads Version and deliberately paces the catalog.

### Pace the handshake

**Wait for a large reply before issuing the next request.** Cortex Control does, and the difference is substantial.

Firing the whole handshake at once - the catalog READ, `Connection`, and the subscribe burst, roughly 24 requests inside a millisecond - makes the device serialise them against a 46 KB transfer. The catalog then stalls behind the pile-up:

| Handshake | `ModelRepo` request to last report |
| --- | --- |
| all requests fired at once | 5.06 s, including a single 4.4 s stall mid-transfer |
| waiting for the catalog first | 0.67 s |
| Cortex Control | 0.65 s |

The reports either side of that 4.4 s stall arrive 0.6 ms apart, so it is not a throughput limit - it is queueing the client created. Cortex Control shows no such gap.

Waiting costs about a second of wall time on the handshake itself and returns it several times over: the catalog is in hand when the handshake completes, rather than still arriving for another four seconds.

### The device sends the host an unsolicited `Version` READ

**Measured here.** A `Version` READ from the host produces *two* inbound `Version` messages:

```text
rx Version (290 bytes)   <- the reply to our READ
rx Version (2 bytes)     <- the device asking US for our version
```

The 2-byte one is `VersionMessage{action: READ}` and decodes as a structurally valid `VersionMessage` with every informational field absent. Since READ replies carry no `request_id`, it is indistinguishable by type from a real reply, so **two concurrent `version()` calls risk one being satisfied by the device's request** - yielding a near-empty message rather than an error.

Do the `Version` READ inside the handshake, before announcing, and keep the result. A READ issued later cannot be told apart from the device's own request, and the handshake has to ask anyway.

## Keeping the session alive

### Send a keepalive every second

Cortex Control sends `KeepAlive` every **1.04 s** (681 over 708 s).

**At a 5 second interval the device stops pushing state.** A subscribed session runs normally for about 40 seconds, then falls silent indefinitely. There is no error and no warning.

At 1 second, a subscribed session is never quiet: zero seconds of silence across a 90 second idle, against Cortex Control's longest inbound gap of 0.11 s over the same test.

A session that goes quiet is a keepalive problem until proven otherwise.

### `GlobalTempo` is a continuous heartbeat, in pairs

Arrives in pairs: about 35 ms between the two messages of a pair, 0.5-0.8 s between pairs. It is the tempo and metronome clock, not state.

- **Exclude it from any settle that waits for inbound silence**, or the settle can never finish.
- **It makes silence a usable liveness signal** - but only at a correct keepalive interval. Silence means the link is down, not that the device is idle.

### Not everything is subscribed with a READ {#cpuload-create}

**`CPULoad` is asked for with `CREATE`, and a READ is silently ignored.**

Every other subscribe in the burst is a plain `READ` and gets answered. `CPULoad` is not. Cortex Control sends `action: CREATE` with a `request_id`, which on the wire is a single field 2 and no action field at all - proto3 omits default values, and `CREATE` is 0:

```text
Cortex Control:  field 2: varint 2      <- request_id, action absent (CREATE)
a plain READ:    field 1: varint 3      <- action = READ, no request_id
```

The device answers the first and ignores the second, with no error either way. Semantically it reads correctly once seen: you are *creating* a subscription whose reply is a continuous stream, not *reading* a value once.

The first push arrives roughly **8 seconds** after the request, not immediately - long enough that an early check looks like failure.

### CPU load: two cores, not four rows

The `CPULoad` push reports a total plus per-chain, per-column load entries. Each entry includes `is_on_core2`; this is the device's only observed core assignment. It establishes that the Quad has **two DSP cores**, not one core per grid row.

The four grid rows are signal-chain and routing lanes. A row can contain cells reported on either core, so no fixed row-to-core mapping should be inferred. The current CLI and MCP surface expose each cell's device-reported second-core flag and aggregate those entries by core.

Moving a block across rows can produce a parallel route, but it changes routing rather than simply migrating work to a known core. Only wire rows 0 and 2 can branch, and the device may create or adjust split/rejoin positions. Treat an attempted reroute as an audible experiment: make one change, fresh-read the grid and CPU mapping, then audition it before attempting another placement.

Whether other message types share this convention is not established. If a subscribe of yours is being ignored, this is the first thing to try.

The action cannot be inferred from ordinary CRUD semantics elsewhere either. `pyquadcortex` reports that pinning a model appends with no action field, unpinning uses `DELETE`, and adding a favourite uses `CREATE`. Treat the captured action as part of each message's wire contract, not as boilerplate a generic builder can choose.

## Observing the wire

On Linux, `usbmon` captures everything, and **it works even when the unit is passed through to a VM** - QEMU's passthrough goes out through the host's own USB stack, so the host sees the official client's traffic without installing anything inside Windows. Every Cortex Control figure on this page was obtained that way.

Two traps in the capture itself, each of which makes a capture look one-sided rather than mis-parsed:

- **QEMU splits each 129-byte report into a 128-byte transfer plus a 1-byte transfer.** A parser assuming one transfer holds one report sees a truncated report followed by a stray byte, and reassembles nothing. Treat each direction as a byte stream and cut it into fixed 129-byte reports.
- **Wireshark puts the payload in different fields depending on what it worked out about the device.** `usb.data_fragment` for the control transfers you send; `usbhid.data` once it has seen the descriptors and knows the interface is HID, which happens if your capture includes the device enumerating; `usb.capdata` when it has not. Read all three and take whichever is populated. Asking for one and getting nothing is indistinguishable from a device that said nothing.

Use the binary usbmon interface, via `dumpcap` or `tshark`. The text interface at `/sys/kernel/debug/usb/usbmon/<bus>u` truncates payload data, which drops bytes from the middle of a 128-byte body while still looking like a successful capture.

## Local backups

**Measured here** from Cortex Control 4.0.1 and a Quad Cortex running CorOS 4.0.1. Creating a local backup sends a 2-byte `LocalBackupMessage` containing only `request_id`; the action is absent and therefore means `CREATE`, not `READ`. The device replies with `UPDATE` messages whose `backup_json` strings must be concatenated in arrival order. Each reply explicitly carries `is_last_chunk`: false for every intermediate chunk and true for the final one.

One measured backup was 1,016,596 JSON bytes split into six 150,000-character chunks and one 116,596-character final chunk. Including protobuf fields, those messages were six 150,008-byte bodies and one 116,604-byte body, carried by 8,072 HID reports. The first chunk arrived 4.688 seconds after the request and the final chunk completed 10.597 seconds after it. There is no chunk index, offset or advertised total, so losing or reordering one message cannot be repaired within the stream; discard the partial backup unless a final chunk arrives after uninterrupted ordered reassembly.

The concatenated value is a JSON object with `author`, `author_id`, `compatibility`, `created`, `creator`, `creator_version`, `name`, `payload`, `payload_hash` and `type` fields. In the measured backup, `payload` was 1,016,248 Base64 characters decoding to 762,184 opaque bytes. Those bytes had no gzip or common archive signature, exposed almost no printable text, and measured 7.999772 bits of Shannon entropy per byte. This is consistent with an encrypted payload, corroborating the older third-party report, but the cipher, key scope and any compression inside the encrypted content remain unestablished. The documented pre-CorOS-3 no-serial decryption path did not produce recognisable plaintext from this CorOS 4.0.1 payload.

This proves a complete opaque backup container can be read over USB. It does **not** prove that an individual Neural Capture or user IR can be extracted from that container, that the backup contains every such item, or that a backup from one unit can be restored to another. Do not present `LocalBackup` as per-capture export until decryption and content inventory establish those claims.

## Saving, renaming, deleting and moving a preset

Save and delete were measured from this project's Cortex Control captures on `CorOS` 4.0.1. The move shape below comes from the MIT-licensed `pyquadcortex` project's Cortex Control capture and was hardware-verified from this Rust implementation on `CorOS` 4.0.1 with a prepared scratch preset moved `7A -> 7B -> 7A`, listing convergence in both directions, storage-revision advancement, and final deletion.

All of them are `FileMessage` with a `FolderInfo` naming the setlist and a single `ProductData` naming the target. Two things are worth knowing before writing any of it:

- **A save does not upload the preset.** It names a destination slot; the device commits whatever is in the working grid. Nothing carries `preset_payload`.
- **Save uses `CREATE`, not `UPDATE`** - and the action field is *absent on the wire*, because `CREATE` is 0 and proto3 omits defaults. Saving over an existing preset is the same `CREATE`.

### Save

```text
FileMessage {
  action:  CREATE          // 0, so absent on the wire
  type:    0
  folder:  FolderInfo {
    key:        "/media/p4/Presets/My Presets"
    is_factory: false
    files: [ ProductData {
      index:      9        // linear slot, (bank - 1) * 8 + letter; 9 is 2B
      name:       "SCRATCH"   // OMIT to keep the existing name
      instrument: 1
    } ]
  }
}
```

**Whether `name` is present is the whole difference between the three save-shaped operations.** Supplying it saves-as or renames; omitting it saves in place under the existing name. Cortex Control's "save", "save as" and "rename" all emit this message; only that field differs.

### Delete

```text
FileMessage {
  action: DELETE           // 2
  type:   0
  folder: FolderInfo {
    key: "/media/p4/Presets/My Presets"
    files: [ ProductData {
      key: "/media/p4/Presets/My Presets/SCRATCH-RENAMED.pb"
    } ]
  }
}
```

Delete addresses its target by **full path**, not by slot index - the opposite of save. A `.pb` extension is appended to the preset name.

### Move

```text
FileMessage {
  action: MOVE             // 4
  type:   0
  folder: FolderInfo {
    key: "/media/p4/Presets/My Presets"
    files: [ ProductData {
      key: "/media/p4/Presets/My Presets/Fictional Source.pb"
    } ]
  }
  to_folder: FolderInfo {
    key: "/media/p4/Presets/My Presets"
    files: [ ProductData {
      index: 9             // linear slot; 9 is 2B
    } ]
  }
}
```

Move addresses the source by **full path** and the destination by **linear slot index**. Only same-setlist moves have been observed. The raw protocol's behaviour when the destination is occupied is unestablished. `cortex-rs` requests a fresh complete listing, resolves the explicitly named source slot to its stored path, and refuses observed occupancy, empty sources, no-op moves, and the factory library before sending `MOVE`. Eventual consistency means the listing cannot prove emptiness, so callers should treat both exact slots as mutable and require explicit confirmation.

### Handling the reply

Each of these is answered by a `File` reply, and a save is followed by a `RecallPreset` push carrying `reason: SAVE` (`RecallPresetReason.SAVE = 2`). That reason is the only thing distinguishing it from an ordinary recall, which matters for a client keeping a cache: a save changes the stored slot without the user having recalled anything.

Do not correlate these replies by message type or non-empty body alone. On CorOS 4.0.1, targeted setlist reads return `File{UPDATE}` with exactly 256 unique indices; save acknowledgements return `File{CREATE}` with the target folder and slot index; delete acknowledgements return `File{DELETE}` with the target folder and full preset path. An unrelated folder announcement or one-item mutation must not report a destructive operation as successful. These broadcasts carry no usable request ID, so a delayed acknowledgement from an earlier identical operation cannot be distinguished from the current operation. A timeout therefore leaves an identical retry ambiguous; inspect a fresh complete listing and do not retry a destructive operation blindly.

Move does not rely on a `File{MOVE}` acknowledgement: prior-art hardware evidence shows that file mutations can land without a reply. After sending, the client polls fresh complete listings until the source slot is empty and the named preset occupies the destination. A materially changed complete listing advances the cache's storage revision even when no mutation acknowledgement arrived, so previously prepared save epochs fail closed. If storage does not converge before the deadline, the move outcome is explicitly unconfirmed and must be inspected rather than retried blindly. The wire request carries neither a listing revision nor a compare-and-swap precondition, so another actor can change storage after the final listing check and before the write. Exact scratch-space authorization limits the consequence but cannot close that interval.

File mutations are **eventually consistent**. `pyquadcortex` observed eleven successful deletes still present in a listing five seconds later; a fresh listing eventually reflected all of them. Do not use a fixed post-write sleep as verification. Poll until the expected listing state appears or a real deadline expires. The device may also de-duplicate a colliding name by truncating it and appending `_N`, so read the stored name back before issuing a later name-addressed delete.

### USER setlists and copy composition

USER setlists are direct children of `/media/p4/Presets`, alongside `My Presets`; nesting one beneath `My Presets` creates an ignored ordinary folder rather than a setlist. Creation uses `File{CREATE, type: 0, folder{key: "/media/p4/Presets/<name>", name: <name>, is_factory: false}}`. Deletion uses the same folder identity with `action: DELETE`. Clients must reject path separators, dot components, the USER root itself, factory paths, and deletion of `My Presets` before writing. Creation and deletion are eventually consistent: use fresh directory listings to identify the actual created key and to poll deletion to absence. A requested name is not the final identity when the device applies collision handling.

The device exposes no host-driven preset copy or setlist duplicate command. Preset copy is destination preparation/backup, then source recall, then ordinary save to the destination, followed by a fresh complete listing to obtain the stored name and instrument. Preparation must precede source recall because recalling an occupied destination afterwards would replace the very grid being copied. Setlist duplication is create followed by one recall/save for each occupied source slot. `BulkOperation` only narrates device-owned progress; replaying it does not duplicate anything. Generated-setlist hardware verification on CorOS 4.0.1 confirmed create, typed-instrument save, copy, recalled audio-state equality after composed duplication, delete convergence and complete cleanup.

A newly subscribed but otherwise healthy cache can begin `Incomplete` when its initial live-grid seed is absent. Before preparing a composed copy, a side-effect-free `RecallPreset{READ}` of the currently loaded grid can supply that baseline and return the cache to `Live`; this repair path passed on hardware. It must not be generalized to `Invalidated`: invalidation means an update may have been lost, so individual reads cannot restore continuity and persistent operations continue to fail closed until a replacement subscribed handshake establishes a new generation.

## Capture and IR selection; transfer remains provisional

The file category is known: `FileMessage.type` is 0 for presets, 1 for IRs, and 2 for captures. That selects a library; it does not reveal the capture payload format.

Read-only variable-length library listings use `File{READ, type, request_id}`. A candidate reply is accepted only when it is `File{UPDATE}`, echoes that request id, and names the requested folder after trailing-slash normalization. This correlation distinguishes a real empty folder from no answer and rejects unrelated folder floods or delayed responses. The schema provides no total item count, last-response flag, or other completion marker, so one correlated response is the observable response boundary rather than wire-level proof of library-wide completeness. On CorOS 4.0.1, repeated capture reads returned stable identical results, and one correlated response arrived for each of the loadable IR library and a user-IR folder.

`pyquadcortex` has hardware-verified valid-reference shapes for capture and IR grid blocks. A capture block uses model 14000 by default (14001 is also observed) and stores its identity at string parameter index 5 as the device library entry's 64-character content-hash key immediately followed by its display name, with no separator. Selecting that string silently resets the block's other parameters to capture defaults, so place and confirm the block first, write index 5 second, then apply every other parameter. A caller-supplied follow-up at reserved index 5 is ambiguous and should be refused.

IR Loader models are 29001 through 29008. Every loader has two slots: slot 0 stores IR PATH at string parameter 2 and IR NAME at 22; slot 1 uses 10 and 23. IR PATH is misleadingly named: it takes the exact non-filesystem `key` returned by the loadable IR library, while IR NAME takes that entry's display name. Write the key first and name second. The device stores nonsensical references byte-for-byte and reports failure only with a warning icon and missing-IR message on the unit, so successful host read-back proves storage but not loadability. Invalid-capture-reference behaviour is unestablished.

Hardware selection tests on CorOS 4.0.1 confirmed the exact capture reference plus a parameter written after selection, and confirmed the exact user-IR key/name pair. A timed on-unit inspection of that real user IR showed no warning, establishing loadability for the selected entry without publishing its identity. Both tests changed only the working copy and restored the stored preset by recall.

Choosing New Neural Capture while a host is connected broadcasts `NeuralCapture{try_to_show_dialog: true}` and waits for `NeuralCapture{UPDATE, show_dialog}`. Replying true hands the flow to the host; without a complete v1/v2 capture UI this makes the unit enter capture state while no usable interface is shown. Remaining silent also suppresses the on-unit flow. A 2026-08-12 CorOS 4.0.1 hardware probe received that broadcast and sent exactly `NeuralCapture{UPDATE, show_dialog: false}`; the unit froze, stopped responding on HID, and rebooted. The response is therefore unsafe and absent from the public client. Causation is not established, but do not retry or infer a graceful decline path. `show_dialog_fail_reason` semantics remain unknown and clients must not invent or send one. Disconnect before using the unit-owned wizard.

IR import is narrowed but unsolved. A candidate `File{CREATE, type: 1, total_bulk_create_count: 1, folder, ir_payload}` request does nothing if `total_bulk_create_count` is absent. Writes spanning 26 HID reports round-tripped exactly, proving the outbound fragmentation path; several candidate payload encodings still produced no imported IR. Repeated multi-kilobyte attempts coincided with the USB link dying until the unit was power-cycled, so destructive probes should be spaced out and run only with recoverable user data.

A separate reference-only project without a repository-wide licence, [`OpenCortex`](https://github.com/VanIseghemThomas/OpenCortex), reports in pre-CorOS-3 material that capture files are encrypted protobufs and locally created captures are keyed using the unit serial. That is a provisional lead to test against current hardware, not an established property of the current wire format. If it still holds, cross-unit import may require a portable re-encoding rather than replaying an exported blob.

## Correlation

Replies are matched **by message type first**, with `request_id` as a consistency check:

- READ replies carry **no** `request_id` echo.
- A state-changing request triggers a cascade of *other-type* messages that all echo its `request_id`. Recalling a preset emits `UndoRedo`, `Grid`, `Scene` and more, all carrying the same id, before the `SetlistPosition` echo arrives.
- So the reply is the first inbound message whose **type** matches, and whose `request_id` matches too *if both carry one*.

When an id-less reply could satisfy several waiters of the same type, the **oldest** wins. Anything else is arbitrary, since the pending map is a hash map.

### Broadcast pushes need a predicate

`RecallPreset` is the important case. The handshake's subscription produces an unsolicited seed push carrying **no** `request_id`, while the push caused by your own recall **echoes** it. Accepting the first `RecallPreset` to arrive returns the preset from the *previous* recall - correct-looking, wrong data.

So a broadcast waiter takes a predicate, and a candidate it rejects leaves the waiter registered.

A recall command must not report completion after merely sending `SetlistPosition{UPDATE}`. The write is acted on before its expected USB stall, but the working grid changes asynchronously. Wait for the `RecallPreset` push carrying the recall's `request_id`; otherwise an immediate grid read can return the previous preset.

### State reads can receive partial pushes

The generated state messages use presence-bearing fields because a push may contain only the part that changed. Send a plain READ and wait for the field the API promises rather than accept the first same-type push. Hardware-verified predicates on CorOS 4.0.1 are `volume` for Master Volume, `status` for Looper, `input_port_id` for Tuner, a non-empty `settings.in_port` for I/O settings, `scene_block_bypass` for General Settings, and `bypassed` for Global EQ.

`Mode` needs two distinct reads: active mode requires `mode` presence, while the configured cycle requires a present, non-empty `available_modes.modes`. A mode-only partial push must not be interpreted as an empty configured cycle.

Recents and Favorites share `RecentsFavorites`. A plain READ asks for Recents, but transient empty pushes can arrive before the list, so an uncorrelated empty Recents push is not a reliable result. Favorites is requested with `is_favorites: true` and a fresh request id. Its reply may omit `is_favorites` and may contain zero items; match the echoed request id instead. The first Favorites request after connection may be dropped, so a bounded retry is appropriate. CorOS 4.0.1 hardware returned Recents and correctly completed an empty Favorites baseline through this request correlation.

Favorites mutations operate on exactly one complete `RecentsFavoritesItem` at a time. Add uses `CREATE`, remove uses `DELETE`, and both set `is_favorites: true`. Require the same-operation echo to contain exactly the requested item, including folder and factory/plugin metadata, so a same-name item from another folder cannot confirm the write. Invented or mismatched metadata can be ignored silently by the device, so callers should pass an item obtained from Recents or Favorites. Add/remove with exact final-baseline restoration passed on CorOS 4.0.1.

PinnedModels READ must carry a fresh request id and match its echo; CorOS 4.0.1 established this correlation for both empty-capable reads and mutation read-back. Mutations contain exactly one model id. Pinning uses the default `CREATE` action, which is omitted on the wire; `UPDATE` is ignored. Pinning appends without de-duplication, while `DELETE` removes every occurrence of the supplied id. Hardware verification pinned one previously unpinned model twice, observed both duplicates, unpinned once, observed no remaining occurrence, and restored the exact baseline.

The Tuner `frequency` field is the reference-frequency offset from 440 Hz, not detected pitch. For example, an offset near `2.0` represents a 442 Hz reference. Upstream testing did not obtain the live needle stream over USB, so a Tuner read reports settings rather than played pitch.

### Global-setting writes

`GeneralSettings` top-level scalar fields are sparse, but its nested `master_volume_assignment`, `global_bypass_cab`, and `global_bypass_ir` messages are replacement groups. Writing one nested flag with its siblings omitted makes those siblings false. Read current state explicitly, merge intent, and write every flag in the affected nested state; restoration must retain complete groups rather than reconstructing them from a partial push.

Do not provide a generic `GeneralSettings` writer to untrusted callers. The schema includes command-shaped power and reset fields, while `internal_midi_clock_enabled` was observed refusing writes. The typed implementation exposes only known writable settings and cannot represent power, reset, updater, reboot/shutdown, factory reset, or internal-MIDI-clock operations. `hold_timing` stores index 0-5 for 500-1000 ms in 100 ms steps. `scene_block_bypass` is the closed enumeration Always/Non-STOMP/Never overwrite = 0/1/2.

Global EQ writes are sparse `parameters{parameter_index,value}` entries. Five bands use stride 5 with Gain/Frequency/Q/Type/Enabled offsets 0/1/2/3/4; Type is one of five list values encoded as index divided by 4, and Enabled uses 1.0 for active. OUT level and assignments are indices 25, 26 and 27. OUT level remains normalized 0-1 because no measured dB mapping exists. The top-level `bypassed` field independently disables the whole EQ.

Mode cycle values 0-2 are base modes and 3-8 are ordered two-row hybrids. A cycle permits at most one hybrid, and a hybrid cannot be its only slot. Value 9 is deliberately refused: upstream hardware accepted and read it back but left the footswitches non-functional. Tuner input accepts only Input 1, Input 2, Input 1/2, Return 1, Return 2, USB 5, and USB 6; combined Return 1/2 was refused. Tuner reference is a finite -15 to +15 Hz offset, corresponding to the unit's 425-455 Hz range.

On CorOS 4.0.1, an automated restoration-first run hardware-verified every exposed readable General Settings, nested assignment/bypass, Global EQ, mode/cycle and Tuner-settings mutation, then independently restored the complete baseline. A 2026-08-26 visual pass confirmed `ShowGigView{UPDATE, show}` opens and closes Gig View. In the same pass, `ShowTuner{UPDATE, show:true}` produced no visible Tuner in two ordinary runs and two runs preceded by an explicit `ShowTuner{READ}` subscription, despite transport success. Physically closing and reopening the Tuner during a synchronized normal subscribed session emitted no unsolicited `ShowTuner` push. Treat this recovered Tuner-visibility shape as a silent no-op, not success. The public client refuses it before I/O until a working Cortex Control host-to-device shape is captured.

## The model catalog

**Measured here.** The `ModelRepo` payload is `gzip(tar(ModelRepo.xml))`. On the unit tested: 46,704 bytes gzipped, 558,592 of tar, 556,732 of XML, describing **533 models in 31 categories with 3,809 parameters**.

`ModelRepo` describes block types and their parameters. Its Neural Capture categories contain capture block types, not an inventory of individual user or factory captures. Individual capture inventory is read separately through request-correlated `File` listings with `type: 2`. This distinction is established by the MIT-licensed `pyquadcortex` implementation and hardware notes, which report that saving a capture does not grow `ModelRepo`; this project has hardware-verified both catalog parsing and separate capture listings on CorOS 4.0.1, but has not yet performed its own before/after capture-save exclusion measurement.

The subscribed state reducer treats a non-empty `NewModels.models` announcement as invalidating the held `ModelRepo`; an empty announcement does not invalidate it. A later non-empty `ModelRepo` payload in the same physical-session generation becomes authoritative. Stream gaps and new generations clear both raw and parsed forms, and old-generation delivery is ignored, so block-name and named-parameter resolution cannot resume from an older payload.

There is deliberately no transparent disk cache. The live handshake's `ModelRepo` READ is load-bearing on CorOS 4.0.1 and must still be fully received and drained before the handshake continues. A held session already serves its in-memory copy in about 0.02 seconds, while `cortex catalog --dump` and `--from-file` provide explicit offline snapshots. CorOS version alone is not a demonstrated content key: installed entitlements may also affect the repository, and whether two same-CorOS entitlement states produce different `ModelRepo` or `NewModels` traffic remains an optional hardware research question.

Structure is `Models > Category > Model > Parameter`.

**A parameter's wire index is positional** - its order within its `Model` element, not any id attribute. Entries of type `empty` and `meter` still occupy an index, so a parser must retain them; dropping them shifts every later parameter and produces writes that land on the wrong control while reading back cleanly.

Parameter types observed: `float` (2,618), `switch` (461), `string` (396), `int` (44), `fader` (48), `meter` (8), `empty` (16).

Some declared ranges are **degenerate** (`min == max`); those are placeholders and no unit conversion is meaningful.

**A stored preset can carry MORE parameters than the catalog describes.** Myth Drive's catalog entry declares three; the stored block carried four. Do not size a parameter array from the catalog, and do not treat an index beyond its range as invalid.

### The catalog carries the vendor's own trademark attribution

Each `Model` may hold a `tm` attribute containing **Neural DSP's own** wording - `Based on Marshall® JCM800®`, `Based on ProCo® Rat®` - present on 318 of 533 models.

`cortex` surfaces it verbatim and never paraphrases it. It concerns other companies' marks and is the vendor's statement, not ours to restate.

## The grid

### Rows are zero-based on the wire and 1-4 on screen

This fails **quietly**: an edit lands on a real row, just not the intended one, and reads back perfectly. The crate uses a `Row` type with distinct `from_wire` and `from_screen` constructors so the convention can never be implicit.

### An edit must be sparse and keyed

A grid edit is a `BinaryPreset` carrying only the elements being changed, each with an explicit `row` (and `column` for a model).

A preset freshly read from a recall carries **no** explicit row, so writing one back wholesale **does nothing**.

### Grid routing uses numeric wire enums and closed host names

`Chain.in_portid` and `Chain.out_portid` remain numeric protobuf fields. The device does not validate every output integer: a meaningless value can be stored and read back cleanly. Host-facing APIs therefore expose closed `GridInputPort` and `GridOutputPort` values, serialised as stable snake-case names, and reject unknown names or raw integers before device I/O.

| Input name | Wire value | Input name | Wire value |
| --- | ---: | --- | ---: |
| `empty` | 0 | `previous_row` | 7 |
| `input1` / `input2` / `input12` | 1 / 2 / 3 | `usb5` / `usb6` / `usb7` / `usb8` | 8 / 9 / 10 / 11 |
| `return1` / `return2` / `return12` | 4 / 5 / 6 | `usb56` / `usb78` / `sidechain_buffer` | 12 / 13 / 14 |

| Output name | Wire value | Output name | Wire value |
| --- | ---: | --- | ---: |
| `empty` | 0 | `usb5` / `usb6` / `usb7` / `usb8` | 10 / 11 / 12 / 13 |
| `xlr12` / `out34` / `send12` | 1 / 2 / 3 | `usb56` / `usb78` | 14 / 15 |
| `xlr1` / `xlr2` / `out3` / `out4` | 4 / 5 / 6 / 7 | `next_row3` / `next_row4` / `next_row34` | 16 / 17 / 18 |
| `send1` / `send2` | 8 / 9 | `multiple` / `usb3` / `usb4` / `usb34` | 19 / 20 / 21 / 22 |

### Traps that fail silently

| Trap | Consequence |
| --- | --- |
| `scene_mode` and a value in one message | The flag is dropped; only the value applies |
| `remove_block` via UPDATE with `hash: 0` | Transmitted and ignored. Only DELETE removes |
| Splitter written to `splitter[]` | Read-only. Write `combined_splitter` |
| Splitter or mixer on an odd row | No such collection; the write does nothing |
| MIDI out via `Grid` | Ignored. Use `MIDISettings` |
| A block exceeding the DSP budget | Accepted on the wire, absent afterwards, no error |
| Load a capture, then expect the block's old parameters | Upstream reports capture selection resets the block to the capture defaults; select it before writing the remaining parameters |
| Place a block into a previously used empty cell | The separate bypass table persists for empty cells, so the new block inherits that cell's old bypass state |
| Add a block while diffing unrelated rows | Combo-box selectors that enumerate blocks keep their selected index but are renormalised when the preset's block count changes |

### Row-level splitter, mixer, output, and gate writes

The following sparse `Grid{UPDATE}` shapes are exact-shape tested and hardware-verified by fresh live read-back on CorOS 4.0.1:

| Control | Writable chain field | Model hash | Valid rows |
| --- | --- | --- | --- |
| Splitter parameter | `combined_splitter[]` | absent | 0 and 2 |
| Mixer parameter | `mixer[]` | 11000 | 0 and 2 |
| Lane output parameter | `output_control[]` | 23000 | 0-3 |
| Input gate parameter | `input_control[]` | 28000 | 0-3 |

Each model carries exactly one indexed `Param`. A value message carries one finite normalised `float_value` in 0..1 and no `scene_mode`. A promotion message carries `scene_mode` and no value. A scene-targeted edit is therefore the same ordered sequence used for ordinary block parameters: promotion when requested, `Scene{UPDATE}`, then value. Packing the flag and value together silently loses the flag.

`combined_splitter` is not a hash-addressed alias for `splitter[]`: its writable shape has no hash, while `splitter[]` is the legacy read view and ignores writes. The hardware test had to read `combined_splitter` to observe the applied value; checking only the legacy view would have produced a false failure. Mixer, lane-output, and input-gate writes do require their fixed hashes in their respective collections. Gate catalogs may include live meter parameters; raw indices carry no writability metadata, so any name-based gate API must resolve through the catalog and reject `Meter` rather than presenting it as a setting.

Split/mix mute is a separate control. Write one `SceneBypass` entry to `split_bypass`; never write `mix_bypass`, which is the reported state. One write changes all eight scene entries even though both schema fields are repeated. Only rows 0 and 2 can carry this control. Splitter, mixer, lane output, input gate and split mute all passed read-back, and recall restored the complete working-copy baseline.

### Preset-local tempo and metronome writes

The Tempo menu is a deliberate exception to the row-keyed grid rule. A sparse `Grid{UPDATE}` carries one model in `BinaryPreset.tempoProgramData`, with hash 25000, no row or column key, and one `Param` whose positional index is explicit. Its value is one finite normalised float in 0..1. This exact shape is hardware-verified on CorOS 4.0.1.

The established screen-control indices are TEMPO 0, LED LIGHT 2, VOLUME 3, MUTE 4, PAN 5, TIME SIGNATURE 6, SUBDIVISIONS 7, SOUND 8, and ROUTING 9. Two catalog names are misleading: index 4 is MUTE even though the catalog calls it START, and index 7 is SUBDIVISIONS on screen but NOTELENGTH in the catalog. Stored tempo parameters are positional and can omit every `Param.index`, so reads must use vector position as the fallback index rather than discarding unindexed entries.

The four established selector lists encode option zero through the last option as `option / (count - 1)`: subdivisions has 4 options, sound 6, routing 5, and time signature 21. The known orders are 1/4, 1/8, 1/8T, 1/16 for subdivisions; Blip, Block, Cowbell, Digital, Drum Kit, Soft Kit for sound; Multi, Headphones, Out 1/2, Out 3/4, Send 1/2 for routing; and the 21 time signatures from 2/4 through grouped 7/8 forms. Changing time signature may also rewrite STEPSTATE parameters 10-22, which hold beat accents, so read-back should verify the target rather than require unrelated parameter equality.

No supported write is assigned to index 1. The catalog calls it TYPE, but changing the Tempo menu's MODE control produced no observed wire traffic in upstream testing. Internal MIDI clock writes likewise lack positive evidence. Neither operation should be inferred from a spare index or from read-only clock traffic.

Hardware verification muted the metronome first, exercised all eight exposed write methods through fresh target read-back, and confirmed that final recall restored the complete `tempoProgramData` baseline. Time-signature verification deliberately ignored the device-owned STEPSTATE rewrite while still requiring the requested signature.

### STOMP, expression, and per-preset MIDI output

The following shapes are exact-shape/fake-link tested and hardware-verified on CorOS 4.0.1 through reversible STOMP/expression working-copy read-back and persistent MIDI save/recall read-back.

A STOMP assignment requires two ordered `Grid` messages. First send `Grid{DELETE, preset{stomp_mode_assignments{row, column}}}`, then `Grid{UPDATE, preset{stomp_mode_assignments{row, column, stomp_index}}}`. UPDATE alone can leave the previous assignment in place. All three scalar keys lack presence, so row 0, column 0, and footswitch A encode as proto3 defaults and must not be treated as absent. `stomp_is_momentary`, `stomp_labels`, and `single_stomp_labels` are maps keyed by footswitch 0-7; each sparse update carries only the selected map entry.

Expression parameter assignment is `Grid{UPDATE, preset{chains{row, models{column, params{index, expression, expression_min, expression_max}}}}}`. Pedal is 1 or 2. The endpoints are finite normalized values in 0..1; minimum above maximum is valid and reverses the sweep. Expression bypass writes both model collections together: `bypass_expression{expression, expression_min:0, expression_max:1}` and `expression_bypass_info{type, invert, delay_ms, latch_emulation}`. Mode numbering is STOP 0, SWITCH 1, HEEL_TOE 2, and delay is 0-5000 ms.

Per-preset MIDI output is not a grid edit. Use trailer message type 8 (`MIDISettings`) with action UPDATE and one nested group: `general_midi_messages{messages{source, msg{...}}}` for footswitch/expression sources, or `preset_load_messages{messages{source:0, msg{...}}}` for preset load. Sources 0-7 are footswitches A-H and 8-9 are expression pedals 1-2. Each source replaces up to 12 messages. MIDI channels are 1-16 and all type-specific data values are 7-bit. Message layouts are CC type 1 `{CC number, value, 0}` for a footswitch, CC type 1 `{CC number, minimum, maximum}` for expression, CC Toggle type 2 `{CC number, minimum, maximum}`, and PC type 3 `{bank MSB, bank LSB, program}`.

The stored read-back is positional: `BinaryPreset.midi_messages_general_v2` is 120 slots arranged as 10 sources by 12 messages, and an all-zero `MidiMessageInfo` is an empty slot. `BinaryPreset.midi_messages` separately holds non-empty messages sent when the preset loads.

List-valued block parameters carry their rendered option names in `Param.dynamic_steps` in the current preset, not reliably in the catalog. Some lists include one option per block, so adding or removing a block changes their cardinality and renormalizes an unchanged selected index. A selected option is stored as `index / (count - 1)`; comparisons must recover the selected index using each preset's own count rather than compare the floats directly. Stored float32 values otherwise need a tolerance. Factory content can store NaN in unused parameter slots, where NaN should match only NaN. `input_control` positional parameter 2 is a sampled gain-reduction meter and must be excluded from settings equality across saves.

A `MIDISettings` READ receives no reply on CorOS 4.0.1. Verification therefore requires saving and re-reading the preset, which is persistent and may emit MIDI on load. With outputs disconnected, the hardware test used low-valued messages in a generated temporary USER setlist and verified footswitch CC, CC Toggle, PC, expression CC and preset-load message families through both typed helpers and the raw 10x12 layout after save/recall. It then deleted all generated storage and restored the original preset.

The DSP-budget trap is why `set_block` verifies. **Measured here**, and it corrected a bug worth describing.

The device echoes a `Grid` broadcast naming a cell it accepted, so the obvious check is to wait for that echo and treat its absence as refusal. That is wrong: echo latency varies with how busy the unit is, and straight after a recall it exceeds a 5 s timeout. Placing six expensive blocks in a row, the first three reported "refused" and a read-back showed **all of them present**.

So the echo is a **fast path** and the grid is **ground truth**. When no echo arrives, `cortex` reads the grid back and only reports a refusal if the cell genuinely does not hold the model. It also reports which check confirmed the placement, because "echo confirmed" would misstate things when a read-back was what settled it.

Both paths are now verified. Forcing a zero timeout still confirms correctly via read-back; and filling a preset produced a **genuine** refusal - six blocks at `cpu=3.5` accepted, the seventh and eighth rejected with no echo and a read-back confirming absence.

That gives a rough DSP budget figure of around 21 catalog CPU units for that preset shape. One data point, not a formula.

### Block moves use `GridMove`, not a sparse `Grid` update

One move is encoded without an action field and without the optional advisory grid snapshot:

```text
GridMove{move{from_row: 0, from_col: 2, to_row: 1, to_col: 6, is_drop: true}}
```

Rows and columns are zero-based on the wire. The source must be occupied and the destination empty. A cross-row move can create or adjust a parallel path; the device computes any split and rejoin columns rather than accepting them in this message. The `grid` snapshot in the schema is advisory and does not drive edits, so hosts should omit it.

`GridMove` broadcasts do not carry enough state to update a complete cached preset safely. Invalidate the cached live grid, issue a fresh `RecallPreset{READ}`, and confirm that the source is empty and the complete source model payload plus bypass state now occupy the destination. `cortex block move` also reads before writing so empty sources, occupied destinations, no-ops and invalid columns are refused before device I/O.

Hardware read-back on CorOS 4.0.1 confirmed a same-row move and reverse preserved every parameter, all eight bypass slots, scene mode, model identity and routing, and a cross-row move transferred the block while clearing its source. That cross-row test began with an existing branch at split 2 / mix 5, which the device retained unchanged; a cross-row move does not gratuitously recompute an already-valid path. Recalling the stored preset restored the original cells and routing.

The discriminating bypass test established why host confirmation cannot trust the subscribed cache immediately after a write: a cache-backed read still showed the old value, while an explicit complete live-grid read observed the applied value. Host-facing parameter, bypass, removal, routing, and split writes now issue that explicit read before reporting success. If the requested state is absent, they return `GridWriteUnconfirmed`, which host surfaces expose as `outcome_unconfirmed`. The mandatory per-call confirmation contract and typed routing passed the official-client MCP hardware smoke against CorOS 4.0.1 on 2026-08-11.

### `read_preset` recalls, and that resets the scene

There is no side-effect-free way to read a *stored* preset: the device only emits one when it recalls it.

**Measured here**, and it demonstrated itself unprompted: we set scene 1, called `read_preset`, and the scene returned to 0 *by itself* because the recall reset it to the preset's default. A scene-targeted write issued after a `read_preset` therefore lands on the default scene rather than the one you selected.

Use `read_current_preset` (`cortex grid show`) to inspect while editing.

On CorOS 4.0.1, an ordinary recall can emit a full `RecallPreset` followed by sparse `Grid` messages from the same request. An empty USER-slot recall included `chain.input_control[0].sidechain_source_flag=false` after the full four-row preset. A subscribed cache must merge that flag-only delta rather than treating the post-recall stream as an invalid baseline; otherwise the session appears live but loses its current grid immediately after recall.

A malformed report or envelope breaks subscribed-state continuity even when later heartbeats prove the USB link is responsive. Explicit reads can repopulate individual values but cannot prove no update was lost. Treat an invalidated cache as a reconnect condition: block new device operations, drain existing calls, destroy the old handle, perform a complete subscribed handshake in a new generation, and serve cached state only after it returns to `live`.

Prepared saves rely on that continuous subscription as their mutation epoch. Preparation and commit both require `cache.phase == live`; one unchanged generation and `storage_revision` must span the preparation listing and backup read. `unsubscribed`, `seeding`, `incomplete`, or `invalidated` state fails closed even if the target's listing metadata is unchanged.

## Scene labels, colours, copy and swap

Scenes are indexed 0-7 on the wire and displayed as A-H. Their labels and colours are stored in `BinaryPreset.scene_labels` and `BinaryPreset.scene_colors`; an unlabelled scene is one space (`" "`), not an empty string. Colours are ARGB `uint32` values.

CorOS 4.0.1 accepts colours outside the unit's built-in scene-colour palette. On 2026-08-11, writing neutral grey `0xFF808080` to scene B and then reconnecting for a fresh complete live-grid read returned exactly `0xFF808080`; recalling the stored preset restored the working copy. This proves arbitrary RGB storage without quantisation. Physical LED rendering and alpha-channel semantics remain visually unverified.

```text
SceneLabel{action: UPDATE, index: 2, label: "Wide Lead"}
SceneColor{action: UPDATE, index: 2, color: 0xFFFF02C2}
SceneCopy{action: UPDATE, from_index: 1, to_index: 4, is_swap: false}
SceneCopy{action: UPDATE, from_index: 2, to_index: 3, is_swap: true}
```

Copy and swap both use `UPDATE`, not the schema's `COPY` or `SWAP` actions. They move the complete scene state, including scene-following parameters, per-scene bypass, label and colour. Hardware read-back on CorOS 4.0.1 confirmed source B copied onto E and scenes C/D exchanged with discriminating parameter values.

The device acts on `SceneCopy.is_swap`, but its acknowledgement omits that flag and therefore looks like a copy even after a successful swap. Do not reduce that acknowledgement into cached parameter state. Invalidate the live preset and issue a fresh `RecallPreset{READ}`; the returned preset is ground truth. Separate `SceneLabel` and `SceneColor` broadcasts do repair metadata, but they cannot repair the copied/swapped parameter and bypass arrays.

## Settings writes are not uniformly sparse

Top-level settings fields behave like sparse updates, but a nested submessage can replace the whole nested value. For structures such as `master_volume_assignment`, read the current value, merge the intended field, and write the complete submessage; sending one flag can silently clear its siblings.

Some I/O fields must travel in a message by themselves. Output mute and input impedance mode can be silently dropped when another field shares the same port entry, and USB dry/wet can be dropped when packed with level. Hardware verification confirms one writable control per `IOSettings{UPDATE}` for every input, output and USB patch. Repeat `input_port_id` or `output_port_id` in every per-port message; send MIDI Thru and each output-pairing flag alone too. Input ids are interleaved with combined entries: Input 1/2 are 1/2, combined Input 1/2 is 3, Return 1/2 are 4/5, and combined Return 1/2 is 6. In particular, Return 1 is not id 3.

Successful dispatch does not confirm an I/O write. An explicit READ can still return stale state, so poll fresh complete reads for the intended value. CorOS 4.0.1 hardware measurement establishes that a complete reply is capability-shaped rather than uniform; absent fields in the following matrix are inapplicable, not incomplete:

| Port kind | IDs | Fields present |
| --- | --- | --- |
| Input | 1, 2 | `level`, `input_zmode`, `input_type`, `ground_lift` |
| Input | 4, 5 | `level`, `ground_lift` |
| Output | 1, 4, 5 | `level`, `ground_lift`, `mute` |
| Output | 2, 6, 7 | `level`, `mute` |
| Output | 8, 9 | `level` |

A restoration-grade I/O read requires exactly those four input and eight output identities with exactly their applicable fields, plus USB `level`/`hp_select`/`dry_wet`, MIDI Thru and both output-pairing flags. Order is not significant. `plugged`, headphone and expression-pedal fields are telemetry rather than part of the writable completion rule. The complete matrix and every applicable mutation passed on CorOS 4.0.1 with outputs disconnected. Discrete selector fields require valid encoded options rather than arbitrary normalized floats, and fresh reads can remain stale briefly, so poll eventual state before declaring failure or restoring. Pairings were exercised last, each member port was restored using only its applicable fields, and an independent final read matched the complete baseline.

An accepted and stored value is not proof that the device supports it. One mode-cycle value observed upstream reads back successfully but leaves the footswitches inoperative; a typed client should refuse that value even though the protobuf accepts it.

## Version field names {#version-field-names}

**Measured here**, by decoding raw protobuf field numbers off the wire rather than trusting the recovered schema's names:

| Wire field | Schema name | Actual content |
| --- | --- | --- |
| 4 | `zenos_git_hash` | `4.0.1` - the CorOS **version**, not a hash |
| 5 | `zenwireless_fw_version` | a 32-hex **checksum** |

Field 5's value is 32 hex characters, which is MD5 length; a git SHA-1 would be 40. So neither field holds a git hash, and the two are **not** swapped - each simply carries something its name does not describe. pyquadcortex renders them identically.

The generated protobuf structs retain the vendor's historical names. Public CLI/JSON views expose descriptive `coros_version` and `wireless_firmware_checksum` fields and document the wire-name mapping.

## No version field on the wire

Nothing in the protocol identifies a schema version, so a CorOS update can silently break a client. Treat everything here as true of CorOS 4.0.1 and verify after an update.
