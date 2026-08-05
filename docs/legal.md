# Legal and attribution

## This project is unofficial

`cortex-rs` is not affiliated with, endorsed by, or supported by Neural DSP Technologies Oy.

**"Neural DSP", "Quad Cortex", "Nano Cortex", and "Cortex Control" are trademarks of Neural DSP Technologies.** They are used here only to identify the hardware this software interoperates with, which is nominative use. No endorsement is claimed or implied.

Other trademarks appear in model attribution text taken from the device - Marshall, Fender, Mesa/Boogie, ProCo, Universal Audio and others. Those marks belong to their respective owners. See [Model attribution](#model-attribution).

## Licence

- **Code** is [AGPL-3.0-or-later](https://www.gnu.org/licenses/agpl-3.0.html).
- **Documentation and written content** are [CC-BY-SA-4.0](https://creativecommons.org/licenses/by-sa/4.0/).

AGPL is deliberate: this toolkit is not available for subsumption into a proprietary product. If a closed derivative genuinely needs to exist, dual licensing can be discussed.

The MIT- and Apache-licensed prior art this project builds on remains under its own terms. Attribution is recorded, not relicensed.

## Prior art

This project is a Rust port of work others did first, and would not exist without it.

| Project | Licence | What we owe it |
| --- | --- | --- |
| [`stokes-audio/pyquadcortex`](https://github.com/stokes-audio/pyquadcortex) | MIT | **The foundation.** Established the protocol against real hardware, recovered the protobuf schema, and documented the framing, the trailer-tagged envelope, the write-STALL gotcha, the connect handshake, and the domain traps. The `.proto` files in this repo are vendored from it with its own SPDX header. |
| [`rixrix/deskop-nano-cortex`](https://github.com/rixrix/deskop-nano-cortex) | Apache-2.0 | The architectural precedent for the planned desktop app, and the verified-versus-provisional labelling discipline these docs use. |
| [`VanIseghemThomas/qc-stomp-tools`](https://github.com/VanIseghemThomas/qc-stomp-tools) | MIT | On-device footswitch and LED work, relevant only if we ever target on-device builds. |

Full attribution is in [`NOTICE`](https://github.com/pacharanero/cortex/blob/main/NOTICE) and [`THIRD-PARTY-NOTICES.md`](https://github.com/pacharanero/cortex/blob/main/THIRD-PARTY-NOTICES.md).

### Reference-only projects

These informed our understanding but lack a clear repository-wide licence, so none of their code, scripts, data, or prose appears here: [`OpenCortex`](https://github.com/VanIseghemThomas/OpenCortex) has no root licence and mixes unlicensed material with file-level GPL notices; [`roelj/qc-extras`](https://github.com/roelj/qc-extras) has GPL-3.0-or-later source headers but no root licence defining the repository-wide scope; [`hsaastamoinen/quad-cortex-usb-re-notes`](https://github.com/hsaastamoinen/quad-cortex-usb-re-notes) and [`vian21/toneparse`](https://github.com/vian21/toneparse) declare no licence. Findings are cited in our own words and linked to their source.

## Reverse engineering for interoperability

Reverse engineering to achieve interoperability is well established in law. In the UK, CDPA s50B permits decompilation to create an independent interoperable program, and s296A voids contract terms purporting to forbid it. EU Software Directive Article 6 is equivalent.

This is a device you own, and the aim is a Linux client for hardware whose vendor ships none.

We follow the norms the existing projects set:

- **No redistribution of Neural DSP binaries, firmware, or artwork.**
- **No raw USB captures published.** They contain readable preset, path, device, and build strings.
- **The recovered schema is limited to what interoperability requires.**
- **The USB route only.** We do not root the device, modify firmware, or touch the SD card. That route exists but carries warranty risk this one does not.
- **The work is clearly labelled unofficial.**

We have also deliberately *not* decompiled Cortex Control. The information we need is behavioural and obtainable from observing the wire, which is both cheaper and better evidence.

## Model attribution {#model-attribution}

When `cortex` shows text like `Based on Marshall® JCM800®`, **that string comes from the device and is Neural DSP's own wording.** The catalog carries it in a `tm` field on 318 of 533 models.

We reproduce it **verbatim** and never paraphrase it, for three reasons: it concerns other companies' trademarks; the vendor's own careful wording is more appropriate than anything we would write; and presenting our own mapping as authoritative would be both less accurate and less defensible.

Neural DSP name their models obliquely - "Brit 2203", not "Marshall JCM800" - and nothing in this project should undo that.

## Your data

`cortex` makes **no network connections whatsoever**. It has no telemetry, no analytics, no update check, and no cloud component. It talks to your Quad Cortex over USB and to nothing else.

The model catalog it reads from your device stays on your machine. It is Neural DSP's content and is not committed to this repository.
