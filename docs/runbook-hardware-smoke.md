# Hardware smoke runbook

CI has no Quad Cortex, so anything touching the wire is unverified until a human runs this against a real unit. Run it before a release, and after any change to the transport, session, or grid layers.

Sections 1-10, the non-physical start/status/routed-command/stop parts of section 11, and the move/restore path in section 12 are scripted with read-back assertions: `s/hardware-smoke --scratch-bank N --restore-slot SLOT --discard-working-copy` (ENG-006). It requires one designated scratch bank, an explicit restore slot, and acknowledgement that preparation recalls a target and can discard a starting working-grid edit. It cross-checks direct and held-session identity, prepares and backs up before editing, commits with the returned opaque token, moves that created fixture to the next scratch slot and back, then deletes it. This page remains the source of truth for why each step is safe and for physical controls and disconnect/reconnect.

Record the CorOS version, firmware, and serial from step 1 alongside the result: a pass on `d14e` says nothing about `d15x`.

## Before you start

- Quit Cortex Control. A second effective HID owner can silently wedge the first even though the open succeeds.
- Connect the unit by USB and power it on.
- **Headphones or nothing.** Several steps change what is heard.
- Declare `SCRATCH_BANK`, `SCRATCH_SLOT`, `RESTORE_SLOT`, `ROW`, and `COLUMN`. Every slot in the scratch bank must be disposable; choose an empty grid cell after inspecting `cortex grid show`. The move check additionally requires `MOVE_NAME`, its current `MOVE_FROM` slot, and a confirmed-empty `MOVE_TO` slot in that bank.

!!! info "Why this is safe"

    Save, delete, and same-setlist move exist. Every write below is confined to the explicitly declared scratch bank, and the restore slot is recalled on exit. No step writes to the factory library. Recall and preparation can discard an unsaved working copy, which is why consent and the restore target are prerequisites.

## 1. Device is reachable

```sh
cortex device version
```

- [ ] Reports device type, serial, and firmware versions
- [ ] Record: CorOS version, `app_firmware`, serial

This needs no handshake, so a failure here is a connection or permissions problem, not a protocol one.

## 2. Session layer

```sh
cortex device version --session
```

- [ ] Same output as step 1

Exercises a different path: background RX thread, correlated request, clean thread join. If step 1 passes and this hangs, the session layer is at fault.

## 3. Handshake and read paths

```sh
cortex device probe
```

- [ ] Handshake completes, each step printed
- [ ] `active_scene`, `read_current_preset`, `list_presets` all report `ok`
- [ ] Record the handshake duration

A healthy handshake is consistently a few seconds. Record a large or variable delay as a failure and trace it: the previously accepted 2-35 second spread was caused by our RX loop starving the writer and by firing the request burst without pacing, not by a cold device building the catalog. Reads timing out after a successful handshake are also a failure.

## 4. Enumeration

```sh
cortex preset list
cortex setlist list
```

- [ ] Presets match what the unit shows
- [ ] `folders` returns a few hundred entries, with the factory library flagged
- [ ] The occupied count for your user setlist agrees between the two

## 5. Catalog

```sh
cortex catalog
cortex catalog --search marshall
cortex catalog --model 1001
```

- [ ] Model count and category breakdown look sane
- [ ] Search matches on attribution, not only name
- [ ] Parameters listed in wire index order with ranges

## 6. Navigation

Changes what is heard. Note the starting slot first.

```sh
cortex preset recall --slot "$SCRATCH_SLOT"
cortex grid show                    # confirm the grid changed
cortex scene switch --index 1
cortex device probe                   # confirm active_scene: 1
```

- [ ] The recall changed the loaded preset
- [ ] The scene switch took effect

Scene metadata and copy/swap are unsaved working-copy edits. Record `cortex grid show --params --format json`, exercise discriminating scenes, then recall the scratch slot to restore it:

