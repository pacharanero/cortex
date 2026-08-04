# Completed

Finished roadmap items, moved out of [roadmap.md](roadmap.md) so it shows only what is outstanding.

Kept rather than deleted: many carry the measurement that settled a question, and those are the entries most likely to be needed again when something regresses.


## Protocol and Crate (PROT)

### PROT-001: Transport layer (zone 100)
- [x] `Transport::open(DeviceKind)` - find and open the Quad Cortex on the USB bus
- [x] `Transport::write(&[u8])` - send a message, split into HID frames, swallow the STALL
- [x] `Transport::read(Duration)` - read one 129-byte input report
- [x] `Transport::request(message_type, payload, timeout)` - synchronous request/response with reassembly + gzip
- [x] Hardware-verified: `cortex device version` reads CorOS 4.0.1 / firmware d14e from a real Quad Cortex

### PROT-002: Framing layer (zone 110)
- [x] `ReportId`, `Flags`, `Frame`, `FrameReassembler` value types
- [x] `Frame::parse` - strip report ID / len / flags, validate
- [x] `FrameReassembler::feed` - flag-driven reassembly state machine
- [x] `encode_message(message_type, payload)` - trailer + chunk + wrap
- [x] 10 unit tests (round-trip, multi-frame, error cases, encode/decode symmetry)

### PROT-003: Proto schema (zone 120)
- [x] Vendor `Preset.proto` and `ProductionAutomation.proto` (MIT, with SPDX headers)
- [x] `build.rs` compiling via `prost-build`
- [x] Add `package cortex_protobuf_v2` to `Preset.proto` so cross-file `Model` references resolve
- [x] Generated `proto` module with all 71 `CortexMessageType` variants and all message structs

### PROT-004: Domain model - core (zone 130)
- [x] `DeviceKind` enum (QuadCortex, NanoCortex) with `vid_pid()`
- [x] `Message` struct with `parse()` (trailer strip, message-type extraction)
- [x] `TRAILER_LEN = 8` constant
- [x] **PROT-004.5**: `Catalog` - HARDWARE-VERIFIED. Container confirmed as `gzip(tar(ModelRepo.xml))`; parses 533 models, 31 categories, 3809 parameters, with the vendor's `tm` attribution carried verbatim
- [x] **PROT-004.7**: Constants: `UNITY_LEVEL`, `USER_SETLIST_ROOT`, `USER_SETLIST`, `SCENE_UNLABELLED`, `BANKS`, `SLOTS_PER_BANK`, `SETLIST_SLOTS`

### PROT-005: Session layer (zone 140)
- [x] **PROT-005.1**: `Session` struct holding the shared `HidDevice`, a background RX thread, and the pending-request/broadcast-waiter maps
- [x] **PROT-005.2**: RX thread - read frames, reassemble, decode (gzip if needed), dispatch by type tag to waiters; never dies on a malformed message
- [x] **PROT-005.3**: `request(message, timeout)` - assign request_id, register waiter, send, block for correlated reply (type-first, request_id consistency check)
- [x] **PROT-005.4**: `await_broadcast(cls, trigger, timeout, match)` - register type waiter, fire trigger, block for matching broadcast
- [x] **PROT-005.5**: `collect(cls, trigger, seconds, match)` - gather every matching message for a duration. Collectors observe rather than consume, so a message still reaches any waiter
- [x] **PROT-005.6**: Keepalive thread - every 5s send `KeepAlive{UPDATE}`, swallow failures
- [x] **PROT-005.7**: Connect handshake - `ResetCommsBuffers` + `Version UPDATE` (announce `cortex_control_version: "4.0.1"`) + `ModelRepo READ` + `Connection{connected: true}` + 22 subscribe READs + 2s settle
- [x] **PROT-005.8**: `disconnect()` - send `Connection{connected: false}` (best effort)
- [x] **PROT-005.9**: Write serialization - a `Mutex` around device writes so a keepalive cannot interleave between a multi-report message's frames
- [x] **PROT-005.10**: 1 MiB reassembly cap - if the buffer exceeds this without completing, reset (defense against a lost LAST frame)
- [x] **PROT-005.11**: Hardware smoke test - VERIFIED 2026-08-02 against CorOS 4.0.1 / firmware d14e / QA00AB123. Handshake completed in 2.2 s; state pushes flowed; `active_scene`, `read_current_preset`, and `list_presets` all answered; disconnect and thread join clean. Run with `cortex device probe`

