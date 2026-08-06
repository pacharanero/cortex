# Walkthrough

Every output on this page is real, captured from a Quad Cortex running CorOS 4.0.1 (firmware `d14e`). The *shapes* are exactly what your unit will produce.

!!! note "Preset names and identifiers are fictional"

    The preset names, serial number, MAC address, and firmware checksum shown here are invented stand-ins. A real serial identifies one person's hardware, and real preset names say what they play - neither belongs in public documentation. Everything structural is genuine.

    Model names such as `Brit 2203` and `Rodent Drive` are **not** invented: those come from Neural DSP's catalog and are identical on every unit.

## Talk to the device

```sh
cortex device version
```

```text
device_type                QC
custom_name                Neural DSP Quad Cortex
serial_number              QA00AB123
coros_version              4.0.1
app_firmware               d14e
bootloader_firmware        d119
zencoder_app               d111
zencoder_bootloader        d10d
wireless_fw_checksum       0123456789abcdef0123456789abcdef
linux_kernel               Linux buildroot 4.0.0-ADI-1.3.0 #1 PREEMPT ... armv7l (none)
uboot                      U-Boot 2015.01 ADI-1.3.0 (Sep 30 2021 - 01:01:44)
mac_address                02:00:5e:10:00:01
```

`version` is the only command that does not need the connect handshake, which makes it the right first thing to try.

