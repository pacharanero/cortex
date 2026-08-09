# Prior art: what is already known, and where

The workspace holds seven local reference clones, and one additional non-local project is an upstream source for the Nano Cortex BLE work. These gitignored study clones are not vendored into the product. This page records **what each one knows that we do not**, so the next question gets asked of a document before it gets asked of the hardware.

It exists because of a specific mistake. We spent a session capturing Cortex Control performing a preset save, decoding the bytes, and implementing from them - and `pyquadcortex` had implemented the same operation, with the wire shape written in its docstring, all along. The capture was good verification. It was an expensive way to learn something already written down.

**Read this page, and the source it points at, before running a hardware capture.**

The repos are pinned shallow clones at the parent workspace root and are gitignored: they are for study, not part of the build. They are not referenced by path here, because those paths mean nothing outside one machine.

## Licence posture, in one table

| Project | Licence | What we may do |
| --- | --- | --- |
| `stokes-audio/pyquadcortex` | MIT | **Port freely with attribution.** |
| `rixrix/deskop-nano-cortex` | Apache-2.0 | **Adapt with attribution.** |
| `VanIseghemThomas/qc-stomp-tools` | MIT | Adapt with attribution. |
| `choldy/nano-cortex-web-editor` | MIT | Adapt with attribution. Not vendored; reached via the above. |
| `VanIseghemThomas/OpenCortex` | **no repository-wide licence; mixed file notices** | **Read only.** Cite findings in our own words. Copy nothing. |
| `roelj/qc-extras` | **no repository-wide licence; GPL-3.0-or-later source headers** | Read only until the licensing scope is clarified. |
| `hsaastamoinen/quad-cortex-usb-re-notes` | **none declared** | Read only. |
| `vian21/toneparse` | **none declared** | Read only - and see the correction below. |

Anything ported or adapted from the clearly MIT/Apache-2.0-licensed projects carries its upstream copyright and needs a `NOTICE` entry. The four reference-only repositories lack a clear repository-wide licence: `OpenCortex` mixes unlicensed material with file-level GPL notices, `qc-extras` has GPL-3.0-or-later source headers but no root licence, and the other two declare none. No code, scripts, data, or copied prose from those repositories may be committed here; findings may be cited in our own words.

---

## `pyquadcortex` - the one to check first

A Python library for the Quad Cortex over USB HID, MIT, verified against CorOS 4.0.1. It is the origin of our framing, the write-STALL model, and the recovered `.proto` files. Method counts and coverage refer to the pinned local clone and may change upstream.

**It implements roughly 75 client methods and documents the wire shape and verification level of each exposed operation.** Its `docs/protocol.md` carries an "Operation coverage" table, while `docs/manual-coverage.md` distinguishes supported, partial, unsupported, and inapplicable manual features. Together they are the starting checklist for PROT-006.7 through PROT-006.14; they remove most blank-sheet reverse engineering, but do not turn partial or negative results into completed methods.

Start with `pyquadcortex/docs/protocol.md` (especially "Operation coverage") and `pyquadcortex/docs/manual-coverage.md`; use `pyquadcortex/pyquadcortex/client.py` for the implementation and method docstrings.

### What it has that we do not

Most planned items in PROT-006.4 through PROT-006.15 already have an implementation or a documented investigation there: the remaining reads, `move_block`, the splitter/mixer/lane/gate group, tempo methods, stomp/expression/MIDI, file operations, captures and IRs, global settings, I/O ports, and pinning/favourites. Scene copy/label/colour have now been ported and independently hardware-verified here. Verification varies by operation - read-back, confirmation on the unit, capture only, or a documented negative result - so copy the evidence level as carefully as the wire shape. Its ergonomic helper for selecting a list parameter by option **name** and centralising the `index / (count - 1)` arithmetic is now tracked in PROT-006.15.

### Device behaviour that fails silently

These are the expensive ones. Each cost that project real time, and would cost us the same:

- **`set_capture` silently resets the block's other parameters** to the capture's defaults. Walking parameters in index order loses every knob written before it, with no error. Write parameters *after* loading a capture.
- **A nested submessage write replaces the whole submessage.** Sending one flag of a group leaves the others false - in one case quietly stopping the Master Volume knob governing most outputs. Top-level fields are sparse; nested ones are not. Read, merge, write.
- **Some fields are silently dropped unless sent alone.** Output mute, input impedance mode, and the USB port's dry/wet each vanish when packed with a sibling.
- **The bypass table persists for empty cells**, so a newly placed block inherits whatever bypass state that cell last held.
- **Adding a block rewrites list-parameter values on rows you never touched**, because the denominator depends on the block count.
- **File operations are eventually consistent, and the lag grows with the number of operations** - after eleven deletes, a listing five seconds later still showed all eleven. A fixed sleep produces false negatives; poll for the expected state instead.
- **The device renames on collision** - truncating and appending `_N`, to 20 characters. Since delete is name-addressed, a caller that saves then deletes must read the listing back rather than assume.
- **One mode-cycle value is accepted, reads back, and leaves the footswitches dead.** A device that stores a value is not a device that supports it.
- **A connected host suppresses the unit's own Neural Capture flow.** Merely being connected and silent stops the on-device wizard responding. Our held session (`cortex session start`) will do this.

### Reported limits and negative results