### PROT-006: Client API (zone 150)
- [x] **PROT-006.1**: `QuadCortex` struct wrapping `Arc<Session>`, lifecycle (connect/disconnect/close, Drop)
- [x] **PROT-006.2**: `version()` - wired through `Session::request` (correlated by type, no request_id echo)
- [x] **PROT-006.3**: Catalog - `fetch_model_repo` + `Catalog::parse`. HARDWARE-VERIFIED 2026-08-02: parsed the real payload from CorOS 4.0.1 (533 models, 31 categories, 318 with vendor attribution). Container confirmed as `gzip(tar(ModelRepo.xml))` - 46,704 bytes gzipped, 558,592 tar, 556,732 XML. Wired into `cortex preset` so blocks show names, and into `cortex catalog` for search by model name OR by the gear it evokes

### PROT-008: Session performance
- [x] **PROT-008.1**: `ConnectMode::Minimal` - skip the 22-type subscription, which is what makes the device dump 600 KB and is not needed for a targeted read
- [x] **PROT-008.2**: Capture the handshake's `ModelRepo` payload instead of requesting it a second time
- [x] **PROT-008.3**: Name the folder in a `File` READ rather than enumerating all 399
- [x] **PROT-008.4**: Interruptible keepalive sleep, so `stop()` does not wait up to 5 s
- [x] **PROT-008.5**: Reduce the spread on a `File` READ listing. **Resolved, and the diagnosis was wrong from the start.** The spread was not the device delivering lazily; it was our own RX loop starving the writer, so the READ sat unsent. Fixed in `session.rs` by a writer-priority gate. Measured `cortex preset list` before 5.4/8.1/10.1/11.4/18.2 s, after 5.34/5.33/5.38/5.34/5.37 s - the floor unchanged, the tail gone entirely. See the ENG-005 note below for how the wire capture identified it.
  - **Retrying was tried and made it worse**, which in hindsight was the clue. Re-firing the READ added writes, and writes were the thing being starved - so the "fix" fed the actual fault. At the time this was read as the device rebuilding the listing repeatedly, which was plausible and wrong. Recorded here because the measurement was sound even though the explanation was not. Implemented as a re-fire every 3 s with a single waiter held across attempts, measured A/B over 5 runs each of `cortex preset list`:

    | | min | median | max | mean |
    | --- | --- | --- | --- | --- |
    | without retry | 5.4 s | 10.1 s | 18.2 s | 10.6 s |
    | with retry (3 s) | 9.8 s | 36.9 s | 65.1 s | 36.3 s |

    About 3.5x worse. Three of the five retry runs exceeded the baseline's worst run. The ordering confound runs the wrong way to explain it away: the baseline arm ran *second*, so drift would have penalised it, and it still won. The mechanism is plain in hindsight - each re-send makes the device build the whole 256-slot listing again, adding exactly the load that made it slow. A `File` READ is not a cheap poll.
  - Untested variants that are not ruled out: a much longer retry interval (10 s+), or retrying only after evidence the request was dropped rather than on a timer. Neither is worth trying until there is a way to tell "dropped" from "still working", which the protocol does not currently give us.
  - The `pyquadcortex` `wait_for_listing` approach was the original motivation here. Whatever it does, a naive periodic re-READ is not it.