!!! info "Two field names are the vendor's, and both are wrong"

    `coros_version` is carried in a schema field Neural DSP named `zenos_git_hash`, and it holds `4.0.1` rather than a hash. `wireless_fw_checksum` is carried in one they named `zenwireless_fw_version`, and it holds a 32-character checksum rather than a version. We verified by decoding raw field numbers off the wire; the fields are not swapped, each simply holds something its name does not describe. We rename them in output and record the originals. See [the protocol](protocol.md#version-field-names).

## Check everything works

```sh
cortex device probe
```

```text
connect handshake:
  resetting comms buffers ...
  announcing client version ...
  requesting model catalog ...
  announcing connection ...
  subscribing to device state ...
  settling ...
  done (2.2s)
active_scene ... ok
read_current_preset ... ok
list_presets ... ok (11 occupied)
active_scene: 0
current_preset: Plexi Sunrise
preset_count: 11
    1A  Plexi Sunrise
    1B  Tweed Porchlight
    ...
```

`probe` runs the handshake then exercises every read path, so it is the one command that tells you the whole stack works.

!!! tip "If the handshake seems to hang"

    A healthy handshake is consistently a few seconds. A large or variable delay is a client-side fault until a wire trace proves otherwise; the two known causes were the RX loop starving its own writer and an unpaced request burst queueing work against the catalog transfer.

    `cortex` waits for the catalog before subscribing, then waits for the non-heartbeat push burst to settle. Progress is printed at each step. If it pauses for tens of seconds, trace our client with `s/usb-trace` rather than treating that as normal device behaviour.

## Browse presets

```sh
cortex preset list
```

```text
  1A  Plexi Sunrise
  1B  Tweed Porchlight
  1C  Cathedral Clean
  1D  Northern Chime
  1E  Velvet Hammer
  1F  Slapback Diner
  1G  Winter Rotary
  1H  Desert Two-Lane
  2A  Glasshouse Lead
  2C  Bramble Fuzz
  2E  Low Tide
```

Slots are named as the unit labels them: bank 1-32 then letter A-H. Empty slots are hidden; `--include-empty` shows the full 256-slot map, which is how to find a free one.

The factory library is just another setlist:

```sh
cortex preset list --setlist "/opt/neuraldsp/Factory Library"
```

And `cortex setlist list` lists every folder the device knows - 399 of them on the unit tested, including plugin artist packs and the captures library.

## Explore the model catalog

This is the fun one. The catalog comes **from the device**, so it covers your unit's purchased plugins and your own Neural Captures.

```sh
cortex catalog
```

```text
models: 533
with_attribution: 318
categories: 31
   13  Bass Amplifier
  111  Guitar Amplifier
   36  Guitar Overdrive
   26  Reverb
   ...
```

Search by name **or by the gear it evokes**:

```sh
cortex catalog --search marshall
```

```text
     9  Brit Blues            Guitar Overdrive  [Based on Marshall® BluesBreaker®]
    11  Brit Governor         Guitar Overdrive  [Based on Marshall® Guv'nor®]
  1001  Brit 2203             Guitar Amplifier  [Based on Marshall® JCM800®]
  1038  Brit TM45 Normal      Guitar Amplifier  [Based on Marshall® JTM 45® - Normal Channel]
  1041  Brit 900 Clean        Guitar Amplifier  [Based on Marshall® JCM900® 4100 - Clean]
```

!!! info "That attribution is Neural DSP's own"

    The `Based on ...` text is not ours. The device's catalog carries a `tm` field holding Neural DSP's own carefully-worded attribution, on 318 of 533 models. We reproduce it verbatim and never paraphrase it - it concerns other companies' trademarks and their wording is both more accurate and more appropriate than anything we would write.

Look at one model in full:

```sh
cortex catalog --model 1001
```

```text
id: 1001
name: Brit 2203
category: Guitar Amplifier
attribution: Based on Marshall® JCM800®
parameters: 7
   0  GAIN                 float    0..10
   1  BASS                 float    0..10
   2  MID                  float    0..10
   3  TREBLE               float    0..10
   4  MASTER               float    0..10
   5  PRESENCE             float    0..10
   6  OUTPUT               float    -60..12 dB
```

Those indices are what the wire uses. You rarely need them - `set-param` takes names - but this is where to look when you do.

!!! tip "Work offline"

    ```sh
    cortex catalog --dump ~/qc-catalog.bin        # once, with the device
    cortex catalog --from-file ~/qc-catalog.bin --search vox
    ```

    Saves the 47 KB payload so you can browse without tying up the unit or paying a handshake.

## Look at a preset

```sh
cortex preset show --slot 1A
```

```text
slot: 1A
name: Plexi Sunrise
chains: 4
row 0 (screen row 1):
  col 0: Opto Comp (M) (Compressor)
  col 1: Rodent Drive (Guitar Overdrive)  [Based on ProCo® Rat®]
  col 2: Studio Capture (Neural Capture)
  col 3: Graphic-9 (Equalizer)
  col 4: Adaptive Gate (Utility)
row 1 (screen row 2):
row 2 (screen row 3):
  col 1: Slapback Delay (M) (Delay)
  col 2: Tape Delay (ST) (Delay)
  col 3: Tremolo (Modulation)
  col 6: Spring Reverb Engine (ST) (Reverb)
row 3 (screen row 4):
  col 1: Legendary 87 (M) (Compressor)  [Based on Universal Audio® 1176®]
  col 2: Parametric-8 (Equalizer)
  col 5: Room (Reverb)
```

!!! warning "`preset show --slot` recalls the slot"

    There is no side-effect-free way to read a **stored** preset: the device only emits one when it recalls it. So this changes what is loaded and what you hear, and discards any unsaved edit.

    To inspect what is loaded **right now** without disturbing it, use `cortex grid show`.

## Edit the grid

If you intend to save the result, start a held session and prepare the destination **before** editing. Preparation recalls and backs up the target, so doing it afterwards would discard the grid you meant to save:

```sh
cortex session start
cortex preset prepare-save --slot 31A --scratch-range 31A-31H
# Keep the reported token, for example save-1.
```

```sh
cortex grid show --params        # what is loaded right now, no side effects
```

Rows are given as the unit labels them, **1-4**.

```sh
cortex block set --row 2 --column 0 --model 1
cortex block param --row 2 --column 0 --param GAIN --value 0.9
cortex grid show --params
```

```text
row 1 (screen row 2):
  col 0: Myth Drive (Guitar Overdrive)  [Based on Klon® Centaur®]
        0  GAIN               0.9
        1  TREBLE             0.5
        2  LEVEL              0.5
```

Parameters are addressed by **name**, resolved through the catalog by reading which model is actually in that cell. Get the name wrong and it tells you the real ones:

```text
cortex: Myth Drive has no parameter "WOBBLE". It has: GAIN, TREBLE, LEVEL
```

You can also give a value in the parameter's own units:

```sh
cortex block param --row 1 --column 1 --param THRESHOLD --real -20
```

!!! warning "Edits remain a working copy until committed"

    Every grid edit lives on the working grid until you save or recall another preset. A recall discards the edits. To keep them, explicitly commit the destination token you reviewed before editing:

    ```sh
    cortex preset save --token save-1 --name "My preset" --yes
    ```

`block set` **verifies**. A block that does not fit the preset's DSP budget is accepted on the wire and simply is not there afterwards, with no error of any kind. So `cortex` uses the device's echo as a fast path and reads the grid back when no echo arrives; it reports `BlockRefused` only when the grid confirms that the block is absent.

## Machine-readable output

Every command takes `--format json`:

```sh
cortex preset list --format json | jq -r '.[] | "\(.slot)  \(.name)"'
cortex catalog --model 1001 --format json | jq '.parameters[] | select(.read_only | not)'
```

Progress and warnings always go to stderr, so stdout stays clean for piping even though the handshake prints six progress lines.

## When something is wrong

```sh
CORTEX_TRACE=1 cortex device probe
```

Traces every inbound message type, size, and correlation id to stderr, plus each handshake step. This is what to attach to a bug report.
