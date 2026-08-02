# Third-Party Notices

This project incorporates material from third-party projects. Each is listed
below with its copyright, license, and a link to the upstream repository.

## stokes-audio/pyquadcortex

- **Project:** stokes-audio/pyquadcortex - Python library and `qcctl` CLI for
  the Neural DSP Quad Cortex over USB HID.
- **Repository:** <https://github.com/stokes-audio/pyquadcortex>
- **License:** MIT (see below)
- **Copyright:** (c) 2026 Stokes
- **Use in cortex-rs:** The recovered Cortex Control protobuf schema
  (`Preset.proto`, `ProductionAutomation.proto`) is vendored into
  `crates/cortex-rs/proto/` under the MIT license's distribution terms. The
  protocol framing, the benign write-STALL gotcha, and the trailer-tagged
  message envelope documented in this project are derived from
  pyquadcortex's protocol documentation and verified against a real Quad
  Cortex on Linux.

### MIT License (pyquadcortex)

```
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
- **Use in cortex-rs:** Architectural precedent only, at this stage. The
  Tauri app layout (Rust device I/O backend + React webview frontend, honest
  verified-vs-provisional labelling, AFX spec zones, release/dx tooling) is
  the model for the planned `gui/` surface in this project. No source code
  has been copied from deskop-nano-cortex; the design is adapted and
  re-implemented independently. Should code be adapted in future, it will be
  recorded here and carry its upstream copyright.

### Apache License 2.0 (deskop-nano-cortex)

A copy of the Apache License 2.0 is available at
<https://www.apache.org/licenses/LICENSE-2.0>.

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

## Reference-only projects (no material incorporated)

The following projects were studied for understanding but are **not
incorporated** into this repository. They carry no declared license (GitHub
reports `license: null`), so all rights are reserved by their authors; no
code, scripts, or prose from them has been committed into this repo. Findings
from them are cited in our own words in `quad-cortex-linux-editor-and-protocol.md`
(at the parent workspace root) and link out.

- **VanIseghemThomas/OpenCortex** - <https://github.com/VanIseghemThomas/OpenCortex> - device-rooting route (SD card, shadow swap, SSH). Not used; the USB route is preferred and carries no warranty risk.
- **roelj/qc-extras** - <https://github.com/roelj/qc-extras> - cross-compilation notes for the QC (ARMv7, ADSP-SC58x). Reference only.
- **hsaastamoinen/quad-cortex-usb-re-notes** - <https://github.com/hsaastamoinen/quad-cortex-usb-re-notes> - independent USB recon corroboration. Reference only.
- **vian21/toneparse** - <https://github.com/vian21/toneparse> - preset-file parser. Reference only; reimplemented independently if needed.