# Hardware smoke runbook

CI has no Quad Cortex, so anything touching the wire is unverified until a human runs this against a real unit. Run it before a release, and after any change to the transport, session, or grid layers.

Sections 1-10, the non-physical start/status/routed-command/stop parts of section 11, and the move/restore path in section 12 are scripted with read-back assertions: `s/hardware-smoke --scratch-bank N --restore-slot SLOT --discard-working-copy` (ENG-006). It requires one designated scratch bank, an explicit restore slot, and acknowledgement that preparation recalls a target and can discard a starting working-grid edit. It cross-checks direct and held-session identity, prepares and backs up before editing, commits with the returned opaque token, moves that created fixture to the next scratch slot and back, then deletes it. This page remains the source of truth for why each step is safe and for physical controls and disconnect/reconnect.

Record the CorOS version, firmware, and serial from step 1 alongside the result: a pass on `d14e` says nothing about `d15x`.

## Before you start

- Quit Cortex Control. A second effective HID owner can silently wedge the first even though the open succeeds.
- Connect the unit by USB and power it on.
- **Headphones or nothing.** Several steps change what is heard.
- Declare `SCRATCH_BANK`, `SCRATCH_SLOT`, `RESTORE_SLOT`, `ROW`, and `COLUMN`. Every slot in the scratch bank must be disposable; choose an empty grid cell after inspecting `cortex grid show`. The provisional move check additionally requires `MOVE_NAME`, its current `MOVE_FROM` slot, and a confirmed-empty `MOVE_TO` slot in that bank.

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
cortex scene --index 1
cortex device probe                   # confirm active_scene: 1
```

- [ ] The recall changed the loaded preset
- [ ] The scene switch took effect

## 7. Stored preset read

```sh
cortex preset show --slot "$SCRATCH_SLOT"
```

- [ ] Returns the preset with blocks named through the catalog
- [ ] The unit is now on 1A - `read_preset` recalls, by design

Then check the documented trap:

```sh
cortex scene --index 1
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

Prepare a scratch-slot save token before unplugging, but do not edit or commit it. After reconnect, attempt the commit with explicit confirmation.

- [ ] The pre-reconnect token is refused because its physical-session generation is stale

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

```sh
cortex session stop
```

- [ ] The session exits and releases the HID interface

## Out of scope

Not covered by this runbook:

- MCP protocol/discovery behavior beyond the same daemon/core operations.
- GUI integration and interaction.
- The wider unimplemented device API, capture/IR transfer, and most physical control paths.
- Everything on the Nano Cortex.