```sh
cortex scene label --index 1 --label "Fictional Lead"
cortex scene color --index 1 --color '#112233'
cortex scene copy --from 1 --to 4
cortex scene swap --first 2 --second 3
cortex grid show --params
cortex scene unlabel --index 1
cortex preset recall --slot "$SCRATCH_SLOT"
```

- [ ] Label and colour read back on scene B
- [ ] Copy makes scene E's parameters, bypass, label and colour equal scene B
- [ ] Swap exchanges discriminating C/D values rather than copying one over both
- [ ] Recalling the scratch slot restores all unsaved scene changes

## 7. Stored preset read

```sh
cortex preset show --slot "$SCRATCH_SLOT"
```

- [ ] Returns the preset with blocks named through the catalog
- [ ] The unit is now on 1A - `read_preset` recalls, by design

Then check the documented trap:

```sh
cortex scene switch --index 1
cortex preset show --slot "$SCRATCH_SLOT"
cortex device probe                   # active_scene should be back to 0
```

- [ ] The scene reset itself, because the recall resets it to the preset default

## 8. Grid editing

Pick an empty cell. `cortex grid show` shows which are free.

Before editing, prepare the destination while the working grid is clean. The preparation recalls and backs up the target, and therefore requires the held session to retain its opaque token:

```sh
cortex session start
SAVE_TOKEN=$(cortex preset prepare-save --slot "$SCRATCH_SLOT" --format json | jq -r '.token')
```

```sh
cortex block set --row "$ROW" --column "$COLUMN" --model 1
cortex grid show --params
```

- [ ] Reports `echo confirmed`
- [ ] The block appears in the live grid at that cell
- [ ] The screen row you asked for is the row it landed on

```sh
cortex block param --row "$ROW" --column "$COLUMN" --param GAIN --value 0.9
cortex grid show --params
```

- [ ] `GAIN` reads back as `0.9`
- [ ] Untouched parameters sit at their catalog defaults

Choose a second empty cell for `TO_ROW`/`TO_COLUMN`, move the block there and back, and inspect both cells after each operation:

```sh
cortex block move --from-row "$ROW" --from-column "$COLUMN" --to-row "$TO_ROW" --to-column "$TO_COLUMN"
cortex grid show --params
cortex block move --from-row "$TO_ROW" --from-column "$TO_COLUMN" --to-row "$ROW" --to-column "$COLUMN"
cortex grid show --params
```

- [ ] Each command reports read-back confirmation
- [ ] The source becomes empty and the block's model, parameters and bypass state appear unchanged at the destination
- [ ] Moving back restores both cells; a cross-row destination lets the device adjust branch routing

```sh
cortex block param --row "$ROW" --column "$COLUMN" --param WOBBLE --value 0.5
```

- [ ] Refused, listing the model's real parameter names

To retain the edited grid, commit the preparation made before editing:

```sh
cortex preset save --token "$SAVE_TOKEN" --name "Smoke Test"
```

- [ ] The saved preset still contains the edits after recall
- [ ] An unknown or reused token is refused

```sh
cortex block remove --row "$ROW" --column "$COLUMN"
cortex grid show
```

- [ ] The cell is empty again

## 9. Restore

```sh
cortex preset recall --slot "$RESTORE_SLOT"
cortex grid show
```

- [ ] Back to the starting state

## 10. Output contract

```sh
cortex preset list --format json | jq -r '.[] | .slot'
```

- [ ] Valid JSON on stdout despite progress lines on stderr

## 11. Held session, cache, and reconnect

```sh
cortex session start
cortex session status --format json | jq '.device, .cache'
cortex grid show --params
cortex preset list
cortex device cpu
```

- [ ] Status reports `device.state: connected`, `cache.phase: live`, and cached preset, scene, catalog, and setlist data
- [ ] `messages_seen` and `pushes_applied` increase while the session runs
- [ ] Grid, preset listing, and CPU commands answer through the held session without another handshake

Turn one block knob on the unit and then return it to its original value. Run `cortex grid show --params` after each movement.

- [ ] Both hardware-originated values appear without recalling the preset
- [ ] A knob burst advances the cache revision without making the session unresponsive