- [x] **PROT-008.13**: **Pace the handshake: wait for the catalog before sending the rest.** Firing the catalog READ, `Connection` and the 22 subscribes together (~24 requests inside a millisecond) makes the device serialise them against a 46 KB transfer, and the catalog stalls behind the queue the client created - measured as a single 4.4 s gap mid-transfer, with the reports either side 0.6 ms apart. Waiting first: **0.67 s**, against Cortex Control's 0.65 s, from 5.06 s. Handshake goes 3.0 s -> 3.8 s (three consecutive runs) and the catalog is cached by the time it completes, where it previously arrived four seconds later.

## Docs (DOCS)

### DOCS-001: Documentation site
- [x] **DOCS-001.1**: Zensical 0.0.52 scaffold, `s/docs`, artifact-based Pages deploy with path filters. Builds clean
- [x] **DOCS-001.2**: `docs/install.md` - udev rule with the reasoning, the exclusive-HID gotcha, `s/install`, completions, and a first check
- [x] **DOCS-001.3**: `docs/walkthrough.md` - every output captured from real hardware, nothing invented. Plus `docs/cli-reference.md`, GENERATED from `--help` by `s/docs-cli-reference` so it cannot drift
- [x] **DOCS-001.4**: `docs/protocol.md` - the wire, the handshake, correlation, the catalog, and the grid traps, with this project's own measurements marked as such
- [x] **DOCS-001.5**: `docs/runbook-hardware-smoke.md` - ten checkpointed steps ending in a restore, with the known gaps listed

## CLI (CLI)

### CLI-001: Scaffold and version
- [x] `cortex device version` - reads device firmware, prints all fields
- [x] `cortex completions <shell>` - bash, zsh, fish, powershell
- [x] `cortex --version` / `-V` - standard version flag
- [x] SIGPIPE reset, `arg_required_else_help`
- [x] Clap derive, thin main.rs, all behaviour in crate

### CLI-002: Format and output
- [x] **CLI-002.1**: `--format text|json` global flag, honoured by every command
- [x] **CLI-002.2**: `cortex device version --format json` - structured JSON. The output types are defined in the CLI rather than serialising the prost types, so the JSON is an interface with stable field names rather than a wire representation. Two fields are renamed to what they actually hold (`coros_version`, `wireless_firmware_checksum`), with the vendor's misleading names recorded in the type's docs
- [x] **CLI-003.10**: `cortex grid show [--params]` - the LIVE grid, read without side effects. Distinct from `cortex preset show --slot X`, which reads a STORED slot and can only do so by recalling it, discarding unsaved edits
- [x] **CLI-003.11**: `cortex block param` / `set-bypass` / `set-block` / `remove-block`, taking rows as the unit LABELS them (1-4)
- [x] **CLI-002.4**: Data on stdout, hints on stderr - every command follows this; progress, warnings, and handshake steps all go to stderr so output stays pipeable

### CLI-003: Preset and scene commands
- [x] **CLI-003.1**: `cortex preset recall --slot <slot> [--setlist <path>] [--factory]`
- [x] **CLI-003.2**: `cortex scene --index <0-7>` - zero-based, where the unit labels scenes A-H
- [x] **CLI-003.3**: `cortex preset show --slot <slot>` - recalls and prints the preset, with each block NAMED via the catalog and the vendor's attribution shown
- [x] **CLI-003.4**: `cortex preset list [--setlist <path>] [--include-empty]`
- [x] **CLI-003.5**: `cortex setlist list` - all 399 folders, via the session's `collect`
- [x] **CLI-003.6**: `cortex device probe` - handshake plus every read path, the hardware smoke test
- [x] **CLI-003.7**: `cortex catalog [--search <text>] [--model <id>] [--dump <file>] [--from-file <file>]`
- [x] **CLI-003.8**: `CORTEX_TRACE=1` - stderr tracing of inbound traffic and handshake steps

