# Third-Party Notices

This file records third-party material incorporated or used as an architectural
precedent, followed by projects studied only as references. Each entry states
what this repository actually uses and the applicable licensing posture.

## stokes-audio/pyquadcortex

- **Project:** stokes-audio/pyquadcortex - Python library and `qcctl` CLI for
  the Neural DSP Quad Cortex over USB HID.
- **Repository:** <https://github.com/stokes-audio/pyquadcortex>
- **License:** MIT (see below)
- **Copyright:** (c) 2026 Stokes
- **Use in cortex-rs:** The recovered Cortex Control protobuf schema
  (`Preset.proto`, `ProductionAutomation.proto`) is vendored into
  `crates/cortex-rs/proto/` under the MIT license's distribution terms. HID
  framing, the trailer-tagged envelope, benign write-STALL handling,
  handshake/session/correlation design, catalog parsing, client-operation wire
  shapes, helper behaviour, and protocol documentation are derived from the
  corresponding pyquadcortex work and adapted to Rust. This project records
  hardware evidence per implemented operation; upstream verification is not
  treated as verification of this implementation.

### MIT License (pyquadcortex)

```text
MIT License

Copyright (c) 2026 Stokes

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## rixrix/deskop-nano-cortex

- **Project:** rixrix/deskop-nano-cortex - Tauri/Rust desktop companion for
  the Neural DSP Nano Cortex.
- **Repository:** <https://github.com/rixrix/deskop-nano-cortex>
- **License:** Apache-2.0 (see below)
- **Copyright:** (c) 2026 Richard Sentino
- **Use in cortex-rs:** Architectural and process precedent. Its Rust-owned
  device I/O and React webview boundary, managed state, bounded release on
  close, honest verified-vs-provisional capability model, mockable frontend
  boundary, AFX spec zones, traceability approach, version synchronization,
  release/DX tooling, and hardware-smoke evidence shape informed this
  project's GUI, specs, and plans. Its current-state decoder, fixed-chain
  domain model, Gate-reduction write layout, and Nano-specific FX model-id to
  display-name table were adapted in
  `crates/cortex-rs/src/nano.rs`, retaining the field-presence and
  provisional-evidence discipline while independently implementing strict
  malformed-input handling and the HID envelope boundary. The current-state
  decoder material credits `choldy/nano-cortex-web-editor`, so both upstreams
  are attributed here.

### Apache License 2.0 (deskop-nano-cortex)

A copy of the Apache License 2.0 is distributed in
`LICENSES/Apache-2.0.txt`.

## VanIseghemThomas/qc-stomp-tools

- **Project:** VanIseghemThomas/qc-stomp-tools - On-device C/Python for
  footswitches, rotaries, and LEDs via `ioctl`.
- **Repository:** <https://github.com/VanIseghemThomas/qc-stomp-tools>
- **License:** MIT
- **Copyright:** (c) 2023 Thomas Van Iseghem
- **Use in cortex-rs:** Reference only at this stage. Relevant only if
  cortex-rs ever targets on-device builds (footswitch/rotary/LED ioctls). No
  code has been copied. Should code be adapted in future, it will be recorded
  here and carry its upstream copyright.

## choldy/nano-cortex-web-editor

- **Project:** choldy/nano-cortex-web-editor - Web editor for the Neural DSP
  Nano Cortex.
- **Repository:** <https://github.com/choldy/nano-cortex-web-editor>
- **License:** MIT
- **Copyright:** (c) 2026 Nano Cortex Web Editor Contributors
- **Use in cortex-rs:** The Nano current-state command, protobuf field map, and
  Gate-reduction write layout used by `crates/cortex-rs/src/nano.rs` derive
  from this project through the Apache-2.0-licensed `deskop-nano-cortex` Rust
  implementation. The state decoder is tested against fictional data and
  independently hardware-verified over USB. The Gate-reduction writer is
  byte-tested against licensed vectors but remains hardware-provisional;
  upstream evidence is not treated as verification of this implementation.

### MIT License (nano-cortex-web-editor)

```text
MIT License

Copyright (c) 2026 Nano Cortex Web Editor Contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Reference-only projects (no material incorporated)

The following projects were studied for understanding but are **not
incorporated** into this repository. None provides a clear repository-wide
licence permitting reuse: some contain file-level notices, while others
declare no licence. No code, scripts, data, or prose from them has been
committed here. Findings are cited in this project's own words and linked to
their source, principally in `spec/prior-art.md` and `docs/protocol.md`.

- **VanIseghemThomas/OpenCortex** - <https://github.com/VanIseghemThomas/OpenCortex> - no repository-wide licence; some decryptor files carry file-level GPL notices while other material remains unlicensed. It documents the device-rooting route and provisional pre-CorOS-3 capture-encryption findings. Reference only; copy nothing.
- **roelj/qc-extras** - <https://github.com/roelj/qc-extras> - no repository-wide licence; source files carry GPL-3.0-or-later headers, but no root licence defines repository-wide scope. Its Quad Cortex cross-compilation notes are reference only until that scope is clarified.
- **hsaastamoinen/quad-cortex-usb-re-notes** - <https://github.com/hsaastamoinen/quad-cortex-usb-re-notes> - no licence declared. Its independent USB observations corroborate report geometry and the write STALL. Findings are cited in our own words; no captures or prose are copied.
- **vian21/toneparse** - <https://github.com/vian21/toneparse> - no licence declared. It parses Neural DSP desktop-plugin presets and Logic Pro channel strips, not Quad Cortex protobuf presets, and includes bundled third-party preset content. It is reference only and supplies no implementation material to this project.