Record `cache.storage_revision`, then use the unit itself to save an unchanged USER preset in a slot the tester has explicitly designated disposable. Never use the factory library for this check.

- [ ] The save advances `storage_revision`; an ordinary recall or working-grid edit does not
- [ ] The session remains responsive and the target's refreshed listing agrees with the unit

Record the cache generation, unplug the USB cable, wait for `cortex session status` to report `reconnecting`, then reconnect it.

- [ ] Cached state is invalidated immediately while disconnected
- [ ] Status reports reconnect attempts and the latest error rather than claiming `connected`
- [ ] No replacement attempt starts until any in-flight command has returned or failed
- [ ] The full subscribed handshake succeeds after reconnect
- [ ] The cache generation increases and returns to `live`
- [ ] `cortex grid show` agrees with the unit after reconnect rather than showing pre-disconnect state
- [ ] The first request after reconnect succeeds; a successful open alone is not evidence because overlapping HID ownership fails on the next request

Do not inject malformed USB traffic into the unit. The remaining malformed-report recovery check may be completed only if a naturally occurring framing/envelope gap is observed in a trace: confirm the daemon marks the cache invalid, performs a full replacement handshake in a new generation, and succeeds on the next ordinary grid request. Otherwise record it as an unexercised hardware residual; offline fake-link injection already covers the deterministic recovery logic.

Prepare a scratch-slot save token before unplugging, but do not edit or commit it. After reconnect, attempt the commit with explicit confirmation.

- [ ] The pre-reconnect token is refused because its physical-session generation is stale

With no explicit session running, exercise auto-managed idle release and a replacement non-mutating direct read through the ignored process test:

```sh
cargo test -p cortex-cli --test lifecycle-hardware auto_managed_daemon_releases_device_for_replacement_direct_read -- --ignored --exact --nocapture
```

- [ ] The auto-managed daemon reports ready, exits two seconds after its final status request, removes its endpoint, and the replacement direct `device version` read succeeds

**Measured 2026-08-10 on CorOS 4.0.1:** passed. Idle expiry closed the HID handle, removed the socket endpoint, and a replacement direct version read succeeded. This verifies release through the next request, not merely through a successful second open.

## 12. Preset move and restore

Use two slots wholly inside the disposable test bank. Confirm `MOVE_FROM` contains `MOVE_NAME` and `MOVE_TO` is empty in a freshly requested complete listing before continuing. The command refuses an occupied destination, but listings are eventually consistent and cannot eliminate the final race with another storage mutation.

```sh
cortex preset list --include-empty
cortex preset move --from "$MOVE_FROM" --to "$MOVE_TO"
cortex preset list --include-empty
```

- [ ] The command succeeds only after fresh complete listings prove the source is empty and `MOVE_NAME` occupies the destination, whether or not a `File{MOVE}` acknowledgement arrived
- [ ] A fresh listing eventually shows `MOVE_NAME` at `MOVE_TO` and `MOVE_FROM` empty
- [ ] `cache.storage_revision` advances and the previous cached listing is not served as current

Move it back before leaving the scratch bank:

```sh
cortex preset move --from "$MOVE_TO" --to "$MOVE_FROM"
cortex preset list --include-empty
```

- [ ] A fresh listing eventually shows the preset restored at `MOVE_FROM` and `MOVE_TO` empty

## 13. GUI daemon read boundary

Keep the held session running, then exercise the production Rust boundary:

```sh
cargo test -p cortex-gui tests::daemon_dashboard_reads_one_live_generation -- --ignored --exact
s/gui-dev
```

- [ ] The Rust boundary returns one live snapshot whose generation and revision match daemon status
- [ ] The preset directory contains populated slots and does not label an unavailable folder empty
- [ ] Tauri mode shows the same working-grid name, blocks, scene and CPU data as the CLI
- [ ] The header identifies daemon mode; it never displays the fixture banner
- [ ] Unplugging the unit changes the GUI to reconnecting and hides the old grid and CPU
- [ ] After reconnect, the generation advances and the first rendered live snapshot agrees with `cortex grid show`
- [ ] A daemon failure remains visible and never switches to fixture data

