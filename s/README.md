# `s/`

Small scripts naming the repeated processes of working on this repository. Each is a thin wrapper round a real tool; the script is the canonical way to invoke that process, so a fresh clone finds the right entry point by listing one directory.

Run them from anywhere in the repo.

## `s/test`

Run the test suite across the workspace.

- `s/test` - everything

## `s/lint`

The full local gate, mirroring CI: `cargo fmt --check`, clippy with `-D warnings` on both feature paths, the tests, and `reuse lint`. A green run here means a green run in CI.

- `s/lint`

## `s/install`

Install the `cortex` CLI from this checkout onto your `PATH`.

Needed because the obvious command does not work here: the workspace root has no `[package]`, so `cargo install --path .` fails. The binary lives in `crates/cortex-cli`.

- `s/install` - install `cortex`
- `s/install --force` - reinstall over an existing copy
- `s/install --debug` - faster build, slower runtime
- `s/install --mcp` - also install `cortex-mcp` (currently a scaffold)

Also preflights the udev rule and prints the fix if it is missing.

## `s/lint-no-device-data`

Fail if anything shaped like real device data - a serial number, MAC address, or firmware checksum - has crept into a tracked file. Run by `s/lint` and by CI.

Documentation examples are captured from a physical unit, so this is easy to do by accident and impossible to undo once published. Works by allow-list rather than deny-list, so the values being kept out never have to be committed.

- `s/lint-no-device-data`

## `s/progress`

Where the project is up to, counted from `spec/roadmap.md` - the single progress record. Counted rather than written, so the summary cannot drift from the thing it summarises.

- `s/progress` - per-section totals
- `s/progress --wip` - what is in progress
- `s/progress --next` - what is planned

## `s/docs`

Serve the documentation site locally with hot reload, on the first free port in 8000-8030, opening a browser.

One-off setup:

```sh
python3 -m venv .venv
./.venv/bin/pip install -r requirements.txt
```

- `s/docs`
- `s/docs --dev-addr localhost:9000` - override the auto-picked port

## `s/docs-cli-reference`

Regenerate `docs/cli-reference.md` from the CLI's own `--help`, so the reference cannot drift from the real command surface. Run after adding or changing a command, and commit the result.

- `s/docs-cli-reference`

## `s/version++`

Bump the version, commit, and land the release commit on `main`. **This pushes.**

The landing *is* the release: the auto-tag workflow creates the tag once the commit reaches `main`. There is deliberately no `s/release` - a second script that also tagged was a source of divergence between repos.

Runs the full `s/lint` gate before touching the version, so a failure leaves no half-bumped tree.

- `s/version++` - patch bump (0.0.1 -> 0.0.2)
- `s/version++ minor` / `s/version++ major`
- `s/version++ --pr` - open a release PR instead of pushing to `main`

Every crate takes `version.workspace = true`, so the single version in the root `Cargo.toml` is the only one to bump. When the Tauri app lands, its `package.json` and `tauri.conf.json` have to move in the same commit.

## `s/usb-trace`

Record the USB traffic to and from the Quad Cortex, using `usbmon` on the Linux host.

This is how to answer "what does the official client actually do". With the unit passed through to the Windows VM running Cortex Control, the host kernel still sees every transfer - QEMU's passthrough goes out through the host's own USB stack - so nothing needs installing inside Windows. It works equally well on our own client, which is the cheaper use: it shows what is on the wire rather than what our RX path believes arrived.

Named `usb-trace` rather than `usb-capture` because "capture" already means a Neural Capture in this domain, and `trace` is the project's existing word for protocol observation (`CORTEX_TRACE`).

- `s/usb-trace` - capture until Ctrl-C
- `s/usb-trace --seconds 30` - capture a fixed window
- `s/usb-trace --output FILE` - choose the file

One-off setup, which the script's preflight will prescribe if missing:

```sh
sudo pacman -S wireshark-cli
sudo modprobe usbmon
```

Writes to `traces/`, which is gitignored. **Captures must not be committed**: they hold Neural DSP's strings verbatim alongside the unit's serial number and the owner's preset names.

## `s/usb-decode`

Read a capture as Cortex Control messages rather than raw USB frames.

Drives `tshark` and pipes it into `cortex decode-trace`, which reassembles using the crate's own framing, trailer and gzip code - so the decoder cannot disagree with what the client actually does.

- `s/usb-decode traces/foo.pcapng` - decode a capture
- `s/usb-decode --live` - decode as it happens
- `s/usb-decode --quiet` - counts only

`--live` is the development monitor. Run it in a second terminal and every message to and from the unit appears as it goes past, whoever sent it - our client, Cortex Control in the VM, or the unit's own front panel. Wireshark's GUI can watch the same traffic but knows nothing of our framing, so it shows 129-byte blobs where this shows messages.

It was a capture of our *own* client that found the writer starvation in `session.rs`, which had been costing up to 100 seconds a handshake and had been misattributed to the device for a full day. Reach for this earlier than feels necessary.

## `s/hardware-smoke`

Run deterministic end-to-end CLI checks against a real Quad Cortex. It reads every write back through a separate command, saves only within one explicitly disposable scratch bank, and restores the named starting slot even after a failed check or Ctrl-C.

- `s/hardware-smoke --scratch-bank 31 --restore-slot 1A --discard-working-copy`
- `s/hardware-smoke --scratch-bank 31 --restore-slot 1A --discard-working-copy --setlist "/media/p4/Presets/My Presets"`
