# cortex

An unofficial, open-source toolkit for the Neural DSP **Quad Cortex** and **Nano Cortex**, built around an eventual desktop GUI for Linux, Windows and macOS, a `cortex` CLI, a `cortex-mcp` server for agentic patch editing, and the reusable `cortex-rs` Rust crate beneath them. The goal is one shared transport and host foundation with honest device-specific models: the Quad's grid, scenes and presets remain distinct from the Nano's fixed signal chain.

That dual-device, cross-platform editor is the destination, not the current support claim. Today the Quad Cortex CLI and MCP paths are hardware-verified on Linux and the GUI supports non-persistent Quad editing. Nano Cortex USB framing, typed current-state decoding, Bluetooth ownership errors, a paced held daemon, state reads, raw amp and bypass editing, and core/daemon/MCP/native-GUI raw FX parameter editing are hardware-verified. Other Nano operations and untested host integrations remain provisional or unimplemented.

> **Unofficial.** This project is not affiliated with, endorsed by, or
> sponsored by Neural DSP Technologies. "Neural DSP", "Quad Cortex", "Nano
> Cortex", and "Cortex Control" are trademarks of Neural DSP Technologies.
> All trademarks are the property of their respective owners. The project
> reverse-engineers the Cortex Control USB HID protocol for interoperability,
> which is the established case under UK CDPA s50B / s296A and EU Software
> Directive Article 6.

## Status

**Pre-alpha and actively changing.** The Quad Cortex core and CLI are usable on Linux and passed a 42-check hardware smoke against CorOS 4.0.1, including live state, grid editing, prepared save, same-setlist preset move/restore, recall and delete. The Nano Cortex state, amp, bypass and raw FX parameter paths run through the same crate and host foundation while retaining their own fixed-chain model. This is not a finished editor: Nano save/model replacement and wider operations, much of the wider Quad device API and cross-platform host paths remain unfinished.