### GUI dual-device switching (GUI-001.8)

Connect both products by USB and disconnect Nano Bluetooth. Start a fresh `s/gui-dev`; Auto-detect should select Quad. Do not run persistent save/delete operations or Nano FX writes during this smoke.

- [ ] Select Nano and confirm only the fixed eight-role chain appears
- [ ] Select Quad and confirm only the routed 4x8 grid appears
- [ ] Both transitions after initial startup complete in under one second
- [ ] Both product daemons remain connected concurrently:

```sh
cortex session status --device quad --format json | jq '.device_kind, .device.state'
cortex session status --device nano --format json | jq '.device_kind, .device.state'
```

- [ ] Leave the GUI open on one product for more than 60 seconds, then confirm both status commands still report connected and switching remains under one second
- [ ] Closing the GUI allows both auto-managed daemons to expire; explicitly started daemons remain running

**Measured 2026-08-25 on Linux with both devices connected:** both auto-managed daemons reported connected concurrently. Quad to Nano and Nano back to the warm Quad each rendered in under one second. After the GUI closed, both product endpoints reported `running: false` after the 60-second idle window. The previous single-endpoint teardown design took 5-8 seconds to return to Quad because it repeated the mandatory subscribed handshake. Explicit-daemon survival still requires a separate smoke.

### GUI scene switching (GUI-003.4)

Still in `s/gui-dev` against the held session. Scene switching is non-persistent - it changes what the unit plays and saves nothing - so no preset is at risk, but it *is* audible. Mute or disconnect outputs if that matters.

Note the working scene before you start so you can return to it.

- [ ] The scene control offers all eight scenes as A-H, including ones this preset has never labelled
- [ ] A labelled scene shows its label beside the letter; an unlabelled one reads "unlabelled" rather than blank
- [ ] Selecting a different scene changes the scene shown on the unit's own display
- [ ] The GUI's scene reflects what the unit reports, and matches `cortex device state` for the same generation
- [ ] Changing the scene **on the unit** is reflected in the GUI within about a second, without touching the GUI control

To keep the last check honest, record the scene independently while you press footswitches, so the observation does not rest on the GUI agreeing with itself:

```sh
while :; do cortex device probe --format json | python3 -c 'import json,sys; print(json.load(sys.stdin).get("active_scene"))'; sleep 0.4; done
```

- [ ] The independent reader shows the same scene sequence you pressed
- [ ] Click a scene radio directly, then press Right arrow: the selection moves on and the unit follows (keyboard parity; confirm focus is on the radio before blaming the control)
- [ ] With a screen reader running (Orca on Linux), the control announces the group, the scene count, the position, and the newly active scene after each switch
- [ ] Step through all eight scenes and confirm the physical scene LED colour matches the colour the GUI reports for that scene
- [ ] Return to the scene noted at the start; no preset was saved and the working copy is not left dirty by the switching itself

```sh
cortex session stop
```

- [ ] The session exits and releases the HID interface

## 14. PROT-006.10 temporary setlists

This ignored test creates uniquely named fictional USER setlists. It aborts before mutation if a generated name already exists, recalls one explicitly authorised existing USER preset only as a source, saves/copies presets with discriminating typed instrument tags, verifies fresh listings and recalled duplicate audio state, and duplicates through create plus recall/save rather than `BulkOperation`.

```sh
cargo test -p cortex-rs --test hardware-reads prot_006_10_file_operations_converge_and_cleanup -- --ignored --exact --nocapture
```

Cleanup is unconditional after connection: every destination with a returned creation receipt is deleted in reverse creation order and polled absent; generated names are then freshly enumerated so a create that landed without returning is also deleted. Finally the authorised restore preset is recalled and the client disconnects, whether the operation passed or failed. A cleanup or final-recall failure fails the test; if the main operation also failed, its error is reported after all cleanup attempts have run.

