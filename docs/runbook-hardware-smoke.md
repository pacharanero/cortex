# Hardware smoke runbook

CI has no Quad Cortex, so anything touching the wire is unverified until a human runs this against a real unit. Run it before a release, and after any change to the transport, session, or grid layers.

Record the CorOS version, firmware, and serial from step 1 alongside the result: a pass on `d14e` says nothing about `d15x`.

## Before you start

- Quit Cortex Control. It holds the HID interface exclusively.
- Connect the unit by USB and power it on.
- **Headphones or nothing.** Several steps change what is heard.
- Note what is currently loaded, so you can restore it: `cortex grid`.

!!! info "Why this is safe"

    Saving is not implemented. Every grid edit lives on the working grid until a recall discards it, so nothing in this runbook can modify a stored preset. That changes the day `save_preset` lands, and this runbook will need a scratch-slot discipline then.

## 1. Device is reachable

```sh
cortex version
```

- [ ] Reports device type, serial, and firmware versions
- [ ] Record: CorOS version, `app_firmware`, serial

This needs no handshake, so a failure here is a connection or permissions problem, not a protocol one.

## 2. Session layer

```sh
cortex version --session
```

- [ ] Same output as step 1

Exercises a different path: background RX thread, correlated request, clean thread join. If step 1 passes and this hangs, the session layer is at fault.

## 3. Handshake and read paths

```sh
cortex probe
```

- [ ] Handshake completes, each step printed
- [ ] `active_scene`, `read_current_preset`, `list_presets` all report `ok`
- [ ] Record the handshake duration

A cold device can take ~35 s; a warm one ~2 s. Both are normal. Reads timing out **after** a successful handshake is the failure to look for - it means the settle returned before the device finished its state dump.

## 4. Enumeration

```sh
cortex presets
cortex folders
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
cortex recall --slot 1B
cortex grid                    # confirm the grid changed
cortex scene --index 1
cortex probe                   # confirm active_scene: 1
```

- [ ] The recall changed the loaded preset
- [ ] The scene switch took effect

## 7. Stored preset read

```sh
cortex preset --slot 1A
```

- [ ] Returns the preset with blocks named through the catalog
- [ ] The unit is now on 1A - `read_preset` recalls, by design

Then check the documented trap:

```sh
cortex scene --index 1
cortex preset --slot 1A
cortex probe                   # active_scene should be back to 0
```

- [ ] The scene reset itself, because the recall resets it to the preset default

## 8. Grid editing

Pick an empty cell. `cortex grid` shows which are free.

```sh
cortex set-block --row 2 --column 0 --model 1
cortex grid --params
```

- [ ] Reports `echo confirmed`
- [ ] The block appears in the live grid at that cell
- [ ] The screen row you asked for is the row it landed on

```sh
cortex set-param --row 2 --column 0 --param GAIN --value 0.9
cortex grid --params
```

- [ ] `GAIN` reads back as `0.9`
- [ ] Untouched parameters sit at their catalog defaults

```sh
cortex set-param --row 2 --column 0 --param WOBBLE --value 0.5
```

- [ ] Refused, listing the model's real parameter names

```sh
cortex remove-block --row 2 --column 0
cortex grid
```

- [ ] The cell is empty again

## 9. Restore

```sh
cortex recall --slot <the slot you started on>
cortex grid
```

- [ ] Back to the starting state

## 10. Output contract

```sh
cortex presets --format json | jq -r '.[] | .slot'
```

- [ ] Valid JSON on stdout despite progress lines on stderr

## Known gaps

Not covered by this runbook, because nothing exercises them yet:

- Everything on the Nano Cortex.
