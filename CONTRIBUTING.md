# Contributing

Thanks for considering a contribution to `cortex`. This is an unofficial, community-maintained interoperability project for the Neural DSP Quad Cortex and Nano Cortex; it is not affiliated with or endorsed by Neural DSP. This document sets expectations so maintainers and contributors can work effectively.

## Before You Start

Search existing issues and pull requests before opening a new one. For non-trivial changes, discuss the problem and approach first, since this project is protocol- and safety-sensitive: changes that touch device I/O, framing, or the MCP safety surface need agreement on approach before substantial code is written.

Read [AGENTS.md](AGENTS.md) before changing anything. It is the entry point for both human and AI contributors, and covers the prior-art licensing boundaries, the protocol invariants that must not break silently, and the MCP safety surface.

## Issues

Bug reports should include the expected and actual behaviour, exact reproduction steps, relevant environment details (OS, CorOS version, which device), and redacted logs.

**Never include real device data** in an issue: serial numbers, MAC addresses, firmware checksums, or your own preset and Neural Capture names. `s/lint-no-device-data` enforces this in the repository, but redact it from pasted logs and screenshots too.

Feature requests should describe the underlying problem before proposing a solution.

## Pull Requests

1. Create a descriptive branch from `main`.
2. Keep the pull request focused on one coherent change.
3. Run `s/lint` and `s/test` before pushing; frontend changes additionally need `npm run check` inside `gui/`. CI runs the same gates.
4. Update documentation (especially `docs/protocol.md` for protocol facts), tests, and `spec/roadmap.md` when they apply.
5. For protocol behaviour, validate against real hardware where possible and say so; otherwise mark the change provisional. The `pyquadcortex` offline test suite is a useful conformance reference but not a substitute for a hardware smoke run.
6. Explain the change and its rationale in the pull request description.

## Prior Art And Licensing

This project ports and adapts code from several reference projects under their own licenses (MIT, Apache-2.0). Read the prior-art table in [AGENTS.md](AGENTS.md) before reusing anything from a vendored reference repo, and record any derivation in `NOTICE` / `THIRD-PARTY-NOTICES.md` in the same change.

## Direction

The maintainer named in the README has final responsibility for scope and design decisions, including publishing to crates.io, cutting releases, and changing licensing or attribution. Discussion and constructive disagreement are welcome.

## Licence

By contributing, you agree that your contribution is licensed under this repository's licence: AGPL-3.0-or-later for code, CC-BY-SA-4.0 for written content (see [README.md](README.md#licensing)).

## Code Of Conduct

This project follows the [Code of Conduct](CODE_OF_CONDUCT.md). By participating, you agree to follow it.