### CLI-005: Noun-primitive command redesign
  - [x] **CLI-005.1**: Vocabulary settled on evidence rather than taste: **block**, not module. The vendor-facing material in the vendored prior art uses *block* 310 times against *module* 35, including `pyquadcortex`'s manual-coverage notes, and our own docs already said block 29 times. The wire's own names (`Model`, `ModuleStats`) stay in the protocol docs. Clean break, no legacy aliases - nothing is released. The held session became `cortex session start|status|stop`, keeping `connect` as a visible alias since it is the word the protocol docs use
  - [x] **CLI-005.2**: Restructured to noun-then-verb: `session`, `preset`, `setlist`, `grid`, `block`, `row`, `device`, plus `scene`, `catalog`, `completions`, `decode-trace`. Single-letter aliases where they do not collide (`s p sl g b r d sc c`). Grouping `version` under `device` also removes a real ambiguity with `--version`, which is the CLI's own. Adds a global `--zero-based` so `--row` takes 0-3 rather than the 1-4 printed on the unit - scripts and agents usually hold a zero-based index already, and converting by hand is the arithmetic that silently edits the wrong row
  - [x] **CLI-005.3**: Command reference regenerated and every prose reference to the old names swept, in docs, specs and the Rust help text alike

### CLI-006: Command reference with syntax and examples
  - [x] **CLI-006.1**: 22 worked examples live in clap's `after_help`, so they appear in `cortex <cmd> --help` as well as the reference, from one source. **A test walks the command tree and checks every example names a real command and real flags** - examples are the part of the help a reader copies verbatim, so a stale one fails in their terminal rather than ours, and unlike syntax they cannot be generated
  - [x] **CLI-006.2**: The generator recurses one level, so the reference is grouped by noun with a section per verb. It has to: the arguments live on the verb, and stopping at `cortex preset` would document none of them

### CLI-004: Distribution
- [x] **CLI-004.1**: `s/version++` - bump the version, commit, and land it on `main`, choosing a direct push or a release PR by detecting branch protection. Adapted from the house-style example. Runs the full `s/lint` gate before the version moves, so a failed check leaves no half-bumped tree, and refuses on a dirty tree or off `main`. Does NOT tag: the workflow does that once the commit lands (CLI-004.2)
- [x] **CLI-004.5**: `cortex completions install` - detects the shell, writes to `~/.zfunc` (zsh) or the conventional directory, prints the one-time setup, never edits startup files
- [x] **CLI-004.6**: `s/install` - installs from `crates/cortex-cli` (the workspace root has no `[package]`, so `cargo install --path .` fails), with a udev-rule preflight

## Engineering (ENG)

### ENG-001: DX and testing
- [x] **ENG-001.x**: Correlation unit tests - 12 tests over `dispatch` covering type-first matching, the id-less oldest-first fallback, cascade rejection, the stale-seed-push skip, collector semantics, and the liveness stamp. No fake transport needed; `dispatch` is a free function. The HashMap-ordering guard was verified to fail 12/12 with the bug reintroduced and 0/12 with it fixed
- [x] `s/test` - cargo fmt + clippy + test
- [x] `s/lint` - cargo fmt + clippy + reuse lint
- [x] `.editorconfig`

### ENG-002: CI
- [x] `.github/workflows/ci.yml` - fmt, clippy (all + no-default-features), tests (both), REUSE lint, protoc install
- [x] `.github/dependabot.yml` - cargo + github-actions, weekly, cooldown, grouping

### ENG-003: Governance
- [x] AGPL-3.0-or-later LICENSE + LICENSES/ (AGPL, MIT for vendored .proto)
- [x] SPDX headers on every source file
- [x] REUSE.toml + `reuse lint` passing
- [x] NOTICE + THIRD-PARTY-NOTICES.md (pyquadcortex MIT, deskop-nano-cortex Apache-2.0, qc-stomp-tools MIT)
- [x] Trademark and unaffiliation notice in README, AGENTS.md, NOTICE
- [x] AGENTS.md (repo-local, pointing at parent workspace)
- [x] **ENG-003.1**: **Copyright holder confirmed as `Dr Marcus Baw`**, with no company, which is what every SPDX header already says. Revisit only if a company is formed; if it changes, every header and `REUSE.toml` move in ONE commit