The MCP server exposes hardware-verified Quad read/edit tools and Nano state, amp, bypass and raw FX parameter tools through the held daemon; it deliberately exposes no persistent save or delete tool. An official MCP client hardware smoke discovered and exercised every Nano tool with same-value writes and fresh read-back. The Nano FX path is also verified through the Tauri backend and rendered native Linux control, including a reversible slider edit with device-confirmed restoration. Released Linux x86_64 archives install both binaries together, while the [agent setup guide](https://pacharanero.github.io/cortex/agent-setup/) covers Claude Code and generic stdio harnesses. The desktop build supports Linux, Windows and macOS host boundaries, but Linux remains the only hardware-verified host; the unsigned Windows and ad-hoc-signed macOS packages are explicitly provisional tester previews. Run `s/progress` for counted progress and read `spec/roadmap.md` for the outstanding backlog.

Protocol implementers can start with the separate [Quad Cortex HID transport](https://pacharanero.github.io/cortex/quad-cortex-hid/) and [Nano Cortex HID transport](https://pacharanero.github.io/cortex/nano-cortex-hid/) references, then continue into the shared [protocol documentation](https://pacharanero.github.io/cortex/protocol/).

## What it is

- `gui/` - the Tauri 2 + React + Mantine desktop editor for both Cortex devices on Linux, Windows and macOS. It starts or reuses product-scoped held sessions, provides non-persistent Quad editing, and provides a Nano fixed-chain view with raw amp, bypass and FX parameter controls in explicit fixture and daemon-backed modes. The native Nano FX control is hardware-verified on Linux. Native `.deb`, `.dmg` and NSIS `.exe` package builds include the sibling `cortex` session helper; Windows and macOS remain tester previews pending hardware verification.
- `cortex-cli` - a thin CLI over the crate, including product-scoped persistent daemons that each own one physical device connection. The CLI explicitly holds Quad or Nano. Quad sessions reconnect without serving stale state; after a Nano timeout or malformed response, the session discards and reopens the transport, invalidates the old generation, and requires a fresh state read before serving live data again.
- `cortex-mcp` - an MCP server for agentic patch editing through `cortex session`. Its read, recall, scene and working-copy tools are hardware-verified; persistent writes remain deliberately unavailable.
- `cortex-rs` - a leaf Rust crate providing the USB HID transport, Cortex Control framing and protobuf envelope, session/correlation, typed domain and client APIs, subscribed state reduction, and shared prepared-save safety.
- `cortex-host` - the shared synchronous daemon contract and local IPC facade used by host surfaces; it has no HID feature and cannot open the device. Unix uses user-owned domain sockets; Windows uses local-only named pipes protected by a current-owner ACL.

## Why the community is building it

Neural DSP provides Cortex Control for macOS and Windows but has not provided a Linux editor. This project began because Linux players still need full access to hardware they own, so the community is building that support for itself. Today that community effort is maintained by one person.

Linux is therefore the project's origin, its first implementation and its only hardware-verified host so far, not the intended boundary of the product. The shared Rust core, local IPC seam and Tauri frontend are being built toward one open-source GUI for Quad Cortex and Nano Cortex on Linux, Windows and macOS, alongside matching CLI and MCP interfaces where each device's capabilities permit them.

## What it is not

- Not a Neural DSP product, and not affiliated with Neural DSP.
- Not a re-distribution of Neural DSP binaries, firmware, or artwork.
- Not a device-rooting tool. It uses the USB HID route exclusively; the
  device-rooting route (OpenCortex) carries warranty risk that the USB route
  does not.

## Current Developer Setup

The hardware-verified path is Linux. Windows and macOS package work is active, but both remain provisional until their native package and CLI-through-device smokes pass. See the [GUI guide](https://pacharanero.github.io/cortex/gui/first-draft/#tester-preview-packages) and [Windows native/QEMU smoke runbook](https://pacharanero.github.io/cortex/gui/windows-smoke/) before testing the unsigned Windows installer.

### 1. udev rule

The Quad Cortex presents as USB `152a:880a` and the Nano Cortex as `152a:88e7`, both on HID interface 5. By default `/dev/hidraw*` is root-only, so install the repository's explicit two-device udev rule:

```sh
sudo install -m 0644 70-neural-dsp-cortex.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo udevadm trigger --action=add --subsystem-match=hidraw
```

Re-plug the device, then `ls -l /dev/hidraw*` should show `crw-rw----+` on the interface-5 node. The installed CLI can hold either product explicitly: Quad uses the full grid/session surface, while Nano currently exposes typed state plus non-persistent amp, bypass and raw FX parameter operations. Gate-reduction writing remains hardware-provisional.

### 2. Build

Install the native prerequisites in the [installation guide](https://pacharanero.github.io/cortex/install/), then build the surface you need:

```sh
cargo build -p cortex-cli
cargo build -p cortex-rs --no-default-features   # protocol/domain crate without hidapi
```

### 3. Run

```sh
cargo run -p cortex-cli -- device version
```

## Project layout

```text
crates/
  cortex-rs/    The leaf crate (transport, framing, session, client, live state,
                save safety, typed domain model, vendored protobuf schema).
  cortex-host/  Shared held-session daemon contract and local IPC facade.
  cortex-cli/   The `cortex` CLI - a thin surface over the crate.
  cortex-mcp/   Non-persistent MCP read, recall, scene and live-grid tools.
gui/           Cross-platform Tauri 2 + React + Mantine desktop editor
               (interactive fixture and daemon-backed modes; no persistent save).
docs/          Protocol notes, runbooks, GUI docs.
spec/          Living spec/design per zone; roadmap and completed work ledgers.
s/             Repo scripts: s/test, s/lint, s/gui-dev, s/version++ ...
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for the workflow, [SECURITY.md](SECURITY.md) to report a vulnerability privately, and [AGENTS.md](AGENTS.md) for the protocol invariants and prior-art licensing boundaries that apply before changing anything.

## Prior art and attribution

This project builds on the protocol work established by the MIT-licensed
[`stokes-audio/pyquadcortex`](https://github.com/stokes-audio/pyquadcortex)
project, which recovered the Cortex Control protobuf schema and documented the
USB HID framing, the trailer-tagged message envelope, and the benign write-STALL
gotcha. The recovered `.proto` files are vendored into
`crates/cortex-rs/proto/` under the MIT license's distribution terms.

The Tauri app architecture follows the precedent set by the Apache-2.0-licensed
[`rixrix/deskop-nano-cortex`](https://github.com/rixrix/deskop-nano-cortex)
project (Rust device I/O backend, honest verified-vs-provisional labelling, AFX
spec layout).

Full attribution and license texts are in `NOTICE` and
`THIRD-PARTY-NOTICES.md`.

## Licensing

- **Code:** GNU Affero General Public License v3.0 or later
  ([AGPL-3.0-or-later](LICENSE)). The work is not available for subsumption
  into proprietary products; dual licensing is available on request.
- **Written content:** Creative Commons Attribution-ShareAlike 4.0
  International (CC-BY-SA-4.0).
- **Vendored schema:** The recovered Cortex Control `.proto` files remain
  under the MIT license (copyright (c) 2026 Stokes); attribution is recorded
  in `NOTICE` and `THIRD-PARTY-NOTICES.md`.

As a non-binding ethical request, the project's own work should not be used in weaponry, immigration enforcement, or other activities that infringe human rights. This request is not an additional licence condition; the AGPL and CC-BY-SA grants above are the enforceable terms.
