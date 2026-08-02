# The protocol

What we know about the Cortex Control wire protocol, and how we know it.

Almost all of this was established by [`stokes-audio/pyquadcortex`](https://github.com/stokes-audio/pyquadcortex) against real hardware. Findings marked **measured here** are this project's own, made against a Quad Cortex running CorOS 4.0.1 / firmware `d14e` on 2026-08-02.

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

### The benign write STALL

The single most important gotcha, and the one that would cost days to work out independently.

Every host-to-device `SET_REPORT` is acted upon and *then* deliberately stalled at the USB status stage. `hid_write()` therefore returns `-1` on a write that worked perfectly. Cortex Control ignores this too - all 273 writes in a captured session stalled.

A client must **swallow write errors** and detect a dead device via read timeouts instead. `cortex` does this in `Transport::write`.

### Exclusive access

The device grants the HID interface to one process at a time. Quit Cortex Control before using anything else, and expect the reverse to hold while `cortex` has a session open.

## Framing

```text
[report_id][len][flags][data ... zero padded]
```

`flags` is `0x40` FIRST, `0x80` LAST, `0xC0` complete, `0x00` middle.

There are **no sequence numbers, no offsets, and no total-length field anywhere**. Reassembly is purely flag-driven: a FIRST frame starts a buffer, middles append, a LAST or COMPLETE emits.

A FIRST frame arriving mid-reassembly means the previous message was lost; drop the stale partial and start clean. This is routine rather than exceptional - the device interleaves bursts of pushes.

## Message envelope

A reassembled message is `protobuf ++ 8-byte trailer`, and **the message-type tag lives in the trailer, not a header**: a little-endian `uint16` `CortexMessageType`. 71 types are declared.

Two independent kinds of gzip are in play, and a client needs both in different places:

- **Frame-level**: the reassembled payload starts `1f 8b`. Decompress before protobuf decode.
- **Field-level**: gzip inside a protobuf `bytes` field. The model catalog is the notable case.

## The connect handshake

The device **will not push state to a client that has merely opened the pipe**. Six steps:

1. `ResetCommsBuffers` with a fresh 32-hex `session_id`, sent as a correlated request.
2. `Version` UPDATE announcing `cortex_control_version: "4.0.1"`. The device gates push behaviour on a valid version. Note this is an UPDATE, never a READ - see below.
3. `ModelRepo` READ.
4. `Connection{connected: true}`.
5. 22 subscribe READs.
6. Settle.

### The ModelRepo READ is load-bearing

**Measured here.** Step 3 looks like a gratuitous 46 KB fetch in the middle of a handshake, and is the obvious thing to optimise away. We removed it: the handshake dropped from ~35 s to **4.2 s**, and then *every* read failed - `active_scene`, `read_current_preset`, and `list_presets` all timed out. The device gates its push behaviour on that request.

### Why it can take 35 seconds

**Measured here.** Reassembly throughput during a cold connect:

| Message | Reports | Time | Rate |
| --- | --- | --- | --- |
| `ModelRepo` | 371 | 4551 ms | 82/sec |
| `ModuleStats` | 179 | 119 ms | 1504/sec |
| others | 2-72 | ~1 ms | 1500-3000/sec |

The client sustains 1500+ reports/sec. `ModelRepo` alone trickles at 82/sec because the unit **builds the catalog on demand**. Everything after it arrives at full speed.

Two runs of identical code: **37.2 s cold, 2.2 s warm.** The unit evidently caches what it built.

This is why `cortex` settles **adaptively** - waiting for the inbound stream to fall silent rather than sleeping a fixed period. No fixed value serves both cases: short enough for the warm path leaves the cold path issuing reads into a 46 KB backlog, where replies queue behind the pushes and time out.

### The device sends the host an unsolicited `Version` READ

**Measured here.** A `Version` READ from the host produces *two* inbound `Version` messages:

```text
rx Version (290 bytes)   <- the reply to our READ
rx Version (2 bytes)     <- the device asking US for our version
```

The 2-byte one is `VersionMessage{action: READ}` and decodes as a structurally valid `VersionMessage` with every informational field absent. Since READ replies carry no `request_id`, it is indistinguishable by type from a real reply, so **two concurrent `version()` calls risk one being satisfied by the device's request** - yielding a near-empty message rather than an error.

Harmless in practice because the handshake announces with an UPDATE and never a READ, which is exactly why pyquadcortex does it that way.

## Correlation

Replies are matched **by message type first**, with `request_id` as a consistency check:

- READ replies carry **no** `request_id` echo.
- A state-changing request triggers a cascade of *other-type* messages that all echo its `request_id`. Recalling a preset emits `UndoRedo`, `Grid`, `Scene` and more, all carrying the same id, before the `SetlistPosition` echo arrives.
- So the reply is the first inbound message whose **type** matches, and whose `request_id` matches too *if both carry one*.

When an id-less reply could satisfy several waiters of the same type, the **oldest** wins. Anything else is arbitrary, since the pending map is a hash map.

### Broadcast pushes need a predicate

`RecallPreset` is the important case. The handshake's subscription produces an unsolicited seed push carrying **no** `request_id`, while the push caused by your own recall **echoes** it. Accepting the first `RecallPreset` to arrive returns the preset from the *previous* recall - correct-looking, wrong data.

So a broadcast waiter takes a predicate, and a candidate it rejects leaves the waiter registered.

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

That last one is why `set_block` verifies. **Measured here**, and it corrected a bug worth describing.

The device echoes a `Grid` broadcast naming a cell it accepted, so the obvious check is to wait for that echo and treat its absence as refusal. That is wrong: echo latency varies with how busy the unit is, and straight after a recall it exceeds a 5 s timeout. Placing six expensive blocks in a row, the first three reported "refused" and a read-back showed **all of them present**.

So the echo is a **fast path** and the grid is **ground truth**. When no echo arrives, `cortex` reads the grid back and only reports a refusal if the cell genuinely does not hold the model. It also reports which check confirmed the placement, because "echo confirmed" would misstate things when a read-back was what settled it.

Both paths are now verified. Forcing a zero timeout still confirms correctly via read-back; and filling a preset produced a **genuine** refusal - six blocks at `cpu=3.5` accepted, the seventh and eighth rejected with no echo and a read-back confirming absence.

That gives a rough DSP budget figure of around 21 catalog CPU units for that preset shape. One data point, not a formula.

### `read_preset` recalls, and that resets the scene

There is no side-effect-free way to read a *stored* preset: the device only emits one when it recalls it.

**Measured here**, and it demonstrated itself unprompted: we set scene 1, called `read_preset`, and the scene returned to 0 *by itself* because the recall reset it to the preset's default. A scene-targeted write issued after a `read_preset` therefore lands on the default scene rather than the one you selected.

Use `read_current_preset` (`cortex grid`) to inspect while editing.

## Version field names {#version-field-names}

**Measured here**, by decoding raw protobuf field numbers off the wire rather than trusting the recovered schema's names:

| Wire field | Schema name | Actual content |
| --- | --- | --- |
| 4 | `zenos_git_hash` | `4.0.1` - the CorOS **version**, not a hash |
| 5 | `zenwireless_fw_version` | a 32-hex **checksum** |

Field 5's value is 32 hex characters, which is MD5 length; a git SHA-1 would be 40. So neither field holds a git hash, and the two are **not** swapped - each simply carries something its name does not describe. pyquadcortex renders them identically.

The names are the vendor's and presumably historical. `cortex` keeps them so output maps to the schema, but annotates them.

## No version field on the wire

Nothing in the protocol identifies a schema version, so a CorOS update can silently break a client. Treat everything here as true of CorOS 4.0.1 and verify after an update.