### ENG-005: `s/usb-trace` - observe Cortex Control on the wire
- [x] **ENG-005.1**: `s/usb-trace` - preflight `usbmon` (module loaded, `/dev/usbmonN` present and readable) and `dumpcap`, identify the QC's bus and device address from `lsusb`, and start a capture to a gitignored `traces/`. Each preflight failure names its own fix rather than just refusing, because a setup error discovered halfway through a session with the official client wastes the whole session. Writes a sidecar `.txt` recording bus and device address: both are assigned at plug time, so a capture without them cannot be filtered afterwards with any confidence.
  - Uses the **binary** usbmon interface via `dumpcap`, not the text interface at `/sys/kernel/debug/usb/usbmon/<bus>u`. The text interface truncates payload data, which would drop bytes from the middle of a 128-byte body while still looking like a successful capture.
- [x] **ENG-005.2**: `s/usb-decode` plus `cortex decode-trace` - reads a capture and prints it in the same shape as `CORTEX_TRACE`. Verified against the first real capture: 666 messages, **0 reports skipped**, independently reproducing two known facts (the `GlobalTempo` pair structure - 35 ms within a pair, 786 ms between - and 399 inbound `File` messages for the 399-folder enumeration).
  - **Reuses the crate's framing rather than reimplementing it.** `Message::decode` was extracted so the live RX path and the offline decoder share one implementation; a decoder that parsed the wire its own way would be worse than useless, because it would be trusted while wrong. The order is the trap: the 8-byte trailer is stripped BEFORE gunzip, since the type tag sits outside the compression.
  - Both directions in one pass. Inbound reports carry their bytes in `usb.capdata`, but our writes are `SET_REPORT`s on the control endpoint and carry theirs in `usb.data_fragment`; asking for both fields and taking whichever is populated is what makes a single pass cover both.
  - `s/usb-decode --live` is the development monitor - every message to and from the unit as it happens, whoever sent it. Wireshark's GUI can watch the same bus but knows nothing of our framing, so it shows 129-byte blobs rather than messages.
  - Unknown type tags print as `<unknown N>` rather than collapsing onto `Undefined`. Reading a capture of a client we do not control is exactly when an unrecognised tag is the interesting part.
- [x] **ENG-005.3**: `cortex decode-trace --verbose` describes each message's protobuf fields - field number, wire type and value, with length-delimited values shown as text where they are text.
  - **Generic rather than per-type, deliberately.** A match over the 70-odd message types would decode more prettily and would go blank on exactly the messages worth looking at: the ones the official client sends that we do not model. The wire format carries field numbers and types regardless, which is enough to compare two clients' requests byte for byte.
  - It earned that on its first use, identifying why `CPULoad` never pushed to us (008.6.12) from a one-line difference invisible at the message level.