**Measured 2026-08-10 on CorOS 4.0.1:** passed create, typed-instrument save, copy, recalled duplicate audio-state comparison, delete convergence and complete generated-storage cleanup. The initial subscribed cache was healthily `Incomplete`; the side-effect-free live-grid read repaired it before preparation. This is not permission to repair `Invalidated` state in place - invalidation still fails closed and requires a replacement handshake.

## 15. PROT-006.13 I/O settings

Before connecting USB, external speakers/amplifiers should be muted/disconnected and no phantom-sensitive path should be connected. The test changes global input and output settings, including levels, input type, mute, USB routing and output pairing. Do not run it through a live PA, powered monitor path, recording input, ribbon microphone or other path that could be harmed by a level, type, pairing or phantom-state interaction. The schema exposes no phantom-power write, but absence of a host field is not a safety interlock for connected equipment.

Stop Cortex Control and any held `cortex session`, leave the Quad Cortex exclusively available, then run exactly:

```sh
cargo test -p cortex-rs --test hardware-reads prot_006_13_io_settings_mutate_poll_restore -- --ignored --exact --nocapture
```

The test refuses to mutate unless a complete capability-aware writable baseline includes the measured four input and eight output identities, USB, MIDI, both pairing couples and all four pairing members. It retains optional per-port fields and changes only baseline-present controls, using Input 1 and XLR Output 1 to exercise every setter. It polls fresh complete reads, restores immediately and requires two matching restoration reads. Pairings run last; after each pairing is restored, both member ports are rewritten using only their applicable baseline fields. Cleanup independently restores and compares every writable present field and requires two final complete baseline reads before disconnecting.

**Measured 2026-08-10 on CorOS 4.0.1:** passed the complete capability matrix, every applicable field mutation, both pairings and independent final-baseline restoration with outputs disconnected. Two traps mattered: discrete selectors must be changed to a valid encoded option rather than an arbitrary float, and accepted writes may need eventual-consistency polling before read-back converges.

## 16. Expanded PROT-006 verification

Stop Cortex Control and any held daemon before each test. Keep audio and MIDI outputs disconnected for the tempo and persistent MIDI tests. These tests choose device-returned entries and generated temporary storage without printing private library identities:

```sh
cargo test -p cortex-rs --test hardware-reads wider_state_reads_answer_without_exposing_device_data -- --ignored --exact --nocapture
cargo test -p cortex-rs --test hardware-reads row_level_grid_mutations_read_back_and_recall_cleanup -- --ignored --exact --nocapture
cargo test -p cortex-rs --test hardware-reads tempo_mutations_read_back_and_recall_cleanup -- --ignored --exact --nocapture
cargo test -p cortex-rs --test hardware-reads stomp_and_expression_mutations_read_back_and_recall_cleanup -- --ignored --exact --nocapture
cargo test -p cortex-rs --test hardware-reads prot_006_9_midi_persists_in_saved_preset_and_cleanup -- --ignored --exact --nocapture
cargo test -p cortex-rs --test hardware-reads capture_selection_reads_back_and_recall_cleanup -- --ignored --exact --nocapture
cargo test -p cortex-rs --test hardware-reads capture_dialog_decline_is_graceful_and_session_stays_healthy -- --ignored --exact --nocapture
CORTEX_VISUAL_PAUSE_SECONDS=15 cargo test -p cortex-rs --test hardware-reads user_ir_selection_reads_back_and_recall_cleanup -- --ignored --exact --nocapture
cargo test -p cortex-rs --test hardware-reads prot_006_12_global_settings_mutate_poll_restore -- --ignored --exact --nocapture
cargo test -p cortex-rs --test hardware-reads pin_and_favorite_mutations_restore_exact_state -- --ignored --exact --nocapture
```