Worth knowing so nobody repeats an exhausted probe: preset descriptive tags were not preserved by any tested route (the unit's own Save As strips them too); master volume writes were ignored; the Tempo menu's MODE control produced no observed wire traffic; the tuner's needle did not stream to the host; no host-driven bulk-copy operation was found; and writing back a whole recalled preset did nothing - only row/column-keyed elements applied. Treat these as upstream findings on CorOS 4.0.1, not timeless impossibilities, and re-test those whose result may depend on the action field.

### Where it is wrong, and why that matters

Its "open questions" section says `CPULoad` never arrives and that DSP headroom therefore cannot be checked before placing a block. **We have disproved that**: `CPULoad` is subscribed with `CREATE`, not `READ`, and pushes about once a second (see the completed PROT-008.6.12 entry). Its attempt used a bare `READ` and adding the type to the subscribe burst - exactly the wrong action field.

Its protocol guide also still says a five-second keepalive is the library default and that the device tolerated twenty seconds idle. **Do not port that timing.** We measured Cortex Control at 1.04 seconds and proved that a five-second interval makes pushes stop silently after about forty seconds. The one-second interval in this project is authoritative until a newer measurement replaces it.

That is a **live lead, not a footnote.** The tuner-meter and internal-MIDI-clock negative results may still depend on an untested action. The IR import probe already used `CREATE`, so its failure cannot be blamed on READ/UPDATE; tracing Cortex Control is still the right way to determine whether its action or payload differs. Given that pinned models need *no* action field and favourites need `CREATE`, the action remains part of each operation's wire contract rather than generic CRUD boilerplate.

### On captures and IRs (PROT-007)

It has never attempted capture export, and its IR *import* attempt failed - but it narrows our unknowns considerably:

- **`FileMessage.type` is a solved category selector**: 0 lists presets, 1 lists IRs, 2 lists captures. The roadmap now records that distinction while leaving the payload container and transfer route unresolved.
- The import request shape is mapped, and a bulk-count field is **mandatory** - without it the device does not react at all, no reply and no error.
- **Outbound fragmentation is proven sound** to at least 26 fragments, so report chunking is not the failure. The remaining variables include payload content, action choice, and other message semantics.
- A capture is referenced by a content hash concatenated with its display name, with no separator; the valid-reference shape is hardware-confirmed. An IR takes a library key rather than a path, and **the device does not validate that IR reference** - nonsense stores back byte-identical and only shows as a warning icon on the unit. Invalid-capture-reference behaviour is unestablished.
- **Safety warning:** during repeated multi-kilobyte import attempts the unit's USB link died and needed a power cycle. Space these out.

---

## `deskop-nano-cortex` - the architectural precedent for the GUI

A Tauri 2 + React app for the Nano Cortex, Apache-2.0. It is the model our GUI zones were written against, and it has more to give than the layout.

The most useful paths are `deskop-nano-cortex/backend/src/domain/nano_state.rs` (capability matrix and honest defaults), `backend/src/app/state.rs` (managed state), `backend/src/lib.rs` (bounded release on close), `scripts/check-traceability.mjs` (the working gate), and `docs/specs/archive/usb-debugging.md` (Nano HID observations).

### For GUI-001 to GUI-005

- **One `AppState` struct of async-mutex fields, `Arc`'d and managed by Tauri.** That is the shape for our harder version of the same problem: holding the *one permitted* exclusive HID connection behind a desktop app.
- **A feature flag gating an entire optional transport, with stub command bodies** returning an error when compiled out, so the command-registration macro is identical in both builds. A direct analogue for our `hid` feature.
- **The window-close handler releases the device handle under a timeout**, explicitly so the device can be claimed again after exit. We need exactly this (see PROT-008.6.3).
- **A hotplug watchdog task** spawned at startup - relevant to reconnect-with-backoff.
- **A testing rule that solves "CI has no hardware"**: the frontend may not import the Tauri API except through wrapper modules, so unit tests mock at that boundary rather than patching globals.

### The idea most worth stealing

Our own instructions say to borrow this project's "honest verified-vs-provisional labelling", but we have never made it a *thing*. There it is a real type: a capability matrix carrying a per-field status - confirmed readable, confirmed writable, inferred, unsupported, unverified - defaulting to unverified, serialised to the frontend, with a unit test asserting the default is honest about what it does not know. Alongside it, a sync-mode enum and per-field confirmed/stale/provisional flags, and a documented vocabulary of allowed and forbidden product claims.

That is portable to GUI-004.2 and would sharpen the MCP tool descriptions too.

### For ENG-004 and ENG-001

It has a **working traceability checker** - the thing our ENG-004 describes but has never started. It resolves both the document and the node id, fails CI on a broken link, warns on a missing back-reference, and carries an explicit extra-files list so configs and workflows are covered rather than only source. Porting it is independent of the GUI and could happen at any time.

It also has version-sync across four manifests with a check mode wired into CI as a drift job - our release script now synchronizes its GUI manifests, while the equivalent CI drift check remains useful future work - and a hardware smoke runbook with a fixed evidence block (date, firmware, app version, OS, per-step pass/fail).

### What it says about the Nano Cortex, and one contradiction

- It records a **Nano USB product id of `0x88E7`** (vendor `0x152A`). Our `DeviceKind` deliberately retains a non-matching `0xFFFF` sentinel until this project verifies the transport. The third-party value is a useful lead, not sufficient evidence to make the client open that device.
- **It records the Nano's HID reports as 65 bytes** - report id plus 64 - rather than the Quad Cortex's 129. The protocol reference and `DeviceKind` documentation now state that the Quad Cortex geometry is not evidence for the Nano; if the third-party observation holds, body size is device-dependent.
- **Nobody has shown the Nano speaking this protobuf protocol over HID at all.** That project reaches it over MIDI and BLE; its HID interface opens but produces no passive reports. `DeviceKind::NanoCortex` therefore keeps a non-matching product-id sentinel, and the project documentation treats `ATMA = 1` in the recovered *Quad Cortex* schema only as a lead.
- Its Nano telemetry work is over **BLE**, with a documented GATT service, a write/notify command-response pair, and a segmented state dump. That is the route if we ever pursue the Nano.

### An additional project, not vendored

Its BLE decode credits its field map to **`choldy/nano-cortex-web-editor` (MIT)**. Adapting that decoder inherits that obligation as well as Apache-2.0. Worth knowing before any Nano work starts, not after.

---

## `quad-cortex-usb-re-notes` - independent corroboration, and four facts

A single README, no code. The author deliberately withheld the packet captures because they contain readable preset and device strings - a discipline we follow for the same reason.

Everything is in `quad-cortex-usb-re-notes/README.md`; search it for `SET_REPORT`, `hidapi`, and `report descriptor` rather than reading it linearly.

Its value is that it reaches our foundations **independently of `pyquadcortex`**: same VID:PID, same interface, same report ids and sizes, the complete-frame flag byte where our framing predicts it, zero-based scene indices, linear preset indices counting across setlists, and normalised little-endian floats rather than UI units.

Four pieces of independent evidence it contributes:

- **The exact `SET_REPORT` setup packet** - request type `0x21`, request `0x09`, value `0x0202`, index `0x0005`, length 129. Anyone reimplementing below `hidapi` needs these, and the index independently pins the interface number. These fields are now recorded in `docs/protocol.md` while remaining third-party observations below this project's `hidapi` boundary.
- **The HID report descriptor**, with a decode matching the geometry now cited in `docs/protocol.md`.
- **Independent, quantified evidence for the benign write STALL, from the other side of the stack.** On macOS, the OS's own counters showed set-report attempts and set-report *failures* incrementing in exact lockstep - every single one "failed" - while the log recorded 129 bytes transferred alongside the stall. A 1:1 ratio observed by a different person on a different operating system is far stronger than "`hid_write` returns -1", and the byte count is the direct proof the write landed.
- **`hidapi` cannot passively observe on macOS**: opening the device after quitting the official client yields nothing, and opening first then launching it also yields nothing while the client works normally. We are Linux-first so this does not bite, but it matters for any macOS build.

One place we are ahead: that author reaches "set-report fails" and stops. The conclusion that the stall is *deliberate and benign* is the better model, and we have it.

---

## `OpenCortex` - one high-value fact, and a minefield

**No repository-wide licence exists, so nothing may be copied from the project as a whole.** Some decryptor files carry file-level GPL notices, while other material remains unlicensed; it also contains Neural DSP's own model-repository data, which its author had no right to license to anyone. Redistributing that would be a problem entirely separate from the licence question. Read it; take nothing.

The capture claim is in `OpenCortex/docs/dev/Captures.md` and `OpenCortex/File-decryption/README.md`. Treat those as evidence to test, never as source material to port.

The fact that justifies keeping it:

- **Neural Captures appear to be encrypted, and user captures keyed to the unit's serial number.** PROT-007 now records this third possibility alongside compression and container questions. If it holds, a capture may not be parseable without deriving a key and may not import to a *different* unit at all, which undercuts the "move between units" motivation.

Treat that as a **provisional, third-party, pre-`CorOS`-3 claim to be tested by trace**, not as fact. Everything in that project predates `CorOS` 3 while we are verified on 4.0.1.

Secondary and lower value: it corroborates that the model catalog is the same artefact we now fetch over the wire, and that it changes across firmware releases - which supports caching it keyed by `CorOS` version. It also describes a consolidated on-device user-data archive containing only personal content, which suggests a single-blob acquisition route as an alternative to per-capture wire reads; our recovered schema does carry backup message types.

The rooting route it documents is exactly what our stated preference for the USB route avoids.

---

## `qc-stomp-tools` and `qc-extras` - exhausted

Both concern running code **on** the unit rather than talking to it, so they matter only if we ever target on-device builds.

The useful paths are `qc-stomp-tools/README.md`, `qc-stomp-tools/src/leds/stomp_led.c`, and `qc-stomp-tools/src/stomps/python-prototypes/stomp.py`; `qc-extras/README.md` contains the cross-compilation and user-mode QEMU notes.

`qc-stomp-tools` (MIT) establishes the on-device input event shape and that LEDs are set by a single ioctl taking an index and RGB. Its one crossover with our plans: the LED index range says the front panel has **12 addressable LEDs**, over 10 footswitches that are also rotaries - hardware ground truth for rendering the control surface.

`qc-extras` establishes the CPU (ARMv7 with NEON/VFPv4, on an Analog Devices SC58x-class board), hence the vendor toolchain and cross prefix, corroborating the above. Its one transferable technique is running cross-compiled ARM binaries on a development machine under user-mode QEMU with the toolchain sysroot as prefix, so on-device code can be iterated without hardware.

Both are now summarised in full. Re-reading them will not yield more.

---

## `toneparse` - not Quad Cortex prior art

It parses **Neural DSP desktop *plugin* presets** and **Logic Pro channel strips**, a different format from Quad Cortex protobuf presets. It does not solve any current Quad Cortex roadmap item.

Its own scope is stated in `toneparse/README.md`, `toneparse/docs/neural_dsp.md`, and `toneparse/docs/logic_pro_cst.md`.

It maps to no roadmap item. The only conceivable use would be importing a desktop-plugin preset and translating it to a grid, which is a semantic-mapping problem rather than a parsing one and is not planned.

It is also by far the largest clone, and the great majority of it is **third-party copyrighted audio-software content** - Logic Pro factory material and plugin presets its author had no right to relicense. Nothing from it may come near this repo.

**Recommend dropping the clone.**

---

## How to use this page

1. **Before a hardware capture**, check `pyquadcortex` - its operation-coverage table and its docstrings, which record captured wire shapes.
2. **Before believing one of its negative results**, check whether the action field was the variable. That is what defeated its `CPULoad` attempt, and we have the tooling to settle it.
3. **Before starting the GUI**, read `deskop-nano-cortex` for the capability matrix, the state shape, and the device-release-on-close handler.
4. **Keep this page current.** A finding that stays in a task result helps nobody twice.