### PROT-008.6 sub-items (parent still open)
  - [x] **008.6.1**: `cortex session start` holds a `ConnectMode::Subscribed` session and owns the HID interface. Subscribing is expensive per command and correct per session: it is how the device reports edits made by the player
  - [x] **008.6.2**: A unix socket at `$XDG_RUNTIME_DIR`, line-delimited JSON, reusing the existing `--format json` output types so client and daemon share one contract
  - [x] **008.6.11**: **`Version` READ before announcing our own**, mirroring Cortex Control. Costs ~0.8 s (handshake 2.2 s -> 3.0 s, three consecutive runs) and buys two things: `connect --status` can report the unit's serial and `CorOS` version, which it previously left `None`, and `cortex device version` through the daemon is served from that cache in **0.002 s** rather than 2.86 s. It also removes a documented race - `Version` READ replies carry no `request_id`, so a later caller's reply is indistinguishable from the handshake's own announce.
  - [x] **008.6.12**: **`CpuLoad` for a live DSP-load display.** Added to the subscribe set and exposed as `Session::cpu_load()` and `cortex device cpu`, with a typed view carrying total load plus a per-column breakdown flagged by DSP core (`is_on_core2` - the QC splits the grid across two cores).
    - **Asked for with CREATE, not READ.** Every other subscribe is a plain READ and gets answered; a READ for `CPULoad` is silently ignored, and that cost an afternoon of looking for the difference elsewhere (keepalive rate, missing `CloudProduct`, a UI toggle). Cortex Control sends action `CREATE` with a `request_id`, which on the wire is a single field 2 and no action field at all - proto3 omits defaults, and `CREATE` is 0. Reading it as "create a subscription" rather than "read a value" also makes more sense of a message whose reply is a continuous stream.
    - Found by `cortex decode-trace --verbose` (ENG-005.3) on its first use, by putting our request and CC's side by side: `field 1: varint 3` against `field 2: varint 2`. Nothing about the message sizes or types differed, so no amount of staring at the message log would have shown it.
    - The first push lands about 8 s after the request, not immediately - long enough that an early check reads as failure. Verified working: total 54.8 % with a per-column breakdown across four rows, second-core columns flagged.
  - [x] **008.6.8**: Fall back to a direct `Minimal` session when no daemon is running, so single commands still work standalone
  - [x] **008.6.9**: **Refuse to open the device while a daemon holds it.** Hardware-verified the hard way: a one-shot command that opened the device alongside the held session left every later read on that held session timing out. Nothing errored at the point of the collision - the damage only showed on the next request, which is what makes refusing loudly worth the code. `ensure_device_free()` is the CLI-side expression of the exclusive-access invariant. The socket is bound *before* the handshake, because the handshake window (33 s on an unsettled device) is when the daemon owns the interface but a late `bind()` would leave `is_running()` answering false.
    - The device also went quiet at that moment. That was discounted for a while, because our own sessions were falling silent for an unrelated reason (too slow a keepalive, since fixed - 008.6.4). The read failures carry the finding on their own either way
  - [x] **008.6.10**: **Handshake time varied hugely between identical runs. Resolved: it was our RX loop starving the writer, not the device.** Five consecutive runs after the fix: **2.2 s every time**, against a prior range of 2.2-102.7 s. The step breakdown now shows every handshake message leaving at t=0, with the only remaining cost the 2 s settle window we choose ourselves.
    - The wire evidence: on one 102.7 s handshake the bus was silent for 46.8 s between our first and second messages, while the device answered every write in 217 us. Handshake steps 2-4 are fire-and-forget `send()` calls, so the delay surfaced in the progress labels as though the device were slow to reply - when in fact nothing had been sent yet. That misreading is why this looked like device behaviour for so long.
    - Everything below is the reasoning from before the cause was known. Kept because the measurements were sound and only the explanations were wrong.
    - **This is almost certainly the same phenomenon as PROT-008.5**, which records the same 5-50 s spread on a listing read because delivery is lazy. The observed handshake range (5.6-52.1 s) matches closely, and the handshake performs exactly those reads. Treat them as one problem.
    - It does **not** follow that they have one fix. Retrying was the obvious candidate and it was measured to be about 3.5x *worse* (see 008.5). Whatever the mechanism is, asking the device again is not the lever.
    - Two hypotheses were entertained and neither survives the data. **Collision residue:** an earlier run of 7.4 s, then 32.9 s, then 52.1 s across sessions separated by the 008.6.9 collisions looked like monotonic degradation; against a 4x idle spread it is three samples of a noisy distribution. `pyquadcortex` had already ruled out plain reconnection (12 abandoned sessions, no degradation). **DSP load** (the unit was being played during those runs) remains plausible and untested, but distinguishing it needs many samples per arm given the variance, so it is not worth chasing before 008.5 lands.
    - Why it matters beyond curiosity: every timeout in the crate (10 s, 15 s, 30 s) was chosen against a handful of runs on an idle unit. The 52.1 s handshake still succeeded only because those budgets are per-step rather than total. Budgets set against a distribution this wide need justifying from its tail, not its median