**Measured 2026-08-10 on CorOS 4.0.1:** every listed test except the operator-assisted capture-dialog decline test passed; mutable tests restored their baselines. The wider reads included stable repeated request-correlated capture results, one correlated response from the loadable IR library and one from a user-IR folder, Recents, an empty Favorites baseline, request-correlated PinnedModels, Master Volume, Looper, Tuner settings, I/O and general settings, Global EQ, active mode and mode cycle. The IR responses have no completion marker and do not prove library-wide completeness. Row controls passed only when splitter verification used the writable `combined_splitter` state. Tempo muted first and all eight exposed methods read back before complete recall restoration. STOMP/expression and every persistent MIDI family passed typed-helper and raw 10x12 verification with generated-storage cleanup. Capture selection preserved its exact reference and a post-selection parameter; user-IR selection preserved its exact key/name, and timed visual inspection showed no warning. Global settings, EQ, mode and Tuner settings passed complete automated restoration. Pin duplicate/unpin-all and Favorite add/remove returned to exact initial state.

**Capture-dialog safety finding, 2026-08-12 on CorOS 4.0.1:** after the on-unit New Neural Capture action produced `try_to_show_dialog: true`, the repository's false-only `show_dialog: false` response was sent. The unit froze, stopped responding over HID, and rebooted. Causation is not established, but the response was removed from the public client and must not be retried. Positive acceptance remains blocked until a complete v1/v2 host UI exists. **Visual controls, 2026-08-26:** Gig View visibly opened and closed from `ShowGigView{UPDATE, show}`. `ShowTuner{UPDATE, show:true}` produced no visible Tuner in two ordinary runs or two runs preceded by `ShowTuner{READ}`, and physically closing/reopening Tuner emitted no unsolicited `ShowTuner` push to the normal subscribed session. The client now refuses Tuner visibility before I/O pending a working official-client capture.

## Nano transport smoke

Run this bounded read-only smoke when the Nano is connected. Disconnect any Neural DSP phone/tablet Bluetooth session first; an active remote editor should be tested separately and must surface the device's explicit `Device is busy!` ownership response.

1. Install `70-neural-dsp-cortex.rules`, reload udev, reconnect the Nano, and confirm its interface-5 hidraw node has a user ACL.
2. Confirm `DeviceKind::NanoCortex` opens PID `152A:88E7`, reports 65-byte geometry, sends one known read-only current-state request, and receives one `FIRST`, seven middle and one `LAST` report totaling 546 declared bytes.
3. Decode only bytes within each report's `len`; never print the body or padding because both can contain private strings.
4. Disconnect Nano, connect Quad, and run `cortex device version` plus `cortex session start`, `cortex session status`, and `cortex session stop` to prove the shared geometry refactor preserved Quad behavior.

NANO-001.2 is hardware-verified. The bounded smoke test `nano_transport_geometry_and_state_read` exercises repository code: it opens the Nano, verifies 65-byte geometry, sends the read-only current-state request, and asserts a correct FIRST/middle/LAST multi-report reassembly. The `nano_rejected_by_quad_paths` test verifies `Transport::request` and `Session::open` reject the Nano before USB I/O.

**Measured 2026-08-12 on a Nano Cortex:** 10 reports (FIRST + 8 middle + LAST), 574 reassembled bytes. The original 2026-08-11 probe measured 9 reports / 546 bytes; the state body varies with device content. Quad regression passed immediately after on the same machine: direct version, correlated session version, and full held session (start/status/grid-show/stop) all healthy on CorOS 4.0.1.

## Out of scope

Not covered by this runbook:

- MCP protocol/discovery behavior beyond the same daemon/core operations.
- GUI write interactions and visual-polish coverage beyond the daemon read boundary above.
- Windows package, named-pipe and QEMU/KVM host verification; use the dedicated [Windows tester-preview smoke](gui/windows-smoke.md).
- Capture/IR payload transfer, the positive host-owned capture workflow, a working host-controlled Tuner visibility shape, and most physical control paths.
- Nano application operations beyond the bounded transport/state smoke above, including typed Bluetooth ownership-conflict decoding under NANO-001.3.
