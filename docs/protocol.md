# The protocol

What we know about the Cortex Control wire protocol, and how we know it.

Almost all of this was established by [`stokes-audio/pyquadcortex`](https://github.com/stokes-audio/pyquadcortex) against real hardware. Findings marked **measured here** are this project's own, collected across several hardware runs against a Quad Cortex running CorOS 4.0.1.

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

### The benign write STALL

The single most important gotcha, and the one that would cost days to work out independently.

Every host-to-device `SET_REPORT` is acted upon and *then* deliberately stalled at the USB status stage. `hid_write()` therefore returns `-1` on a write that worked perfectly. Cortex Control ignores this too - all 273 writes in a captured session stalled.

A client must **swallow write errors** and detect a dead device via read timeouts instead. `cortex` does this in `Transport::write`.

### One effective owner

The protocol requires one effective owner. Quit Cortex Control before using anything else, and expect the reverse while `cortex` has a session open.

**Nothing enforces this, and violating it fails silently.** On Linux a second process opens the device without error. The held session then stops working - every subsequent read on it times out - while the offending command appears to succeed. Nothing errors at the moment of collision; the damage shows on the *next* request.

A client should therefore refuse to open the device when it can tell another owner exists, rather than relying on the OS. A daemon holding a session should take its ownership claim (lock file, socket) **before** the handshake: the handshake is seconds long, and a claim registered afterwards leaves that window unguarded.

Reconnect has the same constraint. Stopping worker threads is not enough if another `Arc` still retains the underlying HID object. A session owner must join readers and explicitly destroy the old handle before attempting the replacement open; in-flight operations must drain behind the same recovery barrier. This implementation has both an exclusivity-aware fake proving that order and a CorOS 4.0.1 unplug/replug run in which the replacement handshake returned to `live` and its first grid read succeeded.

### Nano Cortex compatibility is unestablished

The recovered schema contains `DeviceType.ATMA`, the Nano Cortex codename, but that enum value does **not** establish transport compatibility. Third-party macOS observation in [`deskop-nano-cortex`](https://github.com/rixrix/deskop-nano-cortex) records provisional VID:PID `152A:88E7` and 65-byte HID reports (one report-ID byte plus 64 payload bytes), rather than the Quad Cortex's 129. Its HID interface opened but produced no passive input reports; an unknown handshake may still exist.

This project has not tested a Nano Cortex, and nobody has shown one exchanging this protobuf-plus-trailer protocol over HID. Do not apply the Quad Cortex's report size, framing, handshake, or message semantics to a Nano until hardware establishes each of them.

## Framing

```text
[report_id][len][flags][data ... zero padded]
```

`flags` is `0x40` FIRST, `0x80` LAST, `0xC0` complete, `0x00` middle.

There are **no sequence numbers, no offsets, and no total-length field anywhere**. Reassembly is purely flag-driven: a FIRST frame starts a buffer, middles append, a LAST or COMPLETE emits.

A FIRST frame arriving mid-reassembly means the previous message was lost. The decoder can drop the stale partial and resynchronise, but a subscribed client must invalidate cached continuity because the missing body may have contained state.

## Message envelope

A reassembled message is `protobuf ++ 8-byte trailer`, and **the message-type tag lives in the trailer, not a header**: a little-endian `uint16` `CortexMessageType`. The generated enum has 72 variants, numbered 0 through the terminal sentinel value 71.

Two independent kinds of gzip are in play, and a client needs both in different places:

- **Frame-level**: the reassembled payload starts `1f 8b`. Decompress before protobuf decode.
- **Field-level**: gzip inside a protobuf `bytes` field. The model catalog is the notable case.

## The connect handshake

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

Whether other message types share this convention is not established. If a subscribe of yours is being ignored, this is the first thing to try.

The action cannot be inferred from ordinary CRUD semantics elsewhere either. `pyquadcortex` reports that pinning a model appends with no action field, unpinning uses `DELETE`, and adding a favourite uses `CREATE`. Treat the captured action as part of each message's wire contract, not as boilerplate a generic builder can choose.

## Observing the wire

On Linux, `usbmon` captures everything, and **it works even when the unit is passed through to a VM** - QEMU's passthrough goes out through the host's own USB stack, so the host sees the official client's traffic without installing anything inside Windows. Every Cortex Control figure on this page was obtained that way.

Two traps in the capture itself, each of which makes a capture look one-sided rather than mis-parsed:

- **QEMU splits each 129-byte report into a 128-byte transfer plus a 1-byte transfer.** A parser assuming one transfer holds one report sees a truncated report followed by a stray byte, and reassembles nothing. Treat each direction as a byte stream and cut it into fixed 129-byte reports.
- **Wireshark puts the payload in different fields depending on what it worked out about the device.** `usb.data_fragment` for the control transfers you send; `usbhid.data` once it has seen the descriptors and knows the interface is HID, which happens if your capture includes the device enumerating; `usb.capdata` when it has not. Read all three and take whichever is populated. Asking for one and getting nothing is indistinguishable from a device that said nothing.

Use the binary usbmon interface, via `dumpcap` or `tshark`. The text interface at `/sys/kernel/debug/usb/usbmon/<bus>u` truncates payload data, which drops bytes from the middle of a 128-byte body while still looking like a successful capture.

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

Do not correlate these replies by message type or non-empty body alone. On CorOS 4.0.1, targeted setlist reads return `File{UPDATE}` with exactly 256 unique indices; save acknowledgements return `File{CREATE}` with the target folder and slot index; delete acknowledgements return `File{DELETE}` with the target folder and full preset path. An unrelated folder announcement or one-item mutation must not report a destructive operation as successful. These broadcasts carry no usable request ID, so a delayed acknowledgement from an earlier identical operation cannot be distinguished from the current operation.

Move does not rely on a `File{MOVE}` acknowledgement: prior-art hardware evidence shows that file mutations can land without a reply. After sending, the client polls fresh complete listings until the source slot is empty and the named preset occupies the destination. A materially changed complete listing advances the cache's storage revision even when no mutation acknowledgement arrived, so previously prepared save epochs fail closed. If storage does not converge before the deadline, the move outcome is explicitly unconfirmed and must be inspected rather than retried blindly.

File mutations are **eventually consistent**. `pyquadcortex` observed eleven successful deletes still present in a listing five seconds later; a fresh listing eventually reflected all of them. Do not use a fixed post-write sleep as verification. Poll until the expected listing state appears or a real deadline expires. The device may also de-duplicate a colliding name by truncating it and appending `_N`, so read the stored name back before issuing a later name-addressed delete.

## Capture and IR transfer (provisional)

The file category is known: `FileMessage.type` is 0 for presets, 1 for IRs, and 2 for captures. That selects a library; it does not reveal the capture payload format.

`pyquadcortex` has hardware-verified valid-reference shapes for capture and IR grid blocks: a capture is selected with a string formed from its content hash and display name, while an IR uses its library key. For IRs, the device stores a nonsensical reference byte-for-byte and only renders a warning on the unit, so a successful read-back does not prove the referenced IR exists. Invalid-capture-reference behaviour is unestablished.

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

## The model catalog

**Measured here.** The `ModelRepo` payload is `gzip(tar(ModelRepo.xml))`. On the unit tested: 46,704 bytes gzipped, 558,592 of tar, 556,732 of XML, describing **533 models in 31 categories with 3,809 parameters**.

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

The DSP-budget trap is why `set_block` verifies. **Measured here**, and it corrected a bug worth describing.

The device echoes a `Grid` broadcast naming a cell it accepted, so the obvious check is to wait for that echo and treat its absence as refusal. That is wrong: echo latency varies with how busy the unit is, and straight after a recall it exceeds a 5 s timeout. Placing six expensive blocks in a row, the first three reported "refused" and a read-back showed **all of them present**.

So the echo is a **fast path** and the grid is **ground truth**. When no echo arrives, `cortex` reads the grid back and only reports a refusal if the cell genuinely does not hold the model. It also reports which check confirmed the placement, because "echo confirmed" would misstate things when a read-back was what settled it.

Both paths are now verified. Forcing a zero timeout still confirms correctly via read-back; and filling a preset produced a **genuine** refusal - six blocks at `cpu=3.5` accepted, the seventh and eighth rejected with no echo and a read-back confirming absence.

That gives a rough DSP budget figure of around 21 catalog CPU units for that preset shape. One data point, not a formula.

### `read_preset` recalls, and that resets the scene

There is no side-effect-free way to read a *stored* preset: the device only emits one when it recalls it.

**Measured here**, and it demonstrated itself unprompted: we set scene 1, called `read_preset`, and the scene returned to 0 *by itself* because the recall reset it to the preset's default. A scene-targeted write issued after a `read_preset` therefore lands on the default scene rather than the one you selected.

Use `read_current_preset` (`cortex grid show`) to inspect while editing.

On CorOS 4.0.1, an ordinary recall can emit a full `RecallPreset` followed by sparse `Grid` messages from the same request. An empty USER-slot recall included `chain.input_control[0].sidechain_source_flag=false` after the full four-row preset. A subscribed cache must merge that flag-only delta rather than treating the post-recall stream as an invalid baseline; otherwise the session appears live but loses its current grid immediately after recall.

A malformed report or envelope breaks subscribed-state continuity even when later heartbeats prove the USB link is responsive. Explicit reads can repopulate individual values but cannot prove no update was lost. Treat an invalidated cache as a reconnect condition: block new device operations, drain existing calls, destroy the old handle, perform a complete subscribed handshake in a new generation, and serve cached state only after it returns to `live`.

Prepared saves rely on that continuous subscription as their mutation epoch. Preparation and commit both require `cache.phase == live`; one unchanged generation and `storage_revision` must span the preparation listing and backup read. `unsubscribed`, `seeding`, `incomplete`, or `invalidated` state fails closed even if the target's listing metadata is unchanged.

## Scene labels, colours, copy and swap

Scenes are indexed 0-7 on the wire and displayed as A-H. Their labels and colours are stored in `BinaryPreset.scene_labels` and `BinaryPreset.scene_colors`; an unlabelled scene is one space (`" "`), not an empty string. Colours are ARGB `uint32` values.

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

Some I/O fields must travel in a message by themselves. `pyquadcortex` reports output mute and input impedance mode being silently dropped when another field shared the same port entry, and USB dry/wet being dropped when packed with level. Use one-field messages for those controls and verify by read-back.

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