- [x] **CLI-004.2**: `auto-tag.yml` - a push to `main` that bumps the workspace version creates the matching `v<x.y.z>` tag. Completes the release contract: `s/version++` lands the bump and deliberately does not tag, so release authority lives in CI rather than on a laptop, and the same command works before and after `main` is protected. The tag is created through the API rather than `git push`, because a push made with the default `GITHUB_TOKEN` does not trigger downstream workflows - the same reason `release.yml` and `publish-crates.yml` will be invoked from here by `workflow_call` rather than by the tag push (CLI-004.3, CLI-004.4). Idempotent: a re-run finds its own tag and stops.

## Engineering (ENG) - later additions

- [x] **ENG-001.y**: **A fake transport, so everything above the wire is testable.** `link::HidLink` is the seam - two operations, `write` and `read_timeout`, which is all `Session` ever needed - with `FakeLink` behind a `test-support` feature. `Session::over` takes any link; `Session::open` is that with a real device attached.
  - Coverage: `session.rs` **42.7% -> 71.5%**, workspace **40.0% -> 46.0%**.
  - Five tests pin behaviour that previously only hardware could check, and that has already shipped bugs: a waiting writer is not starved by the RX loop (the fake is *saturated* so reads always return immediately, which is the only condition under which the race is lost), a reassembled message reaches its waiter, keepalives go out about once a second, silence means nothing until the device has spoken, and a malformed report does not stop the loop.
  - **It found a latent bug on its first run.** `has_heard_from_device` used `last_any_inbound_ms > 0` as its sentinel, but that field holds milliseconds since session start, so 0 is a real timestamp for the first millisecond - "arrived immediately" and "never arrived" were the same value. Hardware hid it because the handshake takes seconds. Now an explicit flag.
- [x] **ENG-001.z**: **Daemon tests over a fake session.** `connect.rs` **0% -> 36.8%**, workspace **46.0% -> 48.9%**. `Daemon::over` takes an already-connected session, separating construction from connection, so the socket protocol, the status shape and the error paths can be exercised without a device - all of which had been verified only by hand, through several rewrites of the shutdown and readiness logic. Six tests: status answers without touching the device, a malformed request is answered rather than dropped and does not stop the daemon serving others, a shutdown request stops the accept loop, a CPU-load request before any push explains itself, one connection serves several requests, and blank lines are ignored rather than answered. Device-bound requests are deliberately absent: `REQUEST_TIMEOUT` is 30 s, so testing one would trade half a minute for nothing the fake can assert.
- [x] **PROT-004.1 / PROT-004.3**: **Typed preset views in the crate** (`view::Preset`, `Row`, `Block`, `ParamValue`, `Bypass`). `BinaryPreset` is the wire shape - wide, repeated-field, and awkward: occupancy cannot be taken from `models.len()`, rows are zero-based while the unit prints 1-4, and a scene-following parameter stores one value per scene. These answer those questions once so the CLI, the MCP server and the Tauri backend cannot answer them differently by accident.
  - They already existed, as the CLI's private output types - which would have left the GUI reimplementing the same decisions against the same protobuf. Moved rather than rewritten, and the CLI now aliases them.
  - `ParamValueKind::Number` widened `f32` -> `f64` on the way: the wire carries a value as either a float or an int, and narrowing the int lost precision for nothing. That made text output print the full decimal expansion of a float, so the printer now formats to 6 places while JSON keeps full precision.

